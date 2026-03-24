# OAuth bridge

OAuth redirects hit **`https://…/oauth-bridge/jira.html`** and **`github.html`**. From there:

1. **Desktop:** **Open Molt Hub** → **`molthub://`** → Tauri forwards to the local API (see `crates/tauri/tauri.conf.json`).
2. **Browser / `./dev.sh`:** **Finish in browser (local API)** → `http://127.0.0.1:13401/api/integrations/…/oauth/callback?…` (no extra provider registration; the provider already returned to this HTTPS page).

The default API origin is `http://127.0.0.1:13401`. If you use another port, open DevTools on the bridge page once and run:

`localStorage.setItem('moltHubLocalApiOrigin', 'http://127.0.0.1:YOUR_PORT')`

**Setup**

1. Host the HTML over HTTPS (e.g. GitHub Pages: [`.github/workflows/deploy-oauth-bridge.yml`](../.github/workflows/deploy-oauth-bridge.yml) — set Pages source to **GitHub Actions**, publish from `main`). After changing `github.html` / `jira.html`, redeploy so the hosted copy matches.
2. Register **exactly** those URLs in [Atlassian](https://developer.atlassian.com/console/myapps/) and your [GitHub OAuth App](https://github.com/settings/developers). GitHub allows [one callback URL](https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/authorizing-oauth-apps) per app.
3. Put the same URLs in **`redirect-uris.json`** (`jira` / `github`). They are embedded at build time; empty values panic unless you set **`MOLTHUB_JIRA_REDIRECT_URI`** / **`MOLTHUB_GITHUB_REDIRECT_URI`**. Forks should point this file at **their** hosted bridge URLs (or use those env vars).

**GitHub token exchange** needs a client secret at runtime: set **`MOLTHUB_GITHUB_CLIENT_SECRET`** or **`GITHUB_CLIENT_SECRET`** before starting the server (or compile with `GITHUB_CLIENT_SECRET` set).

**“Callback URL is invalid” (Atlassian)** — `redirect_uri` and console must match; client ID must be that app’s (`oauth.rs` default or **`MOLTHUB_JIRA_CLIENT_ID`**).

Any static HTTPS host works if paths match what you register.
