# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- Horizontal scaling and zero-downtime rolling restarts: `replicaCount` is
  now a chart value (was hardcoded to 1), with a `RollingUpdate` strategy
  (`maxUnavailable: 0, maxSurge: 1`) so a rollout never drops below full
  capacity, even at the default of one replica.
- Graceful shutdown: the backend now drains in-flight HTTP requests on
  `SIGTERM` instead of dropping them immediately, pausing briefly first to
  give Kubernetes time to remove the terminating pod from Service
  endpoints (in place of a `preStop` hook, since the distroless runtime
  image has no shell to run one).
- A new global admin setting, `allow_custom_images` (Quotas tab, default
  `true`), restricts non-admin launches to an image already in the Images
  catalog or an existing Template's own image when turned off — enforced
  server-side (`POST /api/deployments` 400s otherwise), with the Launch
  tab hiding the "Custom" option and disabling image editing to match.
  Admins are always exempt.
- Dedicated **Model**, **context length**, **quantization**,
  **served model name**, **GPU memory utilization**, and **dtype** fields
  (Launch and Templates forms), substituted for `{{model}}`,
  `{{context_length}}`, `{{quantization}}`, `{{served_model_name}}`,
  `{{gpu_memory_utilization}}`, and `{{dtype}}` in `args` — pull the flags
  every LLM-serving template actually needs edited (vLLM's `--model`/
  `--max-model-len`/`--quantization`/`--served-model-name`/
  `--gpu-memory-utilization`/`--dtype`, SGLang's equivalents) out of the
  free-text args box. `model` works identically for a Hugging Face model
  ID or a local path under a storage mount (below); `gpu_memory_utilization`
  is validated to `(0.0, 1.0]`. All six are genuinely optional: an `args`
  line referencing one of them is dropped entirely if that field is left
  blank, instead of substituting an empty value and sending a broken
  `--flag=` with nothing after the `=`.
- A new `{{accelerator_count}}` args placeholder, so tensor parallelism
  (or any other flag that should track GPU count) matches whatever was
  actually requested instead of needing a second number kept in sync by
  hand — defaults to `1` if no accelerator was requested, and (unlike the
  six above) never dropped. vLLM/SGLang's seeded templates now use all
  seven placeholders instead of a hand-edited placeholder string.
- A storage mount for launches/templates: `volume_claim_name` +
  `volume_mount_path` (+ optional `volume_sub_path`) mount an *existing*
  `PersistentVolumeClaim` into the container — e.g. a shared model cache,
  so `model` can be a local path instead of re-downloading on every
  restart. Aether never creates or deletes a PVC itself; a new
  `GET /api/pvcs` (any logged-in user) lists what already exists in the
  namespace to back the forms' datalist, and the claim is confirmed to
  actually exist before the Deployment is created (400 immediately on a
  typo, rather than a pod stuck `Pending` with an opaque mount-failure
  event). New RBAC verbs: `persistentvolumeclaims` `get`/`list`.

### Changed

- Quota enforcement's launch-serializing lock (`AppState::lock_launches`)
  is now a Postgres advisory lock instead of an in-process `Mutex` — the
  in-process version only serialized concurrent requests within a single
  replica, which would have silently under-enforced quota the moment a
  second replica (or an old+new pod overlapping mid-rollout) existed.
- Launching no longer requires picking a name at all: `POST /api/deployments`
  dropped its `name` field entirely and now auto-generates
  `<username>-<instance type>-<random 6-char suffix>` (instance type is a
  slug of `template_name`, or of `image`'s repository component for a
  Custom launch), so nothing ever needs to be unique by anything you'd
  have to think about — not even across your own launches. Usernames are
  now validated against the same DNS-1123 grammar as a Kubernetes name
  (lowercase alphanumeric and `-` only) since a username is now also part
  of one — tighter than the previous rule, which allowed uppercase, `.`,
  and `_`.

## [0.1.1] - 2026-09-03

### Fixed

- `.github/workflows/release.yml` pushed to `ghcr.io/${{ github.repository_owner }}/...`,
  which resolves to this org's actual display name ("Techboredom") — but
  Docker/OCI repository names must be all-lowercase, so both the amd64 and
  arm64 build-and-push-by-digest jobs failed identically on `v0.1.0`.
  Hardcoded lowercase instead.
- The Forgejo-internal build pipeline (`.forgejo/workflows/build.yml`) now
  also builds multi-arch (amd64+arm64) images, via `docker buildx` +
  QEMU emulation on its single runner rather than the public pipeline's
  two native per-arch runners — build time isn't critical there the way
  it is for a tagged public release.

## [0.1.0] - 2026-09-02

First tagged release: packaged for others to run, not just this project's
own cluster.

### Added

- Apache-2.0 `LICENSE` and `NOTICE`, `SECURITY.md`, `CONTRIBUTING.md`.
- A Helm chart (`charts/aether/`) so Aether can be installed on any
  Kubernetes cluster, not just the one it was originally built for.
- A public CI/release pipeline (`.github/workflows/`) building multi-arch
  (amd64+arm64) images and packaging/publishing the Helm chart as an OCI
  artifact on tagged releases.

### Changed

- `public_service` (whether a launched deployment gets a public
  `LoadBalancer` Service) now defaults to `false`. Per-deployment proxy
  origins are the intended access path; a public LoadBalancer with no
  ingress/LB controller in front of it is the thing most likely to look
  broken on a fresh cluster. The option is unchanged otherwise.

### Fixed

- `ProxyOrigin::deployment_for_host` now matches the `Host` header
  case-insensitively.
- README corrected: login rate limiting is implemented (the "known gaps"
  section still listed it as missing, contradicting the security-notes
  section describing the throttle).

[Unreleased]: https://github.com/Techboredom/Aether/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/Techboredom/Aether/releases/tag/v0.1.1
[0.1.0]: https://github.com/Techboredom/Aether/releases/tag/v0.1.0
