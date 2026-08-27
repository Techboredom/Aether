# Aether

A small dashboard that shows the pods running in a Kubernetes namespace, live,
with their basic resource info: CPU/memory requests and limits, accelerators
(GPUs, etc.), status, node, restarts, and age.

- **Backend**: Rust, [Axum](https://github.com/tokio-rs/axum) +
  [kube-rs](https://kube.rs). Watches one namespace via the Kubernetes watch
  API and serves a REST snapshot plus a WebSocket that pushes live updates.
- **Frontend**: Rust, [Leptos](https://leptos.dev) (client-side, compiled to
  WASM with [Trunk](https://trunkrs.dev)). Loads the initial snapshot over
  REST, then stays in sync over the WebSocket.
- **Repo**: `ssh://git@git.example.com:2022/Aether/Aether-Web.git`

## Layout

```
common/     Shared types (PodInfo, PodEvent) used by both backend and frontend
backend/    Axum server + kube-rs watcher
frontend/   Leptos WASM app (built with Trunk)
k8s/        Kubernetes manifests to deploy the dashboard itself
Dockerfile  Multi-stage build: compiles both crates, ships a distroless image
.forgejo/   Forgejo Actions workflow that builds and pushes the image
```

## Running locally

Prerequisites:

```
rustup target add wasm32-unknown-unknown
cargo install trunk
```

Build the frontend, then run the backend (which also serves the built frontend):

```
cd frontend && trunk build --release && cd ..
cd backend
NAMESPACE=<namespace-to-watch> cargo run --release -- --static-dir ../frontend/dist
```

Open `http://localhost:3000`. Auth to the cluster auto-detects: in-cluster
service account first, falling back to your local kubeconfig (`~/.kube/config`,
current context) — the same as `kubectl`.

### Backend configuration

All flags can also be set as environment variables:

| Flag / env var | Default | Description |
|---|---|---|
| `--namespace` / `NAMESPACE` | *(required)* | Namespace to watch |
| `--bind-addr` / `BIND_ADDR` | `0.0.0.0:3000` | Address the HTTP server binds to |
| `--static-dir` / `STATIC_DIR` | `frontend/dist` | Directory of the built frontend to serve |

### Endpoints

- `GET /api/pods` — JSON snapshot of the current pods in the watched namespace
- `GET /ws` — WebSocket; sends a full snapshot on connect, then `upsert`/`delete` events as pods change
- `GET /*` — serves the built frontend (`index.html`, JS, WASM, CSS)

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

2. **Apply the manifests:**

   ```
   kubectl apply -k k8s/
   ```

   This creates:
   - `ServiceAccount` + `Role`/`RoleBinding` (`aether-pod-reader`) — read-only
     (`get`/`list`/`watch`) access to `pods`, scoped to the `ollama` namespace
     only. No cluster-wide permissions.
   - `Deployment` (`aether`) — 1 replica, resource requests/limits set, health
     probes on `/api/pods`, hardened `securityContext` (non-root, read-only
     root filesystem, all capabilities dropped).
   - `Service` (`aether`) — `type: LoadBalancer`, port `3000`, gets an
     external IP from the cluster's MetalLB pool (`192.0.2.0/24`), same
     pattern as the existing `ollama01` service.

3. **Find the external IP and open it:**

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

- RBAC is intentionally minimal: a namespaced `Role` with only
  `get`/`list`/`watch` on `pods`, nothing else, no `ClusterRole`.
- The container runs as a non-root user with a read-only root filesystem and
  all Linux capabilities dropped.
- `CorsLayer::permissive()` is enabled on the backend to make local `trunk
  serve` dev proxying easy. It's harmless behind the cluster's internal
  network, but tighten it if this is ever exposed beyond localhost/your LAN.
