# Aether

A dashboard for a Kubernetes namespace, behind a login, with up to four tabs
depending on your role. It's the web-interface slice of the broader platform
described in `SPEC.md` — the Intelligence Layer (LLM engines) and Interface
Layer (IDEs) as launchable workloads — scoped down to what's actually
buildable today: this cluster has no ingress controller or StorageClass yet,
so there's no Gateway layer or persistent storage (see "Status & known
limitations" below).

There are two account roles: **admin** and **user**. Both can use Pods and
Launch; only admins see Templates and Users.

- **Pods** — shows the running pods live, with their basic resource info:
  CPU/memory requests and limits, accelerators (GPUs, etc.), status, node,
  restarts, and age. A regular `user` only sees pods they launched
  themselves; an `admin` sees every pod in the namespace plus an **Owner**
  column showing who launched each one. A **Credential** column shows the
  auto-generated login token/password/API key for templates that have one,
  click-to-select for copying — and for proxy-enabled templates (JupyterLab),
  an **Open** link that lands you in an already-logged-in session with zero
  copy-paste (see "Ownership, auto-generated credentials, and the reverse
  proxy" below). Click a row to open its detail panel: per-container state
  and failure reason (`CrashLoopBackOff`, `ImagePullBackOff`, exit codes,
  etc.), recent Kubernetes Events, and a log viewer (container picker, tail
  length, previous-container logs for ones that crashed).
- **Launch** — creates a `Deployment` (and, if a container port is given and
  the template isn't proxy-enabled, a matching `LoadBalancer` Service, since
  there's no ingress) in that namespace, owned by whichever account launched
  it. The form's first field is a **Template** dropdown — Ollama, vLLM,
  SGLang (Intelligence Layer) or JupyterLab, RStudio (Interface Layer), or
  **Custom** — which pre-fills image, port, resource sizing, GPU defaults,
  and any default env vars/args for that template. Every pre-filled field
  stays editable, and Custom picks any image from the Postgres-backed image
  catalog instead. Templates that carry a `secret_env_key` (JupyterLab,
  RStudio, vLLM) hide that field from the form entirely and generate a
  random value for it at launch time instead — shown once in the success
  message and persistently on the Pods tab, no need to invent or type one
  yourself. JupyterLab additionally skips the public Service altogether and
  is only reachable by clicking "Open" — Aether proxies straight into it
  with the token already applied.
- **Templates** *(admin only)* — CRUD for the templates the Launch tab
  offers: a table of existing templates (edit/delete) and a form to add a
  new one (same fields as a template pre-fills into Launch, plus notes shown
  when it's selected there, plus an optional "auto-generate a secret for
  this env var" field and a "proxy through Aether" checkbox — see below).
- **Users** *(admin only)* — create accounts (username, password, role),
  delete them, and reset any account's password without knowing the old one
  (forces that account to log in again everywhere, on every device). Still
  no self-service signup — an admin creates every account.

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
frontend/src/login.rs      Login page
frontend/src/pods_tab.rs   Pods tab + detail panel
frontend/src/create_deployment_tab.rs   Launch tab (template dropdown + form)
frontend/src/templates_tab.rs   Templates admin tab (CRUD)
frontend/src/users_tab.rs       Users admin tab (CRUD)
frontend/src/env_editor.rs      Shared add/remove env-var-row widget (Launch + Templates)
k8s/                       Kubernetes manifests to deploy the dashboard itself
Dockerfile                 Multi-stage build: compiles both crates, ships a distroless image
.forgejo/                  Forgejo Actions workflow that builds and pushes the image
SPEC.md                    The broader platform vision this app is a slice of
```

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

### Endpoints

All endpoints below except `POST /api/login` and static assets require a
valid session cookie (401 if missing/expired); the ones marked *(admin)*
additionally require the `admin` role (403 otherwise).

- `POST /api/login` — body `{username, password}`; sets the `aether_session` cookie and returns the logged-in `UserInfo` on success, 401 on bad credentials
- `POST /api/logout` — clears the session (both server-side and the cookie)
- `GET /api/me` — returns the current `UserInfo` (`{id, username, role}`), or 401 if not logged in — this is what the frontend polls on load to decide whether to show the login page
- `PUT /api/me/password` — body `{current_password, new_password}`; changes your own password, 400 if `current_password` doesn't match. Deletes every other session for your account (`DELETE FROM sessions WHERE user_id = $1 AND token != $2`) but leaves the one making this request logged in.
- `GET /api/users` *(admin)* — list accounts (id, username, role — never password hashes)
- `POST /api/users` *(admin)* — create an account; body `{username, password, role}` (`role` is `"admin"` or `"user"`); username 3-32 chars, password ≥ 8 chars
- `DELETE /api/users/{id}` *(admin)* — delete an account; an admin can't delete their own account (guards against an easy self-lockout)
- `PUT /api/users/{id}/password` *(admin)* — body `{password}`; resets another account's password without needing the old one — the admin role itself is the authorization. Deletes **all** of that account's sessions (there's no "current session" to preserve, since it isn't the admin's own).
- `GET /api/pods` — JSON snapshot of the current pods in the watched namespace, filtered by role: a `user` only gets pods whose `aether.io/owner` label matches their own username, an `admin` gets all of them (each with its `owner` field populated). Pods for templates with a `secret_env_key` also carry a `credential: {env_key, value}` looked up from `deployment_secrets`, and pods for proxy-enabled templates carry a `proxy_path: "/proxy/<name>/"`.
- `GET /ws` — WebSocket; sends a full snapshot on connect (same per-role filtering and credential enrichment as `GET /api/pods`), then `upsert`/`delete` events as pods change, filtered the same way per-connection
- `GET /api/images` — JSON list of catalog entries from the `images` table (id, name, image, description)
- `GET /api/templates` — JSON list of templates (any logged-in role — needed for the Launch tab's dropdown)
- `POST /api/templates` / `PUT /api/templates/{id}` *(admin)* — create/update a template. Body is a `TemplateEntry` minus `id`: `{name, image, container_port, cpu_request, cpu_limit, memory_request, memory_limit, accelerator_type, accelerator_count, env, args, notes, secret_env_key, proxy_enabled}` — only `name`/`image` are required, everything else defaults to empty/`null`/`false`. `secret_env_key`, if set, is the env var name (e.g. `JUPYTER_TOKEN`) that Launch should auto-generate instead of showing as an editable field. `proxy_enabled`, if `true`, requires `secret_env_key` to also be set (400 otherwise).
- `DELETE /api/templates/{id}` *(admin)* — delete a template
- `POST /api/deployments` — creates a `Deployment` in the watched namespace (labeled `aether.io/owner: <your username>`), and if `container_port` is set and `enable_proxy` isn't, also a `LoadBalancer` Service exposing it (no ingress controller in the cluster, so this is how a non-proxied launched app becomes reachable). Body: `{name, image, replicas, cpu_request, cpu_limit, memory_request, memory_limit, accelerator_type, accelerator_count, container_port, env, args, generate_secret_for, enable_proxy}` — everything except `name`/`image`/`replicas` is optional; `env` is `[[key, value], ...]` pairs (entries with an empty value are dropped, so an image's own default behavior — e.g. an auto-generated password logged at startup — still applies unless you set one); `args` is a list of container command-line arguments (any occurrence of the literal string `{{name}}` is substituted with the deployment's own name first); `generate_secret_for`, if set to an env var name, generates a random value for it (overriding anything with that key in `env`) and stores it in `deployment_secrets`; `enable_proxy`, if `true`, requires both `generate_secret_for` and `container_port` to be set (400 otherwise), skips creating a Service entirely, and makes the app reachable only via `GET/POST/... /proxy/<name>/...`. Response adds `service_name`/`container_port` (both `null` if no port was given, `service_name` also `null` when proxied), `secret_value` (the generated value, or `null`), and `proxy_path` (`"/proxy/<name>/"` if `enable_proxy` was set, else `null`).
- `ANY /proxy/{deployment_name}/{*rest}` — reverse-proxies into a proxy-enabled deployment's pod (`backend/src/proxy.rs`), injecting its generated credential as the appropriate auth header so there's no login prompt. 403 if you're not that deployment's owner (or an admin); 400 if the deployment isn't proxy-enabled; 502 if it has no running pod yet or the tunnel fails. Handles WebSocket upgrades transparently (needed for JupyterLab's kernel connections). See "Ownership, auto-generated credentials, and the reverse proxy" below.
- `GET /api/pods/{name}/logs?container=&tail_lines=&previous=` — plain-text container logs (`container` defaults to the pod's only container if it has one; `tail_lines` defaults to 500; `previous=true` gets the last terminated instance's logs, for a crashed container)
- `GET /api/pods/{name}/events` — JSON list of Kubernetes Events involving that pod (`type_`, `reason`, `message`, `count`, `last_seen`), most recent first — note the apiserver's default Event TTL is short (commonly ~1h), so older pods often have none left
- `GET /*` — serves the built frontend (`index.html`, JS, WASM, CSS)

## Image and template catalogs (Postgres)

The `images` table (schema in `backend/migrations/0001_create_images.sql`,
applied automatically on startup via `sqlx::migrate!`) backs the image
catalog used by "Custom" mode on the Launch tab. There's no admin UI for it
yet (unlike templates, below) — add entries with plain SQL:

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
    proxy_enabled BOOLEAN NOT NULL DEFAULT false,  -- reverse-proxy instead of a public Service; requires secret_env_key
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

It ships seeded with the same five templates (Ollama, vLLM, SGLang,
JupyterLab, RStudio) that used to be hardcoded in the frontend — edit or
delete them from the Templates tab like any other row. JupyterLab
(`JUPYTER_TOKEN`), RStudio (`PASSWORD`), and vLLM (`VLLM_API_KEY`) are seeded
with a `secret_env_key`; Ollama and SGLang aren't (Ollama has no auth
mechanism at all, and SGLang's equivalent env var name wasn't confirmed).
Only JupyterLab is seeded `proxy_enabled` — see below.

## Ownership, auto-generated credentials, and the reverse proxy

Every `Deployment`/pod created via Launch is labeled `aether.io/owner:
<username>` (a Kubernetes label, kept separate from the `app: <name>`
selector label so it can't interfere with Service routing). The Pods tab
and its underlying REST/WebSocket endpoints filter on this label: a `user`
account only ever sees pods it launched itself; an `admin` sees everything,
with an extra **Owner** column.

Templates with a `secret_env_key` (JupyterLab, RStudio, vLLM) don't expose
that field as editable input on the Launch form at all — instead, the
backend generates a random 48-character alphanumeric value (the same
generator used for session tokens), injects it as that env var on the
container, and stores it in a `deployment_secrets` table keyed by the
Deployment's name:

```sql
CREATE TABLE deployment_secrets (
    deployment_name TEXT PRIMARY KEY,
    namespace TEXT NOT NULL,
    env_key TEXT NOT NULL,
    secret_value TEXT NOT NULL,
    owner_username TEXT NOT NULL,
    proxy_enabled BOOLEAN NOT NULL DEFAULT false,
    container_port INTEGER,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

The value is shown once in the Launch success message and persistently in
the Pods tab's Credential column (for whoever can see that pod — the same
ownership filtering applies). Re-launching under the same Deployment name
replaces the stored value.

**JupyterLab goes one step further and genuinely works like JupyterHub**:
its template is `proxy_enabled`, which doesn't change what gets created at
launch time (it still gets the same `LoadBalancer` Service as any other
templated app, directly reachable the same way) — it adds a *second*,
friction-free way in: Aether's own `GET/POST/... /proxy/<name>/{*rest}`
route (`backend/src/proxy.rs`), which:

1. Checks you own that deployment (or are an admin) — same rule as the Pods
   tab's visibility filtering.
2. Looks up that Service and connects to its in-cluster `ClusterIP` directly
   (`kube`'s `Api<Service>::get`, RBAC already covers `get` on `services`).
   This is the conventional in-cluster design, and it assumes Aether itself
   is running **in-cluster** — a `ClusterIP` isn't routable from outside the
   cluster network, so this specific hop can't be exercised with the backend
   running locally against a remote cluster, unlike everything else in this
   app (see "Status & known limitations"). The connection attempt times out
   after 5 seconds either way, so a stuck/unready pod fails fast rather than
   hanging the request.
3. Injects `Authorization: token <value>` (Jupyter Server's documented
   token-header convention) into every proxied request, so you land directly
   in a logged-in JupyterLab session — click "Open" on the Pods tab (or the
   Launch success message) and go, no token to copy or paste anywhere.
4. Transparently tunnels WebSocket upgrades too (via `hyper::upgrade` +
   `tokio::io::copy_bidirectional`), which is what makes JupyterLab's kernel
   connections (running notebook cells) work through the proxy, not just
   static pages.

This requires the template's own `args` to tell the app it's being served
under a path prefix — JupyterLab's seeded args are `["start-notebook.sh",
"--ServerApp.base_url=/proxy/{{name}}/"]`, where `{{name}}` is a generic
placeholder substituted with the deployment's own name at launch time (any
template's `args` can use it, not just JupyterLab's). **Only JupyterLab
ships proxy-enabled today** — it has documented, reliable support for both
the path-prefix flag and the token-header convention. RStudio's equivalent
(`www-root-path`) hasn't been verified against the `rocker/rstudio` image's
entrypoint, so it keeps its own public LoadBalancer Service and manual
credential paste for now (flip its `proxy_enabled` column once that's
checked). vLLM is intentionally never proxied: its `VLLM_API_KEY` is meant
for scripted API clients setting their own `Authorization: Bearer <key>`
header, not a browser session — it already matches real
bearer-token-via-header usage without needing a proxy in front of it, and
forcing it through Aether's cookie-based login would only get in the way of
automation.

Each proxied HTTP request currently opens a fresh TCP connection and
HTTP/1.1 handshake to the pod rather than reusing a pooled connection —
correct and simple, but adds latency per request; pooling is a reasonable
future optimization, not a correctness issue. The connection attempt times
out after 5 seconds, so an unready or unreachable pod fails fast (502)
instead of hanging the request.

## Building the container image

Cross-compiling for `linux/amd64` from an arm64 machine (e.g. Apple Silicon)
via QEMU reliably crashes `rustc`, so build this on a native amd64 box — CI
handles this (see below). If you're already on amd64:

```
docker build -t <registry>/aether:latest .
docker push <registry>/aether:latest
```

The image is a distroless (`gcr.io/distroless/cc-debian12:nonroot`) runtime
containing just the compiled backend binary and the built frontend assets —
no shell, runs as a non-root user.

## CI (Forgejo Actions)

`.forgejo/workflows/build.yml` builds and pushes the image to
`ctr.int.example.com:8443/aether` on every push to `main`, on version tags
(`v*`), or via manual dispatch. It tags the image with both the short git SHA
and `latest`.

Requires two repository secrets in Forgejo (Settings → Actions → Secrets):

- `REGISTRY_USER`
- `REGISTRY_PASSWORD`

The runner must be labeled `docker` (matching `runs-on: docker` in the
workflow) and have Docker available.

## Deploying to Kubernetes

Manifests live in `k8s/` and deploy into the `ollama` namespace (the app
always watches its own namespace, via the pod's `metadata.namespace`, so
watched namespace = deployed namespace). They're pinned to `amd64` nodes via
`nodeSelector`, since the image is amd64-only.

1. **Image pull secret** — the registry requires auth, so the cluster needs
   credentials to pull the image:

   ```
   kubectl create secret docker-registry regcred \
     --docker-server=ctr.int.example.com:8443 \
     --docker-username=<user> --docker-password=<password> \
     -n ollama
   ```

2. **Database secret** — the app needs a Postgres connection string for the
   image catalog:

   ```
   kubectl create secret generic aether-db -n ollama \
     --from-literal=DATABASE_URL='postgres://user:pass@host:5432/dbname'
   ```

3. **Apply the manifests:**

   ```
   kubectl apply -k k8s/
   ```

   This creates:
   - `ServiceAccount` + `Role`/`RoleBinding` (`aether`) — scoped to the
     `ollama` namespace only, no cluster-wide permissions: `get`/`list`/`watch`
     on `pods` plus `get` on `pods/log` and `get`/`list` on `events` (Pods tab
     and its detail panel), and `create`/`get` on `apps/deployments` and on
     `services` (Launch tab — the Service is how a launched app becomes
     reachable, since there's no ingress controller; `get` is also used by
     the reverse proxy to look up a proxy-enabled deployment's ClusterIP).
   - `Deployment` (`aether`) — 1 replica, resource requests/limits set, health
     probes on `/api/pods`, hardened `securityContext` (non-root, read-only
     root filesystem, all capabilities dropped).
   - `Service` (`aether`) — `type: LoadBalancer`, port `3000`, gets an
     external IP from the cluster's MetalLB pool (`192.0.2.0/24`), same
     pattern as the existing `ollama01` service.

4. **Find the external IP and open it:**

   ```
   kubectl get svc -n ollama aether
   ```

   Then browse to `http://<external-ip>:3000`.

### Watching a different namespace

To watch a namespace other than `ollama`, either:

- Edit the `namespace:` field in `k8s/kustomization.yaml` (and re-run
  `kubectl create secret docker-registry regcred ...` in that namespace too), or
- Deploy a second copy of `k8s/` with a different `namespace` and a distinct
  set of resource names, if you want multiple dashboards watching different
  namespaces at once.

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
  or gets a random value visible only in the pod's own logs. JupyterLab,
  RStudio, and vLLM get an auto-generated credential, stored **in plaintext**
  in the `deployment_secrets` table (no encryption at rest) and visible to
  the owning user and any admin via the Pods tab.
- Pod ownership (`aether.io/owner` label) and the Pods-tab visibility
  filtering it drives are enforced entirely in the Aether backend at read
  time, not via Kubernetes RBAC or admission control — the label itself is
  just metadata anyone with direct `kubectl` access to the namespace can see
  or edit. It restricts what Aether's UI/API surface shows a `user` account,
  not what's actually running in the cluster.
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
`user`-role account: no Service was created, `/proxy/<name>/` served
JupyterLab with the token already applied (no login prompt), a second
non-owning user got 403 while an admin could still open it, and — the part
most likely to silently break — a real notebook cell was executed through
the proxied WebSocket kernel connection and returned the correct output,
confirming the upgrade-tunneling code path actually works and isn't just
serving static pages. Known gaps, in case they matter for what you do next:

- **Still no "forgot password" self-service flow** — that requires emailing
  a reset link, which this app has no mechanism for (no SMTP config, no
  email field on accounts). An admin can reset a locked-out user's password
  from the Users tab instead (see below), which covers the "I forgot it"
  case even without a self-service link.
- **No login rate limiting.** `POST /api/login` has no lockout/backoff, so
  nothing but password strength (≥ 8 chars, enforced at creation) stands
  between an attacker and password guessing.
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
- **Not yet deployed for real.** `k8s/` has only ever been validated with
  `--dry-run=server`; nobody has run `kubectl apply -k k8s/` for real. You
  still need to create the `regcred` and `aether-db` secrets in-cluster and
  actually apply it (see "Deploying to Kubernetes" above).
- **No in-cluster Postgres.** `DATABASE_URL` currently has to point at a
  Postgres you already run somewhere; there's no `k8s/postgres.yaml`. Add
  one if you want this to be fully self-contained.
- **No admin UI for the image catalog** (only for templates) — entries are
  added with raw SQL (see "Image and template catalogs" above). Fine for a
  handful of images, less so at scale.
- **No confirmation on template edits**, only on delete — saving over an
  existing template's fields is immediate.
- **Single namespace only**, fixed at deploy time via the pod's own
  namespace. No in-app namespace switcher; watching multiple namespaces
  means deploying multiple copies (see "Watching a different namespace").
- **No way to scale, delete, or edit a Deployment/Service from the UI** —
  only create. Same for pods: no delete/restart action, view-only plus logs.
- **`VLLM_API_KEY` is an educated guess, not a confirmed env var name** — it
  hasn't been verified against a real vLLM server run (see the vLLM
  templates gap above). If vLLM ignores it, the generated value shown in the
  UI simply won't do anything.
- **Auto-generated credentials are plaintext in Postgres**, not a Kubernetes
  `Secret` or any encrypted store, and persist after the pod that used them
  is gone (no cleanup job) — anyone with `deployment_secrets` table access
  can read every credential ever generated, past or present.
- **Only JupyterLab gets true JupyterHub-style transparent auth** (reverse
  proxy + injected header, click "Open" and you're in). RStudio and vLLM
  still just display a credential for copy/paste — RStudio because its
  path-prefix support is unverified (see the vLLM/SGLang gap above for the
  same kind of caveat), vLLM because it's meant for scripted API clients
  where a proxied cookie-auth flow would be more friction, not less. See
  "Ownership, auto-generated credentials, and the reverse proxy" above.
- **The reverse proxy opens a fresh TCP connection per HTTP request**, not a
  pooled/reused one — correctness over performance for this first pass. Fine
  for interactive single-user use; would need pooling before it'd hold up
  under heavier concurrent load.
- **The reverse proxy assumes Aether itself runs in-cluster** — it connects
  to a proxy-enabled deployment's Service via its `ClusterIP`, which isn't
  routable from outside the cluster network. This is the one code path in
  this app that can't be exercised with the backend running locally against
  a remote cluster (this project's usual local-dev pattern); testing it for
  real requires actually deploying Aether via `k8s/` (see "Not yet deployed
  for real" below) — it was instead verified by curling a proxy-enabled
  deployment's ClusterIP with the exact header Aether would send, from a
  throwaway pod inside the cluster, to confirm the target app accepts it
  correctly; the Rust-side HTTP/WebSocket-tunneling code was verified
  end-to-end in an earlier revision of this feature that used a different
  (pod-portforward-based) transport, then swapped in place — a small,
  well-contained change (only *how* a byte stream to the pod is obtained
  changed, not what's done with it), but that swap itself hasn't been
  exercised with a real Aether instance actually running in-cluster yet.
- **Dashboard is dark-mode only**, no light theme or user preference toggle.
  Colors are pulled from the project's validated dark dashboard palette
  (status/accent/surface tokens) rather than picked ad hoc — keep new UI
  work on those tokens (`frontend/style.css` custom properties) rather than
  introducing new hex values.
- **Cross-compiling `linux/amd64` locally from an arm64 Mac doesn't work**
  (QEMU crashes `rustc`) — this is why the image is built by CI, not on a
  dev machine. If you ever need a local amd64 build, do it on an actual
  amd64 box, not by fighting emulation.
