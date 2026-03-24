# OAuth bridge (HTTPS → `molthub://`)

Providers require **HTTPS** redirect URIs. `jira.html` and `github.html` receive `code` / `state` (or `error`) and redirect into the desktop app via **`molthub://`** (see `crates/tauri/tauri.conf.json` to change the scheme; keep the HTML in sync).

## 1. Deploy these files over HTTPS

Example paths on a **GitHub Pages project site**:

- `https://<user>.github.io/<repo>/oauth-bridge/jira.html`
- `https://<user>.github.io/<repo>/oauth-bridge/github.html`

Register those URLs **exactly** in Atlassian and GitHub.

## 2. Point release builds at the same URLs

Edit **`redirect-uris.json`** in this folder: set `jira` and `github` to the HTTPS URLs above. Release builds embed that file; no env vars on user machines.

**Debug** builds ignore it and use `http://127.0.0.1:<port>/api/integrations/{jira|github}/oauth/callback` for local OAuth apps.

Optional env overrides (no rebuild): `MOLTHUB_JIRA_REDIRECT_URI`, `MOLTHUB_GITHUB_REDIRECT_URI`.

## GitHub Actions (recommended)

Workflow: [`.github/workflows/deploy-oauth-bridge.yml`](../.github/workflows/deploy-oauth-bridge.yml).

1. **Settings → Pages → Build and deployment → Source:** **GitHub Actions**
2. Push to **`main`** (or run the workflow manually). HTML is published under **`/oauth-bridge/`**.

If your default branch is not `main`, update the workflow `branches` list.

## Other hosting

- **Pages from a branch:** serve this folder (or copy into `/docs`) and match paths when registering OAuth callbacks.
- **Custom domain:** use that host in provider settings and in `redirect-uris.json`.
- **Elsewhere:** any static HTTPS host is fine (Netlify, Cloudflare Pages, S3+CDN, etc.).
