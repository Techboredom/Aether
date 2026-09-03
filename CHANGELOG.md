# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

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
