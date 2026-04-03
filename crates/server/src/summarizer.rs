//! AI-summarized status — fast model summarization with content-hash caching.
//!
//! # Architecture
//!
//! `SummaryService` accepts raw agent output and returns a concise, tweet-length
//! (max 280 char) summary. Summaries are cached by content hash so unchanged
//! output is never re-summarized.
//!
//! The service is model-agnostic: any backend that implements [`SummaryProvider`]
//! can be plugged in. A [`MockSummaryProvider`] is included for deterministic
//! testing, and [`ClaudeSummaryProvider`] provides the API request shape for
//! Anthropic's Claude model (without making real HTTP calls).

use std::collections::HashMap;
use std::future::Future;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::RwLock;

// ---------------------------------------------------------------------------
// SummaryProvider trait
// ---------------------------------------------------------------------------

/// Model-agnostic summarization backend.
///
/// Implementations receive raw content and a token budget, returning a concise
/// summary paragraph (max 280 chars).
pub trait SummaryProvider: Send + Sync {
    /// Summarize `content` using at most `max_tokens` model tokens.
    fn summarize(
        &self,
        content: &str,
        max_tokens: usize,
    ) -> impl Future<Output = Result<String, SummaryError>> + Send;
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors that can occur during summarization.
#[derive(Debug, thiserror::Error)]
pub enum SummaryError {
    #[error("provider error: {0}")]
    Provider(String),
    #[error("content is empty")]
    EmptyContent,
}

// ---------------------------------------------------------------------------
// CachedSummary
// ---------------------------------------------------------------------------

/// A cached summary entry with creation timestamp and content hash.
#[derive(Debug, Clone)]
pub struct CachedSummary {
    /// The generated summary text.
    pub summary: String,
    /// When this cache entry was created.
    pub created_at: Instant,
    /// Content hash used as the cache key.
    pub hash: u64,
}

// ---------------------------------------------------------------------------
// SummaryServiceConfig
// ---------------------------------------------------------------------------

/// Configuration for [`SummaryService`].
#[derive(Debug, Clone)]
pub struct SummaryServiceConfig {
    /// Time-to-live for cache entries. Default: 5 minutes.
    pub ttl: Duration,
    /// Maximum number of cache entries before LRU eviction. Default: 1000.
    pub max_entries: usize,
    /// Maximum character length for summaries. Default: 280.
    pub max_summary_chars: usize,
    /// Token budget passed to the provider. Default: 100.
    pub max_tokens: usize,
}

impl Default for SummaryServiceConfig {
    fn default() -> Self {
        Self {
            ttl: Duration::from_secs(300),
            max_entries: 1000,
            max_summary_chars: 280,
            max_tokens: 100,
        }
    }
}

// ---------------------------------------------------------------------------
// SummaryService
// ---------------------------------------------------------------------------

/// Content-hash-cached summarization service.
///
/// Thread-safe via `tokio::sync::RwLock`. Evicts the oldest entry (LRU) when
/// the cache exceeds `max_entries`.
pub struct SummaryService<P: SummaryProvider> {
    provider: P,
    cache: Arc<RwLock<HashMap<u64, CachedSummary>>>,
    config: SummaryServiceConfig,
}

impl<P: SummaryProvider> SummaryService<P> {
    /// Create a new service with the given provider and default configuration.
    pub fn new(provider: P) -> Self {
        Self {
            provider,
            cache: Arc::new(RwLock::new(HashMap::new())),
            config: SummaryServiceConfig::default(),
        }
    }

    /// Create a new service with explicit configuration.
    pub fn with_config(provider: P, config: SummaryServiceConfig) -> Self {
        Self {
            provider,
            cache: Arc::new(RwLock::new(HashMap::new())),
            config,
        }
    }

