# Aether

A web app for managing compute environments and AI engines on a Kubernetes
namespace — launch JupyterLab/RStudio environments or LLM inference engines
(Ollama, vLLM, SGLang) with a few clicks, behind a login.

It's a scoped-down slice of the broader platform described in `SPEC.md`,
built to what's actually buildable today: this cluster has no ingress
controller, so there's no Gateway layer, and a StorageClass is only just
being added (see "Status & known limitations" below) — Aether's own
Postgres is wired up to use it (CloudNativePG, see "Deploying to
Kubernetes" below) but isn't live yet.

There are two account roles: **admin** and **user**. Both can use Pods and
Launch; only admins see Templates and Users.

- **Pods** — shows the running pods live, with their basic resource info:
  CPU/memory requests and limits, accelerators (GPUs, etc.), status, node,
  restarts, and age. A regular `user` only sees pods they launched
  themselves; an `admin` sees every pod in the namespace plus an **Owner**
  column showing who launched each one. A **Credential** column shows the
  auto-generated login token/API key for templates that have one,
  click-to-select for copying — and for proxy-enabled templates (JupyterLab,
  RStudio), an **Open** link that lands you in an already-logged-in session
  with zero copy-paste (see "Ownership, auto-generated credentials, and the
  reverse proxy" below). Click a row to open its detail panel: per-container
  state and failure reason (`CrashLoopBackOff`, `ImagePullBackOff`, exit
  codes, etc.), recent Kubernetes Events, and a log viewer (container
  picker, tail length, previous-container logs for ones that crashed).
- **Launch** — creates a `Deployment` in that namespace, owned by whichever
  account launched it, and if a container port is given, also a Service
  exposing it — `LoadBalancer` (public, since there's no ingress) by
  default, or `ClusterIP`-only for templates where Aether's own proxy is the
  intended (and, for RStudio, only) way in (JupyterLab, RStudio). The form's
  first field is a **Template** dropdown — Ollama,
  vLLM, SGLang (Intelligence Layer) or JupyterLab, RStudio (Interface
  Layer), or **Custom** — which pre-fills image, port, resource sizing, GPU
  defaults, and any default env vars/args for that template. Every
  pre-filled field stays editable, and Custom picks any image from the
  Postgres-backed image catalog instead. Templates that carry a
  `secret_env_key` (JupyterLab, vLLM) hide that field from the form
  entirely and generate a random value for it at launch time instead —
  shown once in the success message and persistently on the Pods tab, no
  need to invent or type one yourself. JupyterLab and RStudio are also
  reachable by clicking "Open" — Aether proxies straight into an
  already-authenticated (or, for RStudio, auth-free) session.
- **Templates** *(admin only)* — CRUD for the templates the Launch tab
  offers: a table of existing templates (edit/delete) and a form to add a
  new one (same fields as a template pre-fills into Launch, plus notes shown
  when it's selected there, plus an optional "auto-generate a secret for
  this env var" field and a "proxy through Aether" checkbox — see below).
- **Users** *(admin only)* — create accounts (username, password, role),
  delete them, and reset any account's password without knowing the old one
  (forces that account to log in again everywhere, on every device). Still
  no self-service signup — an admin creates every account.
- **Activity** — login history (when, from what IP/browser) and launch
  history (what template/image/resources someone launched), kept for
  support/metrics. Same visibility split as Pods: a `user` account sees only
  its own rows, an `admin` sees everyone's with an added Username column.

Any logged-in user (either role) can change their own password via
**Change password** in the header, which does require the current one.

- **Backend**: Rust, [Axum](https://github.com/tokio-rs/axum) +
  [kube-rs](https://kube.rs) + [sqlx](https://github.com/launchbadge/sqlx)
  (Postgres). Watches one namespace via the Kubernetes watch API and serves a
  REST snapshot plus a WebSocket that pushes live pod updates; also serves the
  image/template catalogs, authentication, and creates Deployments on request.
- **Frontend**: Rust, [Leptos](https://leptos.dev) (client-side, compiled to
  WASM with [Trunk](https://trunkrs.dev)). Loads the initial pod snapshot over
  REST, then stays in sync over the WebSocket.
- **Repo**: `ssh://git@git.example.com:2022/Aether/Aether-Web.git`
  (app code — this repo). Kubernetes manifests and the Argo CD `Application`
  live in a separate
  [**Aether-Deploy**](https://git.example.com/Aether/Aether-Deploy)
  repo — see "GitOps deploy (Argo CD)" below for why.

## Layout

```
common/                    Shared types (PodInfo, TemplateEntry, UserInfo, CreateDeploymentRequest, ...)
backend/                   Axum server, kube-rs watcher, sqlx/Postgres catalogs + auth
backend/migrations/        sqlx migrations, auto-run on startup
backend/src/auth.rs        Password hashing, session cookie, CurrentUser/AdminUser extractors, login/logout/me
backend/src/users.rs       Users admin CRUD (admin-only)
backend/src/validate.rs    Input validation (k8s names, ports, quantities, env keys, ...)
backend/src/visibility.rs  Per-user pod filtering + credential/proxy-path enrichment (admin sees all, user sees own)
backend/src/proxy.rs       Reverse proxy for proxy-enabled templates (JupyterLab) — ClusterIP connection + credential injection
backend/src/deployments.rs Create/get/update/delete a Deployment, ownership checks, launch history
backend/src/quota.rs       Global/per-user quota settings + enforcement, usage computed from the pod watcher's cache
frontend/src/login.rs      Login page
frontend/src/pods_tab.rs   Pods tab + detail panel
frontend/src/deployment_manage.rs  Scale/edit/delete panel for a Deployment, shown in the pod detail panel
frontend/src/quotas_tab.rs Quotas admin tab (global defaults + per-user overrides)
frontend/src/create_deployment_tab.rs   Launch tab (template dropdown + form)
frontend/src/templates_tab.rs   Templates admin tab (CRUD)
frontend/src/images_tab.rs      Images admin tab (CRUD, backs "Custom" mode on Launch)
frontend/src/users_tab.rs       Users admin tab (CRUD)
frontend/src/env_editor.rs      Shared add/remove env-var-row widget (Launch + Templates)
frontend/src/theme.rs           Light/dark theme toggle (data-theme attribute + localStorage)
Dockerfile                 Multi-stage build: compiles both crates, ships a distroless image
.forgejo/                  Forgejo Actions workflow that builds and pushes the image, then bumps the deploy repo
SPEC.md                    The broader platform vision this app is a slice of
```

Kubernetes manifests and the Argo CD `Application` that deploys this app
live in a separate repo, **Aether-Deploy** — not here. See "GitOps deploy
(Argo CD)" below for why, and for how the two repos fit together.

## Running locally

Prerequisites:

```
rustup target add wasm32-unknown-unknown
cargo install trunk
```

You'll also need a Postgres instance for the image/template catalogs and
accounts. For local dev, any throwaway instance works — the backend creates
its tables itself on startup:

```
docker run -d -p 5432:5432 -e POSTGRES_PASSWORD=postgres postgres:16-alpine
```

Build the frontend, then run the backend (which also serves the built frontend):

```
cd frontend && trunk build --release && cd ..
cd backend
NAMESPACE=<namespace-to-watch> \
DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres \
ADMIN_BOOTSTRAP_PASSWORD=<pick-something> \
cargo run --release -- --static-dir ../frontend/dist
```

`ADMIN_BOOTSTRAP_PASSWORD` only matters the *first* time — it creates a
username `admin` account if (and only if) the `users` table is empty. Without
it on a fresh database, the app starts fine but nobody can log in (the
backend logs a warning saying so). It's ignored on every later run once a
user exists.

Open `http://localhost:3000`, log in as `admin`. Auth to the cluster
auto-detects: in-cluster service account first, falling back to your local
kubeconfig (`~/.kube/config`, current context) — the same as `kubectl`.

### Backend configuration

All flags can also be set as environment variables:

| Flag / env var | Default | Description |
|---|---|---|
| `--namespace` / `NAMESPACE` | *(required)* | Namespace to watch and to create Deployments in |
| `--bind-addr` / `BIND_ADDR` | `0.0.0.0:3000` | Address the HTTP server binds to |
| `--static-dir` / `STATIC_DIR` | `frontend/dist` | Directory of the built frontend to serve |
| `--database-url` / `DATABASE_URL` | *(required)* | Postgres connection string for the image/template catalogs and accounts |
| `--admin-bootstrap-password` / `ADMIN_BOOTSTRAP_PASSWORD` | *(none)* | Creates the initial `admin` account on first run only; ignored once any user exists |
| `--app-origin` / `APP_ORIGIN` | *(none)* | Public origin this app is served from, e.g. `https://aether.example.com`. Must be set together with `--proxy-base-domain` |
| `--proxy-base-domain` / `PROXY_BASE_DOMAIN` | *(none)* | Base domain for per-deployment proxy origins, e.g. `proxy.aether.example.com`. Needs wildcard DNS + TLS for `*.<domain>`. **Leaving it unset is only appropriate for local development** — see "Per-deployment proxy origins" below |

### Endpoints

All endpoints below except `POST /api/login` and static assets require a
valid session cookie (401 if missing/expired); the ones marked *(admin)*
additionally require the `admin` role (403 otherwise).

- `POST /api/login` — body `{username, password}`; sets the `aether_session` cookie and returns the logged-in `UserInfo` on success, 401 on bad credentials
- `POST /api/logout` — clears the session (both server-side and the cookie)
- `GET /api/me` — returns the current `UserInfo` (`{id, username, role}`), or 401 if not logged in — this is what the frontend polls on load to decide whether to show the login page
- `PUT /api/me/password` — body `{current_password, new_password}`; changes your own password, 400 if `current_password` doesn't match. Deletes every other session for your account (`DELETE FROM sessions WHERE user_id = $1 AND token != $2`) but leaves the one making this request logged in.
- `GET /api/users` *(admin)* — list accounts (id, username, role, node_label — never password hashes)
- `POST /api/users` *(admin)* — create an account; body `{username, password, role}` (`role` is `"admin"` or `"user"`); username 3-32 chars, password ≥ 8 chars
- `DELETE /api/users/{id}` *(admin)* — delete an account; an admin can't delete their own account (guards against an easy self-lockout)
- `PUT /api/users/{id}/password` *(admin)* — body `{password}`; resets another account's password without needing the old one — the admin role itself is the authorization. Deletes **all** of that account's sessions (there's no "current session" to preserve, since it isn't the admin's own).
- `PUT /api/users/{id}/node-label` *(admin)* — body `{node_label}`, a `"key=value"` string (e.g. `"node-type=cpu"`) or `null` to clear it. Every Deployment that account launches afterward gets a matching `nodeSelector`, pinning its pods to nodes carrying that label; `null` (the default for a new account) leaves placement unrestricted. Only affects future launches — see "Per-user node placement" below.
- `GET /api/pods` — JSON snapshot of the current pods in the watched namespace, filtered by role: a `user` only gets pods whose `aether.io/owner` label matches their own username, an `admin` gets all of them (each with its `owner` field populated). Pods for templates with a `secret_env_key` also carry a `credential: {env_key, value}` looked up from `deployment_secrets`, and pods for proxy-enabled templates carry a `proxy_path: "/proxy/<name>/"`.
- `GET /ws` — WebSocket; sends a full snapshot on connect (same per-role filtering and credential enrichment as `GET /api/pods`), then `upsert`/`delete` events as pods change, filtered the same way per-connection
- `GET /api/images` — JSON list of catalog entries from the `images` table (id, name, image, description)
- `GET /api/templates` — JSON list of templates (any logged-in role — needed for the Launch tab's dropdown)
- `POST /api/templates` / `PUT /api/templates/{id}` *(admin)* — create/update a template. Body is a `TemplateEntry` minus `id`: `{name, image, container_port, cpu_request, cpu_limit, memory_request, memory_limit, accelerator_type, accelerator_count, env, args, notes, secret_env_key, proxy_enabled, strip_prefix, public_service}` — only `name`/`image` are required, everything else defaults to empty/`null`/`false`/`true`. `secret_env_key`, if set, is the env var name (e.g. `JUPYTER_TOKEN`) that Launch should auto-generate instead of showing as an editable field — a proxy-enabled template doesn't need one (RStudio has none). `strip_prefix` only matters when `proxy_enabled` is set (see "the reverse proxy" above). `public_service` is independent of `proxy_enabled` — set it to `false` either for a proxied app with no auth of its own (Aether's login becomes the only way in, e.g. RStudio), or for a plain internal-only service consumed from inside the cluster (e.g. an LLM engine other in-cluster tooling talks to directly, with no browser login to bypass and no proxy involved at all).
- `DELETE /api/templates/{id}` *(admin)* — delete a template
- `POST /api/deployments` — creates a `Deployment` in the watched namespace (labeled `aether.io/owner: <your username>`), and if `container_port` is set, also a Service exposing it — `LoadBalancer` (public, MetalLB-assigned external IP) if `public_service` is true (the default), `ClusterIP`-only otherwise. Body: `{name, image, replicas, cpu_request, cpu_limit, memory_request, memory_limit, accelerator_type, accelerator_count, container_port, env, args, generate_secret_for, enable_proxy, strip_prefix, public_service}` — everything except `name`/`image`/`replicas` is optional (`public_service` defaults to `true` if omitted); `env` is `[[key, value], ...]` pairs (entries with an empty value are dropped, so an image's own default behavior — e.g. an auto-generated password logged at startup — still applies unless you set one); `args` is a list of container command-line arguments (any occurrence of the literal string `{{name}}` is substituted with the deployment's own name first); `generate_secret_for`, if set to an env var name, generates a random value for it (overriding anything with that key in `env`) and stores it in `deployment_secrets`; `enable_proxy`, if `true`, requires `container_port` to be set (400 otherwise) and makes the app also reachable via `GET/POST/... /proxy/<name>/...`, with `strip_prefix` controlling how that route forwards paths (see "the reverse proxy" above); `public_service`, independent of `enable_proxy`, controls whether the Service is a public `LoadBalancer` or `ClusterIP`-only. Response adds `service_name`/`container_port` (both `null` if no port was given), `secret_value` (the generated value, or `null`), `proxy_path` (`"/proxy/<name>/"` if `enable_proxy` was set, else `null`), and `public_service` (echoes the request, so the frontend knows whether to mention an external IP).
- `GET /api/deployments/{name}` — current editable state of a Deployment you own (or, for an admin, any Deployment): `{name, replicas, cpu_request, cpu_limit, memory_request, memory_limit, env, generated_secret_key}`. `env` excludes the auto-generated secret's entry, if any — its key is reported separately as `generated_secret_key` rather than its (regeneratable) value, since it's shown read-only rather than as an editable row. 403 if you don't own it, 404 if it doesn't exist. Backs the Pods tab's manage panel.
- `PUT /api/deployments/{name}` — scales and/or updates resources/env on a Deployment you own (or, for an admin, any Deployment). Body: `{replicas, cpu_request, cpu_limit, memory_request, memory_limit, env}`. Image, container port, accelerator, and args are fixed at launch time — changing those is a delete + relaunch, not an edit. An existing auto-generated secret's env var is carried through untouched regardless of what's submitted in `env` — edits never regenerate or require resubmitting it, since a client may already be using that value. Same validation as create (quantities, env keys, non-negative replicas). Returns the same shape as `GET`.
- `DELETE /api/deployments/{name}` — deletes a Deployment you own (or, for an admin, any Deployment), its Service if it has one, and its `deployment_secrets` row (if any) — the one place in the app that actually cleans up a generated credential rather than leaving it to outlive the deployment that used it. 403 if you don't own it.
- `ANY /proxy/{deployment_name}`, `ANY /proxy/{deployment_name}/`, `ANY /proxy/{deployment_name}/{*rest}` — reverse-proxies into a proxy-enabled deployment's pod (`backend/src/proxy.rs`), injecting its generated credential (if any) as the appropriate auth header so there's no login prompt. The first two (bare path / trailing slash, no further segment) are what every "Open" link actually points at; the wildcard one handles everything else the app itself requests once loaded. 403 if you're not that deployment's owner (or an admin); 400 if the deployment isn't proxy-enabled; 502 if the connection to its pod fails or times out (5s). Handles WebSocket upgrades transparently (needed for JupyterLab's kernel connections). See "Ownership, auto-generated credentials, and the reverse proxy" below.
- `GET /api/pods/{name}/logs?container=&tail_lines=&previous=` — plain-text container logs (`container` defaults to the pod's only container if it has one; `tail_lines` defaults to 500; `previous=true` gets the last terminated instance's logs, for a crashed container)
- `GET /api/pods/{name}/events` — JSON list of Kubernetes Events involving that pod (`type_`, `reason`, `message`, `count`, `last_seen`), most recent first — note the apiserver's default Event TTL is short (commonly ~1h), so older pods often have none left
- `GET /api/quota/me` — the caller's own effective quota (their `user_quotas` override if they have one, else the global default), current usage, and `expose_resource_requests`. Always unlimited limits for an admin (exempt from enforcement), though `expose_resource_requests` still applies to everyone. Backs the Launch tab and the Pods tab's manage panel.
- `GET /api/quota/settings` / `PUT /api/quota/settings` *(admin for PUT; GET requires only login)* — the global default quota: `{cpu_limit, memory_limit, gpu_limit, expose_resource_requests, fixed_cpu_request, fixed_memory_request}`. The limit/request fields are quantity strings (e.g. `"4"`/`"16Gi"`) or a plain integer (`gpu_limit`) — `null`/omitted means unlimited for a limit, or "leave unset" for a fixed request. `fixed_cpu_request`/`fixed_memory_request` only take effect while `expose_resource_requests` is `false` (see "User quotas" below).
- `GET /api/quota/users` *(admin)* — every account's `{user_id, username, quota_override, used_cpu_millicores, used_memory_bytes, used_gpu_count}` — `quota_override` is `null` if that user has no override and is bound by the global default. Backs the Quotas admin tab's table.
- `PUT /api/quota/users/{id}` *(admin)* — sets (or replaces) a user's quota override, same `{cpu_limit, memory_limit, gpu_limit}` shape as the global settings' limits. `DELETE /api/quota/users/{id}` *(admin)* clears it, reverting that user to the global default.
- `GET /proxy-auth?deployment=&next=` — the app-origin half of the proxy handshake. Verifies the caller's session and that they may open `deployment`, then redirects to that deployment's own origin carrying a single-use token. 403 if you don't own it; redirects to the SPA if you aren't logged in (it's a link people follow, not an API call). Only meaningful when `PROXY_BASE_DOMAIN` is set.
- `ANY <name>.<PROXY_BASE_DOMAIN>/*` — everything on a per-deployment proxy origin is forwarded to that deployment's pod, including paths like `/api/...` that would otherwise be Aether's own. `GET /__aether/auth` on that origin is the one exception: it redeems the token above and sets the origin's own `aether_proxy` cookie.
- `GET /*` — serves the built frontend (`index.html`, JS, WASM, CSS)

## Image and template catalogs (Postgres)

The `images` table (schema in `backend/migrations/0001_create_images.sql`,
applied automatically on startup via `sqlx::migrate!`) backs the image
catalog used by "Custom" mode on the Launch tab, managed from the **Images**
admin tab — a full CRUD UI for it, same pattern as Templates below. Its
schema, for reference or scripting bulk data:

```sql
CREATE TABLE images (
    id SERIAL PRIMARY KEY,
    name TEXT NOT NULL,        -- display name, e.g. "Ollama (ROCm)"
    image TEXT NOT NULL,       -- image ref, e.g. "ollama/ollama:rocm"
    description TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

```sql
INSERT INTO images (name, image, description) VALUES
  ('Ollama (ROCm)', 'ollama/ollama:rocm', 'Ollama with AMD ROCm GPU support');
```

The `templates` table (schema + seed data in
`backend/migrations/0002_create_templates.sql`) backs both the **Template**
dropdown on the Launch tab and the **Templates** admin tab, which is a full
CRUD UI for it — no need to touch SQL directly unless you're restoring/
scripting data:

```sql
CREATE TABLE templates (
    id SERIAL PRIMARY KEY,
    name TEXT NOT NULL,
    image TEXT NOT NULL,
    container_port INTEGER,
    cpu_request TEXT NOT NULL DEFAULT '',
    cpu_limit TEXT NOT NULL DEFAULT '',
    memory_request TEXT NOT NULL DEFAULT '',
    memory_limit TEXT NOT NULL DEFAULT '',
    accelerator_type TEXT NOT NULL DEFAULT '',
    accelerator_count BIGINT,
    env JSONB NOT NULL DEFAULT '[]',    -- [["KEY", "default value"], ...]
    args TEXT[] NOT NULL DEFAULT '{}',  -- ["--model=...", ...]
    notes TEXT NOT NULL DEFAULT '',
    secret_env_key TEXT,                -- e.g. "JUPYTER_TOKEN"; NULL means no auto-generated secret
    proxy_enabled BOOLEAN NOT NULL DEFAULT false,   -- also reachable via Aether's /proxy/<name>/
    strip_prefix BOOLEAN NOT NULL DEFAULT false,    -- see "the reverse proxy" below
    public_service BOOLEAN NOT NULL DEFAULT true,   -- LoadBalancer (true) vs ClusterIP-only (false)
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

It ships seeded with the same five templates (Ollama, vLLM, SGLang,
JupyterLab, RStudio) that used to be hardcoded in the frontend — edit or
delete them from the Templates tab like any other row. JupyterLab
(`JUPYTER_TOKEN`) and vLLM (`VLLM_API_KEY`) are seeded with a
`secret_env_key`; Ollama, SGLang, and RStudio aren't (Ollama and SGLang have
no auth mechanism at all; RStudio runs with its own auth fully disabled —
see below). Both JupyterLab and RStudio are seeded `proxy_enabled`.

`public_service` is a separate, admin-only toggle available on every
template — including Ollama/vLLM/SGLang, which have no `proxy_enabled` at
all. Unchecking it in the Templates tab (or setting it on a per-launch
basis via the API) makes future launches of that template get a
`ClusterIP`-only Service instead of a public `LoadBalancer`: still fully
reachable from anywhere else inside the cluster (e.g. an in-cluster coding
tool calling an LLM engine's API directly), just not from outside it. This
is independent of the reverse proxy — it doesn't require (or imply) auth of
any kind, it's purely about network exposure. Ollama/vLLM/SGLang default to
`public_service = true` (external), matching their behavior before this
toggle existed; flip it per template as needed.

## Ownership, auto-generated credentials, and the reverse proxy

Every `Deployment`/pod created via Launch is labeled `aether.io/owner:
<username>` (a Kubernetes label, kept separate from the `app: <name>`
selector label so it can't interfere with Service routing). The Pods tab
and its underlying REST/WebSocket endpoints filter on this label: a `user`
account only ever sees pods it launched itself; an `admin` sees everything,
with an extra **Owner** column.

Templates with a `secret_env_key` (JupyterLab, vLLM) don't expose that field
as editable input on the Launch form at all — instead, the backend
generates a random 48-character alphanumeric value (the same generator used
for session tokens), injects it as that env var on the container, and
stores it in a `deployment_secrets` table keyed by the Deployment's name:

```sql
CREATE TABLE deployment_secrets (
    deployment_name TEXT PRIMARY KEY,
    namespace TEXT NOT NULL,
    env_key TEXT,           -- NULL if this deployment has no generated credential at all
    secret_value TEXT,      -- NULL alongside env_key
    owner_username TEXT NOT NULL,
    proxy_enabled BOOLEAN NOT NULL DEFAULT false,
    container_port INTEGER,
    strip_prefix BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

The value is shown once in the Launch success message and persistently in
the Pods tab's Credential column (for whoever can see that pod — the same
ownership filtering applies). Re-launching under the same Deployment name
replaces the stored value. A row exists here for *any* proxy-enabled
deployment, credential or not — see RStudio below for why.

**JupyterLab and RStudio genuinely work like JupyterHub**: both templates
are `proxy_enabled`, meaning they're also reachable via Aether's own
`GET/POST/... /proxy/<name>/{*rest}` route (`backend/src/proxy.rs`, plus two
bare-path variants registered alongside it in `main.rs` — every "Open" link
points at the bare `/proxy/<name>/` with no trailing segment, which
matchit's `{*rest}` wildcard doesn't match on its own). The handler:

1. Checks you own that deployment (or are an admin) — same rule as the Pods
   tab's visibility filtering.
2. Looks up that deployment's Service and connects to its in-cluster
   `ClusterIP` directly (`kube`'s `Api<Service>::get`, RBAC already covers
   `get` on `services`) — whether that Service is itself a public
   `LoadBalancer` or `ClusterIP`-only makes no difference here, both have a
   ClusterIP. This is the conventional in-cluster design, and it assumes
   Aether itself is running **in-cluster** — a `ClusterIP` isn't routable
   from outside the cluster network, so this specific hop can't be
   exercised with the backend running locally against a remote cluster,
   unlike everything else in this app (see "Status & known limitations").
   The connection attempt times out after 5 seconds either way, so a
   stuck/unready pod fails fast (502) rather than hanging the request.
3. Injects an `Authorization` header for the credential, if the deployment
   has one — `token <value>` for `JUPYTER_TOKEN` (Jupyter Server's
   documented convention). No header at all if there's no credential
   (RStudio — see below).
4. Forwards the request path one of two ways, per the template's
   `strip_prefix`: JupyterLab (`false`) wants the full
   `/proxy/<name>/...` path forwarded as-is, since `--ServerApp.base_url`
   registers its own routes under that prefix. RStudio (`true`) is the
   *opposite* — its `www-root-path` setting only stamps that prefix onto
   redirects and cookies sent back to the browser, and it still expects
   requests to arrive at the bare path, so the proxy strips the prefix
   first. (Confirmed by hand against a real `rocker/rstudio` container:
   hitting the prefixed path 404s, while hitting the bare path with
   `www-root-path` set produces a redirect whose `Location` header — and
   whose `Set-Cookie` `Path` attributes — correctly include the prefix,
   because the proxy also forwards the original `Host` header unchanged.)
5. Transparently tunnels WebSocket upgrades too (via `hyper::upgrade` +
   `tokio::io::copy_bidirectional`), which is what makes JupyterLab's kernel
   connections (running notebook cells) work through the proxy, not just
   static pages.

JupyterLab's seeded args are `["start-notebook.sh",
"--ServerApp.base_url=/proxy/{{name}}/"]`, where `{{name}}` is a generic
placeholder substituted with the deployment's own name at launch time (any
template's `args` can use it). Kubernetes' `args` field *replaces* a
container's default command rather than appending to it, which is why the
start script has to be named explicitly — leaving it out makes the
container try (and fail) to `exec` the flag itself as a program. Its
template also sets `public_service = false` — the token-header injection
above is the only auth the proxy adds, so, same as RStudio below, the pod
gets a `ClusterIP`-only Service rather than a public `LoadBalancer`;
otherwise anyone who obtained a raw pod IP could skip Aether's proxy (and
its ownership check) and reach Jupyter directly with no token at all.

**RStudio runs with its own authentication fully disabled** (`env:
[["DISABLE_AUTH", "true"]]`) and relies entirely on Aether's login plus the
ownership check above — there's no credential to generate or inject at all,
and no login prompt to skip, because RStudio simply never asks. This is
only safe because its template also sets `public_service = false`: Launch
creates a `ClusterIP`-only Service for it instead of a public
`LoadBalancer`, so the *only* way to reach it is through Aether's own login
followed by that ownership check — nothing on the network can hit it
directly. Its `args` are a small shell wrapper (`rocker/rstudio`'s
`ENTRYPOINT` is empty, so `args` alone becomes the whole command line):
```
/bin/bash -c 'echo "www-root-path=/proxy/{{name}}/" >> /etc/rstudio/disable_auth_rserver.conf && exec /init'
```
`disable_auth_rserver.conf` is the config file the image's own init script
copies over `rserver.conf` when `DISABLE_AUTH=true` — appending to it
first, then letting `/init` run normally, is what gets `www-root-path` set
without needing a mounted config file or overriding the image's own init
logic.

vLLM is intentionally never proxied: its `VLLM_API_KEY` is meant for
scripted API clients setting their own `Authorization: Bearer <key>`
header, not a browser session — it already matches real
bearer-token-via-header usage without needing a proxy in front of it, and
forcing it through Aether's cookie-based login would only get in the way of
automation.

Each proxied HTTP request currently opens a fresh TCP connection and
HTTP/1.1 handshake to the pod rather than reusing a pooled connection —
correct and simple, but adds latency per request; pooling is a reasonable
future optimization, not a correctness issue.

## Managing running deployments

The Pods tab's detail panel (click any pod row) shows a **Manage** section
for any pod that has a `deployment_name` — i.e. anything launched through
Aether (or carrying an `app` label some other way). It lets you scale
replicas, adjust CPU/memory requests and limits, edit environment
variables, and delete the Deployment (plus its Service, if any) entirely.
Image, container port, accelerator, and args are intentionally not
editable here — changing any of those is a delete + relaunch through the
Launch tab, not an in-place edit.

Authorization is enforced backend-side against the Deployment's own
`aether.io/owner` label (`backend/src/deployments.rs::check_owner`), not
trusted from what the frontend happens to show: an admin can manage any
Deployment, everyone else only their own. In practice a `user`-role account
never even sees a pod it doesn't own to begin with (`visibility.rs`
filters `GET /api/pods` per-role already), so the frontend doesn't
separately re-check ownership before rendering the Manage section — it
just renders whenever `deployment_name` is present, and the backend is the
actual gate for admins looking at someone else's pod.

If the Deployment has an auto-generated credential (`secret_env_key` on
its template), its env var is shown as a read-only note rather than an
editable row, and edits never regenerate or resend it — the backend always
carries the existing stored value through untouched, since resubmitting a
placeholder or omitting it entirely would otherwise silently invalidate a
value someone might already be using. Deleting a Deployment does finally
clean up its `deployment_secrets` row, though — this is the one place a
generated credential doesn't just outlive the workload it was made for
(see "Known limitations" below for the general case).

## User quotas

A single user could otherwise launch enough replicas/resources to occupy
the entire shared cluster. The **Quotas** admin tab sets a cluster-wide
default (CPU limit in cores, memory limit, GPU count — any left blank
means unlimited for that dimension) plus optional per-user overrides; an
override fully replaces the global default for that user across all three
dimensions, rather than overriding just one field at a time. Enforced in
`backend/src/quota.rs::check_quota`, called from both
`create_deployment` and `update_deployment` before either ever touches the
cluster — a launch or edit that would push the *owning user's* total over
their effective quota gets rejected with 400 and a message naming the
exceeded dimension and the numbers involved. **Admins are exempt** —
quotas exist to stop a `user` account from monopolizing shared capacity;
an admin already has unrestricted cluster access via their own kubeconfig
regardless of what Aether enforces.

Quota is checked against resource **limits**, not requests — interactive
workloads are bursty, so it's peak usage that risks starving other users,
not steady-state reservation. Usage itself is summed live from the pod
watcher's own in-memory cache (`PodInfo::cpu_limit_millicores`/
`memory_limit_bytes`/`accelerators`, already computed for the Pods tab) —
no separate accounting table, and no new Kubernetes RBAC verbs were needed
to compute it. GPU quota is a single aggregate count regardless of
accelerator vendor/type (`nvidia.com/gpu`, `amd.com/gpu`, ... all count
toward the same limit) — simplest, and this cluster currently only has
AMD GPUs anyway.

A separate global toggle, **`expose_resource_requests`**, controls whether
the Launch tab and the Pods tab's manage panel show CPU/memory *request*
fields at all, independent of the quota limits themselves. This is
deliberately just a display/input setting, not a quota dimension of its
own. With it off, those fields disappear and the backend substitutes an
admin-configured **fixed request** (`fixed_cpu_request`/
`fixed_memory_request`, also set from the Quotas tab, shown only while
`expose_resource_requests` is off) for every launch and edit instead —
regardless of what limit is set. Left blank, a dimension's request is
simply never set at all, and Kubernetes' own default behavior takes over
(matching a container's request to its limit when a limit is given with
no request — Guaranteed QoS, reserving the full limit). A configured fixed
value avoids that default, letting requests stay low and predictable
(Burstable QoS) even when limits are generous. The Launch tab and manage
panel both show a note naming whatever fixed values are configured, so
users aren't left guessing why the request fields disappeared or what
they're actually getting.

Scaling or editing an existing deployment excludes *that deployment's own*
current usage from the baseline before adding its proposed new footprint,
so raising its own replica count or limits is judged only against what it
would become, not double-counted against what it already is.

## Per-deployment proxy origins

A proxied app runs code Aether doesn't control — JupyterLab and RStudio run
arbitrary user code by design, and `enable_proxy` can be set on any image.
Serving those apps from a path on Aether's own origin (`/proxy/<name>/`, the
original design) means their JavaScript is *same-origin* with the SPA and
`/api/*`, so it can call Aether's API with the browsing user's session cookie
attached automatically. `HttpOnly` is no defence (the JS never reads the
cookie — the browser just sends it) and neither is `SameSite=Lax` (same site).
Because an admin can open anyone's proxied app, that let a `user` account
escalate to admin simply by getting theirs opened. This is the same reason
JupyterHub ships per-user subdomains.

Setting `PROXY_BASE_DOMAIN` (plus `APP_ORIGIN`) gives every deployment its own
origin — `<name>.proxy.aether.example` — so the browser treats it as a
different site entirely. Requests to a proxy origin are dispatched by `Host`
in a middleware sitting *outside* the app's router
(`proxy::dispatch_by_host`), so they never reach `/api/*` or the SPA at all;
everything on that origin, including a path like `/api/pods`, is forwarded to
the pod. The old `/proxy/<name>/` path stops serving content and just
redirects to the new origin, so the hole closes rather than lingering beside
the fix. Host matching accepts exactly one label in front of the base domain,
matching what a wildcard TLS cert covers.

Because Aether's session cookie is host-only, it is never sent to a proxy
origin — which is the point, but means that origin needs its own way to know
who you are. Hence a small handshake, mirroring OAuth's shape:

1. A request to `<name>.proxy…` with no proxy session redirects to
   `/proxy-auth` on the **app** origin — the only host that receives the
   session cookie.
2. There, Aether verifies the session and that the caller may open this
   deployment (owner, or an admin), then mints a single-use token with a 30
   second lifetime and redirects back to the deployment's origin.
3. That origin redeems the token (deleted as it's read, so a copy left in
   history or a `Referer` is already spent), checks it was minted for *this*
   deployment, and sets its own `aether_proxy` cookie — host-only, so it
   belongs to that one subdomain and nothing else under the base domain.
4. Later requests carry that cookie. The user is re-resolved from it on every
   request rather than trusted from the cookie alone, so deleting an account
   (or replacing a deployment with a same-named one owned by someone else)
   takes effect immediately.

The `aether_proxy` cookie authorizes exactly one deployment and nothing else
in Aether, so a pod capturing its own is no more powerful than it already was.
Both it and `aether_session` are stripped from anything forwarded upstream.

Note this also makes the **admin bypass safe again**: an admin can open a
user's app for support, because that app is now cross-origin from `/api`.

**Deploying it** needs a wildcard DNS record for `*.<PROXY_BASE_DOMAIN>` and a
TLS cert covering both that wildcard and the app's own hostname, pointed at
whatever fronts Aether. `APP_ORIGIN`'s scheme decides whether the
`aether_proxy` cookie is marked `Secure`, so serve both over HTTPS.

## Per-user node placement

An admin can pin a user's workloads to a specific subset of nodes by
setting a **node label** on their account from the Users tab —
`"key=value"` (e.g. `node-type=cpu`, or `accelerator=amd` to keep someone
on the AMD GPU node), matching an actual label already on some subset of
the cluster's nodes. Every Deployment that account launches afterward gets
a `nodeSelector` with that single key/value pair
(`backend/src/deployments.rs::node_selector_for`, set on the pod template
in `create_deployment`); the Kubernetes scheduler then refuses to place
its pods anywhere else. Clearing the label (set it to `null`/empty)
returns the account to unrestricted placement for future launches.

Like the image/accelerator/args fields, the selector is fixed at launch
time from whatever the label was *then* — it isn't retroactively applied
to already-running deployments if an admin changes it later, and
`update_deployment` never touches it (only `replicas`/`resources`/`env`
are editable post-launch). This is validated server-side
(`validate::node_label`) as a practical subset of the real Kubernetes
label-key/value grammar, admin-only (`PUT /api/users/{id}/node-label`),
and otherwise invisible to the affected user — no UI surfaces it to them,
since it's a placement decision, not something they need to act on.

## Activity logging

Two append-only tables back the Activity tab, kept for support/metrics
purposes ("when did this user last log in, from where" and "who launched
JupyterLab with what resources") — separate from `sessions` and
`deployment_secrets`, which get deleted/overwritten and are used only for
live auth/proxy checks, not history:

```sql
CREATE TABLE session_log (
    id SERIAL PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    ip_address TEXT,
    user_agent TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE launch_log (
    id SERIAL PRIMARY KEY,
    deployment_name TEXT NOT NULL,
    namespace TEXT NOT NULL,
    owner_username TEXT NOT NULL,
    template_name TEXT,        -- NULL for a Custom launch
    image TEXT NOT NULL,
    replicas INTEGER NOT NULL,
    cpu_request TEXT, cpu_limit TEXT, memory_request TEXT, memory_limit TEXT,
    accelerator_type TEXT, accelerator_count BIGINT,
    container_port INTEGER,
    env JSONB NOT NULL DEFAULT '[]',   -- [[key, value], ...] — see redaction note below
    args TEXT[] NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

- `POST /api/login` records the login's source IP (`ConnectInfo`, real since
  Aether's own frontend sits directly behind its LoadBalancer with no proxy
  in front of itself — unrelated to the `/proxy/` routes above, which proxy
  *to* other apps) and `User-Agent` header into `session_log`.
- `POST /api/deployments` records the full launch request into `launch_log`
  after a successful create — **except** any env value matching
  `generate_secret_for`, which is replaced with the literal string
  `"<generated>"` before it's ever written. This matters because, unlike
  `deployment_secrets` (admin-adjacent, tied to a specific still-running
  deployment), `launch_log` is visible to the launching user themselves
  indefinitely — it must never carry a real credential.
- `GET /api/sessions` / `GET /api/launches` — same visibility rule as Pods:
  an `admin` gets every row (with a `username` field), a `user` account gets
  a query that's already server-side filtered to just their own, not a
  client-side-hidden subset of everyone's.

## Building the container image

Cross-compiling for `linux/amd64` from an arm64 machine (e.g. Apple Silicon)
via QEMU reliably crashes `rustc`, so build this on a native amd64 box — CI
handles this (see below). If you're already on amd64:

```
docker build -t <registry>/aether/aether:latest .
docker push <registry>/aether/aether:latest
```

The image is a distroless (`gcr.io/distroless/cc-debian12:nonroot`) runtime
containing just the compiled backend binary and the built frontend assets —
no shell, runs as a non-root user.

## CI (Forgejo Actions)

`.forgejo/workflows/build.yml` builds and pushes the image to
`ctr.int.example.com:8443/aether/aether` (the `aether` project in
Harbor, repository also named `aether` — deliberately not `aether-web` or
`aether-app`, since there's only one image this whole project produces) on
every push to `main`, on version tags (`v*`), or via manual dispatch.

Every build gets tagged with three things: the short git SHA (immutable,
never overwritten — this is what actually gets deployed, see below),
`v<run number>` (Forgejo's own ever-increasing per-workflow counter, e.g.
`v42` — a free, zero-effort human-readable version bumped on every single
run, including a `workflow_dispatch` re-run of the same commit), and
`latest` (which does move, by definition — nothing pulls it for anything
that matters, since the deploy path always pins to the SHA tag). Pushing a
`vX.Y.Z` git tag additionally tags that specific build with the exact
version string (e.g. `v1.4.0`), for deliberate, human-chosen releases —
alongside the other three tags, not instead of them.

After a successful push, the same job also bumps the deploy: it clones the
separate **Aether-Deploy** repo (see "GitOps deploy" below — that's where
this app's Kubernetes manifests actually live, not here), downloads
`kustomize`, runs `kustomize edit set image ...` there to point its
`kustomization.yaml` at the new SHA-tagged image, and — if that actually
changed anything — commits and pushes that one file straight to
Aether-Deploy's `main`. Argo CD is what actually notices that commit and
rolls the cluster forward. This job never touches the cluster itself, or
even this repo's own `main` — only the registry and Aether-Deploy's git
history. Deploys always pin to the SHA tag specifically (not `v<run
number>` or a release tag) — it's the one tag guaranteed to exist for
every single build with no extra logic needed to pick the "right" one.

Requires three repository secrets in Forgejo (Settings → Actions → Secrets):

- `REGISTRY_USER` / `REGISTRY_PASSWORD` — container registry push access.
- `AETHER_DEPLOY_TOKEN` — an access token with write access to
  **Aether-Deploy's** contents (not this repo), used only to push the
  image-tag-bump commit above.

The runner must be registered with a `docker` label (matching `runs-on:
docker` in the workflow) — registered from the *repo's* (or org's) Actions →
Runners settings page specifically, not some other scope, or it won't be
visible to this workflow at all despite showing "Idle" in its own runner
list (Forgejo scopes runners by wherever their registration token came
from: instance-wide, org, user, or single-repo).

This workflow's `docker` label runs in **host mode**
(`runner.labels: ["docker:host"]` in the runner's own `config.yaml` — no
per-job container at all), a deliberate choice since this workflow only
ever builds/pushes Docker images and doesn't need per-job isolation; it
also sidesteps the classic Alpine-job-image problem (Forgejo's runner
injects a glibc-linked Node build to execute JS-based actions like
`actions/checkout@v4`, which won't run on musl-based Alpine). In host mode,
the runner machine itself needs, directly on its `PATH`:

- **Docker**, obviously, plus TLS trust for the Harbor registry — Docker's
  daemon has its own registry-TLS trust store, completely separate from the
  system CA bundle, so a cert trusted system-wide (`update-ca-trust`/`trust
  anchor`) still won't let `docker login`/`push` succeed. Drop the CA cert
  at `/etc/docker/certs.d/ctr.int.example.com:8443/ca.crt` (directory
  name must exactly match the registry host:port) and restart the Docker
  daemon specifically — not just the runner.
- **git** (for `actions/checkout`) and **Node.js 20+** (`actions/checkout@v4`
  is itself a Node.js action, regardless of host vs. container execution).

## Deploying to Kubernetes

Kubernetes manifests, the Argo CD `Application`, and full deploy
instructions live in the separate
[**Aether-Deploy**](https://git.example.com/Aether/Aether-Deploy)
repo — not here (see "GitOps deploy" below for why). One thing still needs
creating by hand directly in the cluster's `aether` namespace regardless,
since it's a credential rather than a manifest and shouldn't live in either
git repo:

- **Image pull secret** — the registry requires auth:

  ```
  kubectl create secret docker-registry regcred \
    --docker-server=ctr.int.example.com:8443 \
    --docker-username=<user> --docker-password=<password> \
    -n aether
  ```

Postgres doesn't need a manual secret — it runs in-cluster via
CloudNativePG (`postgres-cluster.yaml` in Aether-Deploy), which
auto-generates its own connection-string secret. See Aether-Deploy's
README, "Postgres (CloudNativePG)", for the one-time operator/StorageClass
bootstrap that needs.

Everything else — the `ServiceAccount`/`Role`/`RoleBinding`, `Deployment`,
`Service`, Postgres, and how to point at a different namespace — is
documented in Aether-Deploy's own README.

## GitOps deploy (Argo CD)

Once code merges to `main` and CI pushes a new image, something still has to
actually roll it out to the cluster. That's Argo CD's job, not CI's — CI
never holds cluster credentials at all, only registry access and git access
to a *deploy* repo (see "CI" above). This was a deliberate choice over
having CI run `kubectl apply` directly: it keeps the only thing with
cluster-write access running *inside* the cluster itself, watching for
changes, rather than handing that power to a build runner.

**This app's manifests live in a separate repo, Aether-Deploy, not here.**
That split (rather than a `k8s/` directory in this repo, which is how this
was originally built) is deliberate: this repo's history stays pure
app-code commits, the bot's tag-bump commits get their own repo instead of
polluting this one, and the CI token that pushes those commits only ever
needs write access to Aether-Deploy — not to this repo's actual source
code. If you're rebuilding this setup elsewhere, "one repo for app code,
one for rendered manifests + the Argo CD `Application`" is the pattern to
follow from the start rather than splitting later.

**One-time bootstrap** (already done for this cluster; recorded here for
rebuilding it elsewhere):

1. Install Argo CD into its own `argocd` namespace using the standard
   (non-HA) install manifests, then expose its UI the same way everything
   else in this cluster is exposed — a `LoadBalancer` Service, since there's
   no ingress controller:
   ```
   kubectl create namespace argocd
   kubectl apply -n argocd --server-side --force-conflicts \
     -f https://raw.githubusercontent.com/argoproj/argo-cd/stable/manifests/install.yaml
   kubectl patch svc argocd-server -n argocd -p '{"spec": {"type": "LoadBalancer"}}'
   ```
   (`--server-side` matters here — plain `kubectl apply` fails on the
   `applicationsets.argoproj.io` CRD with "annotations: Too long", since
   client-side apply stuffs the whole manifest into a
   `last-applied-configuration` annotation and this one's big enough to
   exceed the 256KiB etcd field limit.) The initial admin password is
   auto-generated: `kubectl -n argocd get secret
   argocd-initial-admin-secret -o jsonpath='{.data.password}' | base64 -d`
   — log in and change it.
2. Trust this Forgejo host's SSH key (Argo CD ships known-hosts entries for
   GitHub/GitLab/Bitbucket, not arbitrary self-hosted instances):
   ```
   ssh-keyscan -p 2022 git.example.com
   ```
   merged into the `argocd-ssh-known-hosts-cm` ConfigMap's
   `ssh_known_hosts` key (then `kubectl rollout restart
   deployment/argocd-repo-server -n argocd` to pick it up). Generate a
   dedicated keypair for Aether-Deploy specifically (`ssh-keygen -t
   ed25519`) — don't reuse a personal key, and don't reuse the same key
   across repos if this pattern grows to more apps, since scoping each
   credential to exactly the repo it needs was the whole point of this
   split. Add the *public* half as a read-only Deploy Key on the
   **Aether-Deploy** repo, and store the private half as a repository
   credential Argo CD reads directly:
   ```
   kubectl create secret generic aether-deploy-repo-creds -n argocd \
     --from-literal=type=git \
     --from-literal=url=ssh://git@git.example.com:2022/Aether/Aether-Deploy.git \
     --from-file=sshPrivateKey=<path to private key>
   kubectl label secret aether-deploy-repo-creds -n argocd argocd.argoproj.io/secret-type=repository
   ```
   Never commit the private key — it only ever exists as this in-cluster
   Secret and a local temp file deleted right after.
3. Apply the `Application` (lives in Aether-Deploy's repo root as
   `application.yaml`, not in this repo at all):
   ```
   kubectl apply -f application.yaml
   ```
   It targets Aether-Deploy's repo root (`path: .`), `targetRevision:
   main`, destination namespace `aether`, with `syncPolicy.automated:
   {prune: true, selfHeal: true}` and `CreateNamespace=true` — fully
   automatic from here on, no manual sync button needed.

**Steady-state loop** (this is the actual "build → deploy" pipeline): push
to this repo's `main` → CI builds and pushes
`ctr.int.example.com:8443/aether/aether:<sha>` → CI clones Aether-Deploy,
bumps its `kustomization.yaml`'s `images:` override to the new tag, and
pushes that commit to Aether-Deploy's `main` → Argo CD's `Application`
controller notices the new commit there (polls every few minutes by
default, or near-instantly with a Forgejo webhook configured later) and
re-syncs, which rolls the `aether` Deployment to the new image. Check
`kubectl get application aether -n argocd` for `Synced`/`Healthy` status, or
the Argo CD UI.

**Known limitation, same tradeoff every Argo CD install faces**: the
standard install grants `argocd-application-controller` a cluster-scoped
`ClusterRole` (it's built to manage any namespace), even though this
particular `Application` only ever targets `aether`. Fine for now; a
namespace-restricted install is a reasonable hardening step if Argo CD
starts managing more than this one app.

## Security notes

**Authentication:**

- Passwords are hashed with argon2 (via the `argon2`/`password-hash` crates),
  never stored or logged in plaintext.
- Sessions are opaque random tokens (48 alphanumeric chars from the OS RNG)
  stored server-side in the `sessions` table, sent to the browser as an
  `HttpOnly`, `SameSite=Lax` cookie (`aether_session`) so client-side JS
  (including any XSS) can't read it, and cross-site requests can't ride on
  it. Sessions last 7 days and aren't refreshed on activity; logging out
  deletes the row server-side, not just the cookie.
- The cookie is **not** marked `Secure` — this cluster has no TLS anywhere
  (see "Status & known limitations"), and a `Secure` cookie would simply
  never be sent over the plain-HTTP LoadBalancer this app is reached
  through. In this network, the session token (and the login password on
  the wire) are only as protected as the network itself.
- There's no rate limiting on `POST /api/login` — nothing stops password
  guessing beyond whatever's normal for your account's password strength
  (enforced at creation: ≥ 8 characters, nothing more).
- The app-level `admin`/`user` roles gate *application* actions (Templates,
  Users, Launch) and are unrelated to Kubernetes RBAC below, which gates
  what the backend's own ServiceAccount can do to the cluster regardless of
  which human is logged in.
- Every login records its source IP and `User-Agent` into `session_log`
  (see "Activity logging" above), visible to the account it belongs to and
  to admins — a privacy/data-retention tradeoff worth knowing about if this
  is ever used somewhere IP logging needs disclosure.

**Input validation** (`backend/src/validate.rs`), applied to Launch, Templates,
and Users requests server-side (the real boundary) and mirrored as HTML5
attributes client-side for early feedback:

- Deployment/Service names must be valid Kubernetes DNS-1123 labels
  (lowercase alphanumeric + `-`, 1-63 chars) — also closes off any
  path-injection risk from a crafted pod name reaching the Kubernetes API
  through `/api/pods/{name}/logs` or `/events`.
- Container ports must be 1-65535; CPU/memory quantities get a light format
  check (still not full k8s `Quantity` grammar — malformed-but-plausible
  values are caught by the Kubernetes API server itself); env var keys must
  look like real identifiers; env values, args, template names, and image
  refs are all length-capped.
- None of this defends against a *logged-in* user launching something
  legitimately dangerous (arbitrary image, arbitrary args) — that's a
  trust decision inherent to what this app does, not something input
  validation can fix. It only rejects malformed/oversized/injection-shaped
  input.

**Kubernetes RBAC** is minimal and namespaced (no `ClusterRole`): read-only
(`get`/`list`/`watch`) on `pods`, `get` on `pods/log`, `get`/`list` on
`events`, plus `create`/`get` on `apps/deployments` and on `services` —
nothing else. The app can create Deployments and Services but can't delete,
patch, list, or watch existing ones, and has no access to Secrets,
ConfigMaps, RBAC objects, etc. The reverse proxy doesn't need any RBAC
beyond `get` on `services` (already listed above) — it never talks to the
Kubernetes API to reach the app itself, just to look up a Service's
ClusterIP, then connects to that IP directly over plain TCP like any other
in-cluster client would.

**Other:**

- Pod logs can contain sensitive application output; any logged-in user
  (either role) can read the logs of anything running in the watched
  namespace, and (via Launch) can create a Service with a public-facing
  LoadBalancer IP — there's no admission control over what gets exposed.
- Templates (Ollama/vLLM/SGLang/JupyterLab/RStudio) are unauthenticated *at
  the app they launch* by default, unrelated to logging into Aether itself.
  Ollama and SGLang have no auto-generated credential (see "Ownership, auto-generated
  credentials, and the reverse proxy" above) — set your own token/password via the
  env var editor if the image supports one, otherwise it's either unauthenticated
  or gets a random value visible only in the pod's own logs. RStudio runs
  with its own auth *deliberately* disabled and no public Service at all —
  Aether's login is the only gate. JupyterLab and vLLM get an auto-generated
  credential instead, stored **in plaintext** in the `deployment_secrets`
  table (no encryption at rest) and visible to the owning user and any admin
  via the Pods tab.
- Pod ownership (`aether.io/owner` label) and the Pods-tab visibility
  filtering it drives are enforced entirely in the Aether backend at read
  time, not via Kubernetes RBAC or admission control — the label itself is
  just metadata anyone with direct `kubectl` access to the namespace can see
  or edit. It restricts what Aether's UI/API surface shows a `user` account,
  not what's actually running in the cluster.
- **The reverse proxy strips Aether's own credentials before forwarding.** A
  proxied pod runs code Aether doesn't control — JupyterLab and RStudio run
  arbitrary user code by design, and `enable_proxy` can be set on any image —
  so `backend/src/proxy.rs::forwarded_headers` removes the caller's
  `aether_session` cookie and their `Authorization` header on the way in, and
  drops any upstream `Set-Cookie` that would overwrite `aether_session` on the
  way back out (which would otherwise let a hostile pod pin the caller's
  browser to a session of its choosing). Every *other* cookie is forwarded
  untouched, because proxied apps set and depend on their own (RStudio's
  session, JupyterLab's XSRF token). Unit-tested in that module.
- **Each proxied deployment is served from its own origin** when
  `PROXY_BASE_DOMAIN` is set — see "Per-deployment proxy origins" below. This
  is what stops a proxied app's JavaScript from calling Aether's own API as
  whoever is browsing it. **With it unset, that hole is open**: `/proxy/<name>/`
  then shares an origin with the SPA and `/api/*`, so a pod's JS can call the
  API with the browsing user's cookie attached automatically (`HttpOnly`
  doesn't help — the JS never reads the cookie, the browser just sends it; nor
  does `SameSite=Lax` — it's the same site), and since an admin can open
  anyone's proxied app, a user could escalate to admin by getting one opened.
  Configure it for any shared deployment; leaving it unset is a local-dev
  convenience only, and the backend logs a warning at startup when it is.
- The container runs as a non-root user with a read-only root filesystem and
  all Linux capabilities dropped.
- `CorsLayer::permissive()` is enabled on the backend to make local `trunk
  serve` dev proxying easy. It sets `Access-Control-Allow-Origin: *` without
  `Access-Control-Allow-Credentials`, which per the Fetch spec means
  browsers won't expose credentialed cross-origin responses to another
  site's JS — CSRF protection here actually comes from the cookie's
  `SameSite=Lax`, not from this CORS policy.

## Status & known limitations

Everything above is implemented and has been exercised against the real
cluster (not just locally built) — including the failure-diagnosis path
against a pod that had genuinely been `Failed` for two weeks, the Launch
tab's port/env/args/Service wiring verified with real throwaway Deployments
(one confirmed via its own container logs: args were passed through to the
container and it ran and printed them), full template CRUD (create, edit,
delete, and launching from a DB-backed template) exercised against a
throwaway Postgres, and the full auth flow: bootstrap admin creation,
login/logout, a real `user`-role account confirmed able to reach Pods/Launch
but getting 403s from the Templates/Users write endpoints, and each
validation rule in `backend/src/validate.rs` confirmed to actually reject
its bad input (bad k8s name, out-of-range port, malformed quantity, bad env
key, path-injection-shaped pod name, weak password) via curl. The reverse
proxy was verified against a real `jupyter/base-notebook` pod launched by a
`user`-role account: `/proxy/<name>/` (the bare path every "Open" link
actually uses) served JupyterLab with the token already applied (no login
prompt), a second non-owning user got 403 on that exact path while an admin
could still open it, and — the part most likely to silently break — a real
notebook cell was executed through the proxied WebSocket kernel connection
and returned the correct output, confirming the upgrade-tunneling code path
actually works and isn't just serving static pages. (That "bare path"
qualifier matters: an earlier verification pass only ever tested
`/proxy/<name>/lab`, which masked a real bug where the bare path — with no
segment after the trailing slash — didn't match the route at all and fell
through to the frontend's own SPA fallback; fixed by adding two explicit
routes for it, see `backend/src/proxy.rs::handler_root`.) A parallel
`rocker/rstudio` pod, launched with `DISABLE_AUTH=true` and `public_service:
false`, was confirmed to get a `ClusterIP`-only Service (no external IP at
all) and to correctly reject a non-owner on that same bare path. The
Activity tab's login/launch history was verified the same way: two real
accounts confirmed each only sees their own rows while an admin sees both,
and a launched deployment's `generate_secret_for` value confirmed redacted
to `"<generated>"` in `launch_log` rather than storing the real credential.
The Images admin tab was verified the same way as Templates: full CRUD via
curl (including a `user`-role account confirmed to read the list but get
403 on create), plus a Puppeteer pass creating an entry through the actual
form. The theme toggle was confirmed via Puppeteer: default is dark,
toggling flips `data-theme` and the rendered background color to the
validated light-mode step, the choice persists in `localStorage` across a
reload, and both states were screenshotted to check for layout/contrast
issues. Deployment lifecycle management (scale/edit/delete from the Pods
tab, see "Managing running deployments" above) was verified against the
real cluster: ownership enforcement in every direction (owner, a
non-owning `user`, and admin, tested against `GET`/`PUT`/`DELETE` all
three), a scale+resource+env edit confirmed via `kubectl` to have actually
changed the live Deployment, an auto-generated secret's env var confirmed
byte-for-byte unchanged after an edit that didn't mention it, delete
confirmed to remove the Deployment, its Service, and its
`deployment_secrets` row, and a full Puppeteer pass as a `user`-role
account editing and then deleting a real deployment through the actual
Pods tab UI (including handling the native `confirm()` dialog). User
quotas were verified end-to-end against the real cluster: CPU, memory, and
GPU rejections all confirmed with the exact math checked (e.g. a launch
correctly rejected once existing usage plus its own footprint exceeded the
limit, with the error message's numbers matching by hand), a per-user
override confirmed to both raise a limit and, once cleared, correctly
revert that user to the global default, admin exemption confirmed by
launching wildly over-limit resources as admin with no rejection, the
scale/edit path's exclude-self accounting confirmed correct via exact
arithmetic on a real scale-up attempt, and a full Puppeteer pass covering
the Launch tab's quota summary display, the request-fields toggle actually
hiding/showing the right inputs after being flipped from the Quotas admin
tab, and the per-user usage table rendering real numbers. Fixed requests
were verified via `kubectl`: a deployment launched with a 1-core limit and
no request fields sent came back with its actual request pinned to the
configured fixed value (not defaulted up to match the limit), an edit
raising that same deployment's limit left its request untouched at the
fixed value, and re-enabling `expose_resource_requests` and launching with
an explicit request confirmed that value was honored normally again (no
fixed-request interference once the toggle is back on). Per-user node
placement was verified against the real cluster: setting a test account's
node label to `node-type=cpu` (a label that actually exists on this
cluster's worker nodes) and launching as that account produced a
Deployment whose `spec.template.spec.nodeSelector` was exactly
`{"node-type":"cpu"}` via `kubectl`, with its pod scheduled onto a matching
node; clearing the label and relaunching produced no `nodeSelector` at all;
malformed label values (no `=`, illegal characters) were rejected with 400;
a non-admin account got 403 attempting to set it; and a Puppeteer pass
confirmed the Users tab's new "Node label" column and edit form render and
save correctly. Known gaps, in case they matter for what you do next:

- **Still no "forgot password" self-service flow** — that requires emailing
  a reset link, which this app has no mechanism for (no SMTP config, no
  email field on accounts). An admin can reset a locked-out user's password
  from the Users tab instead (see below), which covers the "I forgot it"
  case even without a self-service link.
- **No login rate limiting.** `POST /api/login` has no lockout/backoff, so
  nothing but password strength (≥ 8 chars, enforced at creation) stands
  between an attacker and password guessing.
- **Quotas aren't retroactive.** Lowering a user's limit (or the global
  default) below what they're already running doesn't touch existing
  deployments — enforcement only ever blocks a *new* launch or edit from
  pushing usage over the limit, never reaches back to shrink or kill
  something already running.
- **Quota usage only counts pods Aether can attribute to an owner.** A
  Deployment created some other way (raw `kubectl apply`, no
  `aether.io/owner` label) doesn't count against anyone's usage and can't
  be blocked by this mechanism at all — quotas only govern what's launched
  through Aether itself.
- **This is a scoped-down slice of `SPEC.md`, not the whole thing.** No
  ingress controller and no StorageClass exist in this cluster yet, so
  there's no Gateway layer and no persistent storage — launched apps
  (including the LLM engines) lose all state on pod restart. `SPEC.md`'s
  multi-tenancy, HPA, and ArgoCD/GitOps roadmap items are entirely
  unaddressed; this only covers "deploy a single-namespace workload from a
  template."
- **vLLM/SGLang templates haven't been launched for real** — verified via
  a lightweight substitute (nginx/busybox) exercising the same code path
  (port/env/args/Service), not by actually pulling and running the
  multi-GB vLLM/SGLang images, which would have been slow in this
  environment. Expect to iterate on their default resource sizing once you
  actually run one.
- **Not yet fully deployed for real, as of this writing** — Argo CD has
  successfully synced Aether-Deploy (namespace, `Deployment`, `Service` with
  a real external IP all exist), but the pod itself is stuck in
  `ImagePullBackOff`: it needs the `regcred` secret created (see "Deploying
  to Kubernetes" above) and a Postgres connection, neither of which exist
  yet. The Forgejo Actions runner setup (see "CI" above) is now believed
  complete — scope, job-container/host-mode, Node, and registry-TLS issues
  all resolved — but a full green build → tag-bump → Argo CD sync run
  hasn't actually been observed end-to-end yet.
- **In-cluster Postgres (CloudNativePG) is wired up but not live yet** —
  `postgres-cluster.yaml` in Aether-Deploy needs the CloudNativePG operator
  installed and a real StorageClass named in its `storageClassName` field
  (currently `REPLACE_ME`); both are in progress. See Aether-Deploy's
  README, "Postgres (CloudNativePG)".
- **No confirmation on template or image edits**, only on delete — saving over an
  existing template's fields is immediate.
- **Single namespace only**, fixed at deploy time via the pod's own
  namespace. No in-app namespace switcher; watching multiple namespaces
  means deploying multiple copies (see "Watching a different namespace").
- **Deployment management covers scale/edit/delete, not everything** — see
  "Managing running deployments" above. Still no way to hand-edit a
  Service, no pod-level delete/restart independent of its Deployment
  (scale to 0 and back up instead), and no rollout-history/rollback view —
  a bad edit just needs manually editing it back, same as `kubectl`.
- **`VLLM_API_KEY` is an educated guess, not a confirmed env var name** — it
  hasn't been verified against a real vLLM server run (see the vLLM
  templates gap above). If vLLM ignores it, the generated value shown in the
  UI simply won't do anything.
- **Auto-generated credentials are plaintext in Postgres**, not a Kubernetes
  `Secret` or any encrypted store, and anyone with `deployment_secrets`
  table access can read every credential ever generated, past or present.
  Explicitly deleting a Deployment through the Pods tab's manage panel does
  clean up its row, but a credential still outlives its Deployment if that
  Deployment is instead removed some other way (directly via `kubectl`,
  e.g.) — there's no reconciliation loop that notices and cleans up after
  the fact.
- **JupyterLab and RStudio get true JupyterHub-style transparent auth**
  (reverse proxy + injected credential or, for RStudio, no auth at all —
  click "Open" and you're in). vLLM still just displays a credential for
  copy/paste, deliberately — it's meant for scripted API clients where a
  proxied cookie-auth flow would be more friction, not less. See "Ownership,
  auto-generated credentials, and the reverse proxy" above.
- **RStudio's no-auth mode was verified as thoroughly as possible without
  deploying Aether in-cluster.** Confirmed by hand against a real
  `rocker/rstudio` container: `DISABLE_AUTH=true` + the `www-root-path`
  config line produce the expected redirect/cookie behavior when the
  correct `Host` header is forwarded (which the proxy does). What's *not*
  confirmed is a real browser session completing that flow through Aether's
  actual `/proxy/` route end to end — same ClusterIP-reachability limitation
  as JupyterLab's proxy path (see below), compounded by RStudio's own
  user-agent sniffing making command-line verification less conclusive than
  Puppeteer-based verification was for JupyterLab. Worth a real click-through
  once Aether is deployed in-cluster, before relying on it for anything
  sensitive.
- **The reverse proxy opens a fresh TCP connection per HTTP request**, not a
  pooled/reused one — correctness over performance for this first pass. Fine
  for interactive single-user use; would need pooling before it'd hold up
  under heavier concurrent load.
- **The reverse proxy assumes Aether itself runs in-cluster** — it connects
  to a proxy-enabled deployment's Service via its `ClusterIP`, which isn't
  routable from outside the cluster network. This is the one code path in
  this app that can't be exercised with the backend running locally against
  a remote cluster (this project's usual local-dev pattern); testing it for
  real requires actually deploying Aether via Aether-Deploy/Argo CD (see
  "Not yet deployed for real" below) — it was instead verified by curling a proxy-enabled
  deployment's ClusterIP with the exact header Aether would send, from a
  throwaway pod inside the cluster, to confirm the target app accepts it
  correctly; the Rust-side HTTP/WebSocket-tunneling code was verified
  end-to-end in an earlier revision of this feature that used a different
  (pod-portforward-based) transport, then swapped in place — a small,
  well-contained change (only *how* a byte stream to the pod is obtained
  changed, not what's done with it), but that swap itself hasn't been
  exercised with a real Aether instance actually running in-cluster yet.
- **Light theme covers chart-chrome/ink tokens only** — the header's "Light
  theme"/"Dark theme" toggle (persisted in `localStorage`, defaulting to
  dark) swaps `--page`/`--surface`/`--surface-raised`/`--border`/
  `--border-strong`/`--text-primary`/`--text-secondary`/`--accent` to their
  validated light-mode steps via `:root[data-theme="light"]` in
  `frontend/style.css`. `--text-muted`, `--accent-ink`, and the status
  palette are intentionally identical in both modes (per the design
  system), so they're inherited rather than overridden. Keep new UI work on
  these custom properties rather than introducing new hex values, so it
  themes correctly for free.
- **Cross-compiling `linux/amd64` locally from an arm64 Mac doesn't work**
  (QEMU crashes `rustc`) — this is why the image is built by CI, not on a
  dev machine. If you ever need a local amd64 build, do it on an actual
  amd64 box, not by fighting emulation.
