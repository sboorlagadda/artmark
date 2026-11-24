# Changelog

artmark follows [Semantic Versioning](https://semver.org/). During `0.x`, the CLI and database schema can change between minor releases.

## [0.5.2] - 2026-11-24

- Updated `CHANGELOG.md` through `v0.5.2` and aligned `README.md` release install filenames.
- Made the `README.md` "Try it" section a self-contained, copy-pasteable quickstart using a public URL and an isolated temporary database.

## [0.5.1] - 2026-11-24

- Added end-to-end cross-session agent workflow integration tests (`tests/agent_workflow.rs`) using a mock provider with revisions, bearer credentials, and access-denied handling.
- Added a reproducible two-session CLI walkthrough in `docs/cross-session-walkthrough.md`.

## [0.5.0] - 2026-11-24

- Expanded local URL canonicalization and deduplication coverage across Google Drive/Docs/Sheets/Slides (including `/u/N/d/...`), Figma, GitHub (including PR `/files` and `/commits`), Notion, Slack, generic HTTP(S), and cross-platform Windows/macOS local paths.
- Added pure local URL-path canonicalization rules for Jira, Linear, and Confluence without provider API calls or credentials.
- Added transactional SQLite schema `v3` migration to reconcile legacy canonical keys and aliases on upgrade.

## [0.4.0] - 2026-11-24

- Added versioned SQLite schema migrations (`PRAGMA user_version`, `v1` → `v2`) with standalone pre-migration `VACUUM INTO` backups and automatic rollback on error.
- Upgraded local FTS5 search to `tokenize = 'porter unicode61'` with stopword filtering, AND-first querying with OR fallback, weighted BM25 ranking, and a real-query evaluation suite (`docs/search-evaluation.md`).
- Added local artifact lookup tracking (`access_count`, `last_accessed_at`) on `artmark get` (with `--no-track` opt-out) and surfaced usage summaries in `artmark stats`.

## [0.3.0] - 2026-11-24

- Stopped silent auto-bookmarking on pasted URLs across `skills/artmark/SKILL.md`, `artmark context init`, `AGENTS.md`, `docs/design.md`, and `README.md`.
- Added atomic save-and-index via `artmark add <uri> --explicit --json-input <file|->` and `artmark recent --state` / `--provider` filters.
- Updated `rusqlite` and `url` dependencies to resolve `cargo audit` advisories.

## [0.2.3] - 2026-10-14

- Updated `README.md` release archive install examples.

## [0.2.2] - 2026-09-29

- Clarified agent artifact discovery triggers in `skills/artmark/SKILL.md` and improved empty search explanations in human output mode.

## [0.2.1] - 2026-09-29

- Added `artmark context init` to manage an artmark guidance block in project-local `AGENTS.md` files (originally targeted for `0.2.0`, shipped in `0.2.1`).
- Slimmed release archives to contain only the `artmark` binary, `skills/artmark/SKILL.md`, and `LICENSE`.

## [0.1.2] - 2026-09-28

- Rejected recognizable credential-bearing and signed URLs before canonicalization and persistence, documented in `docs/credential-validation.md`.

## [0.1.1] - 2026-09-27

- Added scalable symbol, wordmark, and monochrome logo variants and improved the README header contrast.

## [0.1.0] - 2026-09-27

First public release. The CLI and local SQLite FTS5 catalog are available on Linux x86-64, macOS Intel, macOS Apple Silicon, and Windows x86-64.

- Added a security reporting policy, issue templates, and an architecture guide.
- Documented early maturity, a complete install path, and macOS Gatekeeper behavior.
- Added dependency auditing and clean-install smoke tests to release CI.
- Added the supplied brand logo, README hero illustration, social graphic, and brand kit.
- Moved the remaining design and task notes into the design guide and GitHub issues.

## [0.0.4] - 2026-09-27

- Refined the README around durable artifact bookmarks and the agent workflow.

## Earlier development releases

Versions `0.0.1` through `0.0.3` established the Rust CLI, SQLite registry, local FTS5 search, cross-platform release archives, and agent skill. These were pre-public development builds.

[0.5.2]: https://github.com/sboorlagadda/artmark/releases/tag/v0.5.2
[0.5.1]: https://github.com/sboorlagadda/artmark/releases/tag/v0.5.1
[0.5.0]: https://github.com/sboorlagadda/artmark/releases/tag/v0.5.0
[0.4.0]: https://github.com/sboorlagadda/artmark/releases/tag/v0.4.0
[0.3.0]: https://github.com/sboorlagadda/artmark/releases/tag/v0.3.0
[0.2.3]: https://github.com/sboorlagadda/artmark/releases/tag/v0.2.3
[0.2.2]: https://github.com/sboorlagadda/artmark/releases/tag/v0.2.2
[0.2.1]: https://github.com/sboorlagadda/artmark/releases/tag/v0.2.1
[0.1.2]: https://github.com/sboorlagadda/artmark/releases/tag/v0.1.2
[0.1.1]: https://github.com/sboorlagadda/artmark/releases/tag/v0.1.1
[0.1.0]: https://github.com/sboorlagadda/artmark/releases/tag/v0.1.0
[0.0.4]: https://github.com/sboorlagadda/artmark/releases/tag/v0.0.4
