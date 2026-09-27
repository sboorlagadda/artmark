# Changelog

Artmark follows [Semantic Versioning](https://semver.org/). During `0.x`, the CLI and database schema can change between minor releases.

## [0.1.0] - 2026-09-27

First public release. The CLI and local SQLite FTS5 catalog are available on Linux x86-64, macOS Intel, macOS Apple Silicon, and Windows x86-64.

- Added a security reporting policy, issue templates, and an architecture guide.
- Documented early maturity, a complete install path, and macOS Gatekeeper behavior.
- Added dependency auditing and clean-install smoke tests to release CI.

## [0.0.4] - 2026-09-27

- Refined the README around durable artifact bookmarks and the agent workflow.

## Earlier development releases

Versions `0.0.1` through `0.0.3` established the Rust CLI, SQLite registry, local FTS5 search, cross-platform release archives, and agent skill. These were pre-public development builds.

[0.1.0]: https://github.com/sboorlagadda/artmark/releases/tag/v0.1.0
[0.0.4]: https://github.com/sboorlagadda/artmark/releases/tag/v0.0.4