    /// Summarize `content`, returning a cached result when available.
    pub async fn summarize(&self, content: &str) -> Result<String, SummaryError> {
        if content.is_empty() {
            return Err(SummaryError::EmptyContent);
        }

        let hash = Self::hash_content(content);

        // --- Fast path: cache hit with valid TTL ---
        {
            let cache = self.cache.read().await;
            if let Some(entry) = cache.get(&hash) {
                if entry.created_at.elapsed() < self.config.ttl {
                    return Ok(entry.summary.clone());
                }
            }
        }

        // --- Slow path: call provider, store result ---
        let summary = self
            .provider
            .summarize(content, self.config.max_tokens)
            .await?;

        // Truncate to max chars if the provider exceeded the limit.
        let summary = if summary.len() > self.config.max_summary_chars {
            summary[..self.config.summary_truncation_point(&summary)].to_string()
        } else {
            summary
        };

        let entry = CachedSummary {
            summary: summary.clone(),
            created_at: Instant::now(),
            hash,
        };

        {
            let mut cache = self.cache.write().await;

            // LRU eviction: remove the oldest entry if at capacity.
            if cache.len() >= self.config.max_entries && !cache.contains_key(&hash) {
                Self::evict_oldest(&mut cache);
            }

            cache.insert(hash, entry);
        }

        Ok(summary)
    }

    /// Return current number of cached entries.
    pub async fn cache_len(&self) -> usize {
        self.cache.read().await.len()
    }

    /// Compute a deterministic hash for the given content.
    fn hash_content(content: &str) -> u64 {
        let mut hasher = DefaultHasher::new();
        content.hash(&mut hasher);
        hasher.finish()
    }

    /// Evict the oldest cache entry by `created_at`.
    fn evict_oldest(cache: &mut HashMap<u64, CachedSummary>) {
        if let Some((&oldest_key, _)) = cache.iter().min_by_key(|(_, v)| v.created_at) {
            cache.remove(&oldest_key);
        }
    }
}

impl SummaryServiceConfig {
    /// Find a char-boundary-safe truncation point.
    fn summary_truncation_point(&self, s: &str) -> usize {
        let mut end = self.max_summary_chars;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        end
    }
}

// ---------------------------------------------------------------------------
// MockSummaryProvider
// ---------------------------------------------------------------------------

/// Deterministic provider for testing. Returns a fixed-format summary and
/// tracks the number of calls made.
pub struct MockSummaryProvider {
    call_count: AtomicUsize,
}

impl MockSummaryProvider {
    pub fn new() -> Self {
        Self {
            call_count: AtomicUsize::new(0),
        }
    }

    /// Number of times `summarize` has been invoked.
    pub fn call_count(&self) -> usize {
        self.call_count.load(Ordering::SeqCst)
    }
}

impl Default for MockSummaryProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl SummaryProvider for MockSummaryProvider {
    fn summarize(
        &self,
        content: &str,
        _max_tokens: usize,
    ) -> impl Future<Output = Result<String, SummaryError>> + Send {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        let len = content.len();
        let preview: String = content.chars().take(40).collect();
        let summary = format!("Summary ({len} chars): {preview}");
        async move { Ok(summary) }
    }
}

// ---------------------------------------------------------------------------
// ClaudeSummaryProvider
// ---------------------------------------------------------------------------

/// Provider that calls the Anthropic Messages API to produce real summaries.
#[derive(Debug)]
pub struct ClaudeSummaryProvider {
    /// Model identifier, e.g. `"claude-sonnet-4-20250514"`.
    pub model: String,
    /// Anthropic API key (`x-api-key` header).
    api_key: String,
    /// Reusable HTTP client.
    client: reqwest::Client,
}

impl ClaudeSummaryProvider {
    /// Create from an explicit model name; reads `ANTHROPIC_API_KEY` from the
    /// environment.  Returns an error if the variable is absent.
    pub fn new(model: impl Into<String>) -> Result<Self, SummaryError> {
        Self::new_from_env(model)
    }

    /// Create from env.  `ANTHROPIC_API_KEY` must be set.
    pub fn new_from_env(model: impl Into<String>) -> Result<Self, SummaryError> {
        let api_key = std::env::var("ANTHROPIC_API_KEY").map_err(|_| {
            SummaryError::Provider("ANTHROPIC_API_KEY env var is not set".to_string())
        })?;
        Ok(Self {
            model: model.into(),
            api_key,
            client: reqwest::Client::new(),
        })
    }

