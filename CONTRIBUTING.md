# Contributing to Artmark

Thanks for helping improve Artmark. It is a local catalog of pointers and search cards. Please read [AGENTS.md](AGENTS.md) and [spec.md](spec.md) before changing product behavior: Artmark does not store source documents or retrieve them from providers.

## Develop locally

Use a recent stable Rust toolchain. The release scripts and their tests use Python 3.11 or newer.

```bash
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
python3 -m unittest discover -s tests -p 'test_*.py'
```

The CLI uses SQLite with FTS5 and works without embeddings or a network connection. The default database is in `~/.artmark`; use `ARTMARK_DB` or `--database` for an isolated test database.

## Open a pull request

Create a branch for each change to `main`. Every PR advances the Artmark package version once in both `Cargo.toml` and `Cargo.lock`, even for documentation or internal changes. Run `cargo check` after changing the manifest so the lockfile matches.

| Change | Label | Example from `0.0.2` |
| --- | --- | --- |
| Fix, docs, or internal change | `semver:patch` | `0.0.3` |
| New feature or incompatible change during `0.x` | `semver:minor` | `0.1.0` |
| First stable release or later breaking change | `semver:major` | `1.0.0` |

Add exactly one matching release label. The `Release label and version` check compares it with the PR's version bump. `Format and lint` and the four native build checks must also pass before merge. A PR with several commits still gets one version bump.

Dependabot vulnerability alerts are enabled for this repository. When addressing an alert, update the affected dependency in a normal PR with the appropriate Artmark version bump and label. Dependency-only PRs follow the same release policy.

## Releases

A successful push to `main` builds and tests the CLI on Linux x86-64, Windows x86-64, macOS Intel, and macOS Apple Silicon. The workflow publishes the new `vX.Y.Z` release after all four archives and checksums pass verification. Each archive includes the binary, README, license, contributor and agent guidance, spec, and agent skill. GitHub Releases are the primary install channel; the crate is not published to crates.io.
