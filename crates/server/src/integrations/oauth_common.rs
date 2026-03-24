//! Shared OAuth helpers (PKCE CSRF, success HTML).
//!
//! OAuth **app** credentials: [`super::oauth_clients`] only (`oauth-clients.json`).

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
