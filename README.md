# Aether

A small dashboard for a Kubernetes namespace, with two tabs:

- **Pods** — shows the running pods live, with their basic resource info:
  CPU/memory requests and limits, accelerators (GPUs, etc.), status, node,
  restarts, and age.
- **Create Deployment** — a form to create a new `Deployment` in that
  namespace: pick a container image from a Postgres-backed catalog, set
  replicas, CPU/memory requests+limits, and an optional accelerator
  type+count.

- **Backend**: Rust, [Axum](https://github.com/tokio-rs/axum) +
  [kube-rs](https://kube.rs) + [sqlx](https://github.com/launchbadge/sqlx)
  (Postgres). Watches one namespace via the Kubernetes watch API and serves a
  REST snapshot plus a WebSocket that pushes live pod updates; also serves the
  image catalog and creates Deployments on request.
- **Frontend**: Rust, [Leptos](https://leptos.dev) (client-side, compiled to
  WASM with [Trunk](https://trunkrs.dev)). Loads the initial pod snapshot over
  REST, then stays in sync over the WebSocket.
- **Repo**: `ssh://git@git.example.com:2022/Aether/Aether-Web.git`

## Layout

```
common/             Shared types (PodInfo, PodEvent, ImageEntry, CreateDeploymentRequest, ...)
backend/            Axum server, kube-rs watcher, sqlx/Postgres image catalog
backend/migrations/ sqlx migrations, auto-run on startup
frontend/           Leptos WASM app (built with Trunk), one component per tab
k8s/                Kubernetes manifests to deploy the dashboard itself
Dockerfile          Multi-stage build: compiles both crates, ships a distroless image
.forgejo/           Forgejo Actions workflow that builds and pushes the image
```

## Running locally

Prerequisites:

```
rustup target add wasm32-unknown-unknown
cargo install trunk
```

You'll also need a Postgres instance for the image catalog. For local dev,
any throwaway instance works — the backend creates the `images` table itself
on startup:

```
docker run -d -p 5432:5432 -e POSTGRES_PASSWORD=postgres postgres:16-alpine
```

Build the frontend, then run the backend (which also serves the built frontend):

```
cd frontend && trunk build --release && cd ..
cd backend
NAMESPACE=<namespace-to-watch> \
DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres \
cargo run --release -- --static-dir ../frontend/dist
```

Open `http://localhost:3000`. Auth to the cluster auto-detects: in-cluster
service account first, falling back to your local kubeconfig (`~/.kube/config`,
current context) — the same as `kubectl`.

### Backend configuration

All flags can also be set as environment variables:

| Flag / env var | Default | Description |
|---|---|---|
| `--namespace` / `NAMESPACE` | *(required)* | Namespace to watch and to create Deployments in |
| `--bind-addr` / `BIND_ADDR` | `0.0.0.0:3000` | Address the HTTP server binds to |
| `--static-dir` / `STATIC_DIR` | `frontend/dist` | Directory of the built frontend to serve |
| `--database-url` / `DATABASE_URL` | *(required)* | Postgres connection string for the image catalog |

### Endpoints

- `GET /api/pods` — JSON snapshot of the current pods in the watched namespace
- `GET /ws` — WebSocket; sends a full snapshot on connect, then `upsert`/`delete` events as pods change
- `GET /api/images` — JSON list of catalog entries from the `images` table (id, name, image, description)
- `POST /api/deployments` — creates a `Deployment` in the watched namespace; body is `{name, image, replicas, cpu_request, cpu_limit, memory_request, memory_limit, accelerator_type, accelerator_count}` (all the resource fields and `accelerator_type`/`accelerator_count` are optional/nullable)
- `GET /*` — serves the built frontend (`index.html`, JS, WASM, CSS)

## Image catalog (Postgres)

The `images` table (schema in `backend/migrations/0001_create_images.sql`,
applied automatically on startup via `sqlx::migrate!`) backs the dropdown on
the Create Deployment tab:

```sql
CREATE TABLE images (
    id SERIAL PRIMARY KEY,
    name TEXT NOT NULL,        -- display name, e.g. "Ollama (ROCm)"
    image TEXT NOT NULL,       -- image ref, e.g. "ollama/ollama:rocm"
    description TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

There's no admin UI for it yet — add entries with plain SQL against whatever
Postgres instance `DATABASE_URL` points at:

```sql
INSERT INTO images (name, image, description) VALUES
  ('Ollama (ROCm)', 'ollama/ollama:rocm', 'Ollama with AMD ROCm GPU support');
```

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
     on `pods` (Pods tab), plus `create`/`get` on `apps/deployments` (Create
     Deployment tab).
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

- RBAC is intentionally minimal and namespaced (no `ClusterRole`): read-only
  (`get`/`list`/`watch`) on `pods`, plus `create`/`get` on `apps/deployments` —
  nothing else. The app can create Deployments but can't delete, patch, list,
  or watch existing ones, and has no access to Secrets, ConfigMaps, RBAC
  objects, etc.
- The Create Deployment form does not sanitize CPU/memory quantity strings
  beyond checking they're non-empty — malformed values are rejected by the
  Kubernetes API server itself (returned to the UI as an error), not
  pre-validated client- or server-side.
- The container runs as a non-root user with a read-only root filesystem and
  all Linux capabilities dropped.
- `CorsLayer::permissive()` is enabled on the backend to make local `trunk
  serve` dev proxying easy. It's harmless behind the cluster's internal
  network, but tighten it if this is ever exposed beyond localhost/your LAN.
