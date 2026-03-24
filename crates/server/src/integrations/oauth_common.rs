//! Shared OAuth helpers for third-party integrations (PKCE CSRF, env resolution, success HTML).
//!
//! Client IDs and secrets are resolved the same way for every provider so **dev** (`.env` on disk)
//! and **release** (injected env, optional compile-time `option_env!` defaults from your CI) behave
//! consistently.

/// Read the first non-empty trimmed value from the given environment variable names.
pub fn first_env_trimmed(keys: &[&'static str]) -> Option<String> {
    for key in keys {
        if let Ok(v) = std::env::var(key) {
            let t = v.trim().to_owned();
            if !t.is_empty() {
                return Some(t);
            }
        }
    }
    None
}

/// OAuth client secret: runtime env keys first, then optional compile-time embedding (no rebuild
/// needed when using runtime env in production).
pub fn resolve_oauth_secret(
    env_keys: &[&'static str],
    compile_time: Option<&'static str>,
) -> Option<String> {
    if let Some(s) = first_env_trimmed(env_keys) {
        return Some(s);
    }
    compile_time
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(std::string::ToString::to_string)
}

/// OAuth client ID: prefer runtime env, then optional compile-time default, then a crate-level
/// fallback string (upstream dev app — forks should set env or `option_env!` at build time).
pub fn resolve_client_id(
    env_keys: &[&'static str],
    compile_time: Option<&'static str>,
    fallback: &'static str,
) -> String {
    if let Some(id) = first_env_trimmed(env_keys) {
        return id;
    }
    if let Some(id) = compile_time.map(str::trim).filter(|s| !s.is_empty()) {
        return id.to_owned();
    }
    fallback.to_owned()
}

/// Cryptographically random CSRF `state` for OAuth authorize → callback correlation.
pub fn random_oauth_state() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Minimal HTML shown after a successful browser/deep-link OAuth callback.
pub fn oauth_success_html(provider: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <title>{provider} Connected</title>
  <style>
    *{{margin:0;padding:0;box-sizing:border-box}}
    body{{display:flex;align-items:center;justify-content:center;min-height:100vh;
         background:#0d0d14;font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif;color:#e2e2ea}}
    .card{{text-align:center;padding:48px 40px;background:#16161f;border:1px solid #2a2a3a;border-radius:12px;max-width:420px}}
    .icon{{font-size:48px;margin-bottom:16px}}
    h1{{font-size:20px;font-weight:600;margin-bottom:8px}}
    p{{font-size:14px;color:#888;margin-bottom:16px;line-height:1.5}}
    a{{color:#7c9cff;text-decoration:none}}
    a:hover{{text-decoration:underline}}
    .links{{font-size:13px;color:#888;margin-bottom:20px}}
    .links a{{display:inline-block;margin:6px 8px}}
    .closing{{font-size:12px;color:#555}}
  </style>
</head>
<body>
  <div class="card">
    <div class="icon">✓</div>
    <h1>{provider} connected</h1>
    <p>Return to the UI to use this integration. If this tab does not close, use a link below.</p>
    <p class="links">
      <a href="/settings">Open Settings (this origin)</a><br/>
      <a href="http://127.0.0.1:5173/settings">Vite dev UI (port 5173)</a>
    </p>
    <div class="closing">This tab will try to close automatically…</div>
  </div>
  <script>
    setTimeout(function(){{window.close()}}, 2000);
  </script>
</body>
</html>"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_oauth_state_hex_len() {
        let t = random_oauth_state();
        assert_eq!(t.len(), 32);
        assert!(t.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
