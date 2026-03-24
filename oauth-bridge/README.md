# OAuth bridge

OAuth redirects hit **`https://…/oauth-bridge/jira.html`** and **`github.html`**, then **`molthub://`** opens the desktop app (scheme in `crates/tauri/tauri.conf.json`). Providers never see a `localhost` callback.

**Setup**

1. Host the HTML over HTTPS (e.g. GitHub Pages: [`.github/workflows/deploy-oauth-bridge.yml`](../.github/workflows/deploy-oauth-bridge.yml) — set Pages source to **GitHub Actions**, publish from `main`).
2. Register **exactly** those URLs in [Atlassian](https://developer.atlassian.com/console/myapps/) and your [GitHub OAuth App](https://github.com/settings/developers). GitHub allows [one callback URL](https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/authorizing-oauth-apps) per app.
3. Put the same URLs in **`redirect-uris.json`** (`jira` / `github`). They are embedded at build time; empty values panic unless you set **`MOLTHUB_JIRA_REDIRECT_URI`** / **`MOLTHUB_GITHUB_REDIRECT_URI`**.

**“Callback URL is invalid” (Atlassian)** — `redirect_uri` and console must match; client ID must be that app’s (`oauth.rs` default or **`MOLTHUB_JIRA_CLIENT_ID`**).

Any static HTTPS host works if paths match what you register.
