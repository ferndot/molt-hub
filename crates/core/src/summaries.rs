//! AI-summarised status types (T27 prep).
//!
//! This module defines the *types* for requesting and caching LLM-generated
//! summaries of agent output.  The actual LLM integration lives in the server
//! crate; this module has no I/O dependencies.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::model::TaskId;

// ---------------------------------------------------------------------------
// SummaryRequest
// ---------------------------------------------------------------------------

/// A request to summarise recent agent output for a task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SummaryRequest {
    /// The task whose agent output should be summarised.
    pub task_id: TaskId,
    /// The tail of agent output to summarise (last N lines or bytes).
    pub agent_output: String,
    /// Additional context passed to the summarisation model
    /// (e.g. task title, current stage, pipeline name).
    pub context: Option<String>,
}

// ---------------------------------------------------------------------------
// SummaryResponse
// ---------------------------------------------------------------------------

/// A generated summary returned by the LLM layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SummaryResponse {
    /// The generated summary text.
    pub summary_text: String,
    /// Model-reported confidence in the summary, in the range `[0.0, 1.0]`.
    /// `None` if the model does not report confidence.
    pub confidence: Option<f64>,
    /// Wall-clock time the summary was generated.
    pub generated_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// SummaryCache trait
// ---------------------------------------------------------------------------

/// A key-value cache for `SummaryResponse` values, keyed by `TaskId`.
///
/// Implementations may be in-memory, Redis-backed, SQLite-backed, etc.
/// TTL enforcement is the responsibility of the implementor.
pub trait SummaryCache: Send + Sync {
    /// Retrieve a cached summary for the given task, if one exists and has not expired.
    fn get(&self, task_id: &TaskId) -> Option<SummaryResponse>;

    /// Store a summary for the given task with the given TTL in seconds.
    ///
    /// If `ttl_seconds` is `None`, the entry should be treated as non-expiring.
    fn set(&self, task_id: TaskId, response: SummaryResponse, ttl_seconds: Option<u64>);

    /// Evict the cached summary for the given task, if any.
    fn invalidate(&self, task_id: &TaskId);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::TaskId;

    #[test]
    fn summary_request_roundtrip() {
        let id = TaskId::new();
        let req = SummaryRequest {
            task_id: id.clone(),
            agent_output: "Agent finished step 1.\nAgent finished step 2.".to_string(),
            context: Some("Pipeline: dev, Stage: review".to_string()),
        };
        let json = serde_json::to_string(&req).expect("serialize");
        let back: SummaryRequest = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.task_id, id);
        assert_eq!(back.agent_output, req.agent_output);
    }

    #[test]
    fn summary_response_roundtrip() {
        let resp = SummaryResponse {
            summary_text: "Agent completed code review with 3 findings.".to_string(),
            confidence: Some(0.87),
            generated_at: Utc::now(),
        };
        let json = serde_json::to_string(&resp).expect("serialize");
        let back: SummaryResponse = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.summary_text, resp.summary_text);
        assert_eq!(back.confidence, resp.confidence);
    }

    #[test]
    fn summary_response_no_confidence() {
        let resp = SummaryResponse {
            summary_text: "Minimal summary.".to_string(),
            confidence: None,
            generated_at: Utc::now(),
        };
        let json = serde_json::to_string(&resp).expect("serialize");
        let back: SummaryResponse = serde_json::from_str(&json).expect("deserialize");
        assert!(back.confidence.is_none());
    }
}