    /// Build the JSON request body sent to the Anthropic Messages API.
    pub fn build_request(&self, content: &str, max_tokens: usize) -> serde_json::Value {
        serde_json::json!({
            "model": self.model,
            "max_tokens": max_tokens,
            "system": "You are a concise status summarizer. Produce a single paragraph of at most 280 characters summarizing the agent output below.",
            "messages": [
                {
                    "role": "user",
                    "content": content,
                }
            ]
        })
    }
}

impl SummaryProvider for ClaudeSummaryProvider {
    fn summarize(
        &self,
        content: &str,
        max_tokens: usize,
    ) -> impl Future<Output = Result<String, SummaryError>> + Send {
        let body = self.build_request(content, max_tokens);
        let client = self.client.clone();
        let api_key = self.api_key.clone();

        async move {
            let response = client
                .post("https://api.anthropic.com/v1/messages")
                .header("x-api-key", &api_key)
                .header("anthropic-version", "2023-06-01")
                .header("content-type", "application/json")
                .json(&body)
                .send()
                .await
                .map_err(|e| SummaryError::Provider(format!("HTTP request failed: {e}")))?;

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                return Err(SummaryError::Provider(format!(
                    "Anthropic API returned {status}: {text}"
                )));
            }

            let json: serde_json::Value = response
                .json()
                .await
                .map_err(|e| SummaryError::Provider(format!("Failed to parse response: {e}")))?;

            let text = json["content"][0]["text"]
                .as_str()
                .ok_or_else(|| {
                    SummaryError::Provider(format!(
                        "Unexpected response shape: {}",
                        json
                    ))
                })?
                .to_string();

            Ok(text)
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    // -- Hash stability --

    #[test]
    fn same_content_produces_same_hash() {
        let content = "Agent completed task: deploy v1.2.3 to staging";
        let h1 = SummaryService::<MockSummaryProvider>::hash_content(content);
        let h2 = SummaryService::<MockSummaryProvider>::hash_content(content);
        assert_eq!(h1, h2);
    }

    #[test]
    fn different_content_produces_different_hash() {
        let h1 = SummaryService::<MockSummaryProvider>::hash_content("content A");
        let h2 = SummaryService::<MockSummaryProvider>::hash_content("content B");
        assert_ne!(h1, h2);
    }

    // -- Cache hit --

    #[tokio::test]
    async fn cache_hit_returns_without_calling_provider() {
        let provider = MockSummaryProvider::new();
        let service = SummaryService::new(provider);

        let content = "Agent output: migration completed successfully";

        // First call: cache miss — provider invoked.
        let s1 = service.summarize(content).await.unwrap();
        assert_eq!(service.provider.call_count(), 1);

        // Second call: cache hit — provider NOT invoked.
        let s2 = service.summarize(content).await.unwrap();
        assert_eq!(service.provider.call_count(), 1);

        assert_eq!(s1, s2);
    }

    // -- Cache miss --

    #[tokio::test]
    async fn cache_miss_calls_provider_and_stores_result() {
        let provider = MockSummaryProvider::new();
        let service = SummaryService::new(provider);

        assert_eq!(service.cache_len().await, 0);

        let _ = service.summarize("hello world").await.unwrap();

        assert_eq!(service.provider.call_count(), 1);
        assert_eq!(service.cache_len().await, 1);
    }

    // -- TTL expiry --

    #[tokio::test]
    async fn ttl_expiry_causes_re_summarization() {
        let provider = MockSummaryProvider::new();
        let config = SummaryServiceConfig {
            ttl: Duration::from_millis(50),
            ..Default::default()
        };
        let service = SummaryService::with_config(provider, config);

        let content = "Agent output: tests passing";

        let _ = service.summarize(content).await.unwrap();
        assert_eq!(service.provider.call_count(), 1);

        // Wait for TTL to expire.
        tokio::time::sleep(Duration::from_millis(80)).await;

        let _ = service.summarize(content).await.unwrap();
        assert_eq!(service.provider.call_count(), 2);
    }

    // -- LRU eviction --

    #[tokio::test]
    async fn lru_eviction_when_cache_is_full() {
        let provider = MockSummaryProvider::new();
        let config = SummaryServiceConfig {
            max_entries: 3,
            ..Default::default()
        };
        let service = SummaryService::with_config(provider, config);

        // Fill the cache to capacity.
        for i in 0..3 {
            service.summarize(&format!("content-{i}")).await.unwrap();
        }
        assert_eq!(service.cache_len().await, 3);

        // Adding a 4th entry triggers eviction — cache stays at max.
        service.summarize("content-3").await.unwrap();
        assert_eq!(service.cache_len().await, 3);
    }

    // -- Concurrent access --

    #[tokio::test]
    async fn concurrent_access_is_safe() {
        let provider = MockSummaryProvider::new();
        let service = Arc::new(SummaryService::new(provider));

        let mut handles = Vec::new();
        for i in 0..20 {
            let svc = Arc::clone(&service);
            handles.push(tokio::spawn(async move {
                svc.summarize(&format!("concurrent-content-{i}"))
                    .await
                    .unwrap()
            }));
        }

        let mut results = Vec::new();
        for handle in handles {
            results.push(handle.await.unwrap());
        }

        assert_eq!(results.len(), 20);
        assert!(service.cache_len().await <= 20);
    }

    // -- Empty content --

    #[tokio::test]
    async fn empty_content_returns_error() {
        let service = SummaryService::new(MockSummaryProvider::new());
        let err = service.summarize("").await.unwrap_err();
        assert!(matches!(err, SummaryError::EmptyContent));
    }

    // -- Truncation --

    #[tokio::test]
    async fn summary_is_truncated_to_max_chars() {
        let provider = MockSummaryProvider::new();
        let config = SummaryServiceConfig {
            max_summary_chars: 30,
            ..Default::default()
        };
        let service = SummaryService::with_config(provider, config);

        // MockSummaryProvider generates "Summary (N chars): ..." which can exceed 30.
        let result = service
            .summarize("This is a very long piece of content that should produce a summary exceeding 30 characters")
            .await
            .unwrap();

        assert!(result.len() <= 30);
    }

    // -- ClaudeSummaryProvider request shape --

    #[test]
    fn claude_provider_builds_correct_request_shape() {
        // Temporarily set env var so construction succeeds.
        std::env::set_var("ANTHROPIC_API_KEY", "test-key");
        let provider = ClaudeSummaryProvider::new("claude-sonnet-4-20250514").unwrap();
        std::env::remove_var("ANTHROPIC_API_KEY");

        let req = provider.build_request("test content", 100);

        assert_eq!(req["model"], "claude-sonnet-4-20250514");
        assert_eq!(req["max_tokens"], 100);
        assert!(req["system"].as_str().unwrap().contains("280 characters"));
        assert_eq!(req["messages"][0]["role"], "user");
        assert_eq!(req["messages"][0]["content"], "test content");
    }

    /// Requires `ANTHROPIC_API_KEY` to be set in the environment.
    /// Run with: ANTHROPIC_API_KEY=sk-... cargo test claude_provider_live
    #[tokio::test]
    #[ignore]
    async fn claude_provider_returns_live_summary() {
        let provider = ClaudeSummaryProvider::new("claude-haiku-4-20250514")
            .expect("ANTHROPIC_API_KEY must be set to run this test");
        let result = provider
            .summarize("Agent completed: ran unit tests, all 42 passed.", 100)
            .await
            .unwrap();
        assert!(!result.is_empty());
        assert!(result.len() <= 280);
    }

    #[test]
    fn claude_provider_new_fails_without_api_key() {
        std::env::remove_var("ANTHROPIC_API_KEY");
        let err = ClaudeSummaryProvider::new("claude-sonnet-4-20250514").unwrap_err();
        assert!(matches!(err, SummaryError::Provider(_)));
    }
}
