# Contributing to Kroa

Thank you for contributing. Kroa is an Alpha language; keep changes focused and well tested.

## Development environment

1. Install Rust (stable) and LLVM/Clang 18+.
2. Clone the repository and use the `develop` branch for new work.
3. Run the local gate before opening a PR:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test -- --test-threads=1
```

## Branch model

| Branch | Purpose |
|--------|---------|
| `main` | Production releases only |
| `develop` | Integration for the next Alpha |
| `feature/*`, `fix/*` | Short-lived work branches |

Open pull requests against `develop` unless you are preparing a release into `main`.

## Coding rules

- Prefer the smallest change that fixes or implements the request.
- Keep diagnostics English-only with stable codes when applicable.
- Do not introduce `&&`, `||`, bare `!`, or tabs into Kroa source examples.
- Update bilingual documentation when behavior or public APIs change.
- Update `CHANGELOG.md` and `PROJECT_STATUS.md` for user-visible changes.

## Pull requests

- Fill out the PR template.
- Link related issues when they exist.
- Ensure CI is green on Linux and Windows.
- Keep commits readable; squash only when the history is noisy.

## Releases

Releases are production-only and must follow [`docs/en/versioning.md`](docs/en/versioning.md).
Do not advertise development artifacts as public releases.
