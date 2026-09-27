# Artmark implementation plan

The first delivery is a local Rust CLI with SQLite and FTS5. MCP and embeddings follow only after the CLI has been exercised with real artifacts.

## 1. Project foundation

- [x] Write `AGENTS.md` with product boundaries and agent registration policy.
- [x] Initialize Git and commit the project plan.
- [x] Create the Rust package, help text, and minimal README.

## 2. Durable registry

- [x] Create a private local database directory and versioned SQLite schema.
- [x] Implement deterministic canonicalization for common provider URLs and local paths.
- [x] Implement `add`, `get`, `recent`, and `forget` with aliases and deduplication.
- [x] Support stable JSON output and documented exit codes.
- [x] Test persistence, canonical identity, aliases, and deletion.

## 3. Catalog and lexical search

- [x] Implement `index` from agent-supplied JSON without fetching source content.
- [x] Maintain an FTS5 index for title, summary, search text, topics, and entities.
- [x] Implement `search` with identity lookup, FTS ranking, filters, and compact result cards.
- [x] Implement `reindex`, `doctor`, and useful statistics.
- [x] Test indexing, search, updates, filters, and full FTS rebuild.

## 4. CLI release check

- [x] Run formatting, linting, and tests.
- [x] Exercise the CLI end to end against an isolated database.
- [x] Update README with installation and usage examples.
- [x] Commit each completed slice with a focused message.

## 5. Cross-platform releases

- [x] Set the initial CLI version to `0.0.1` in the manifest and lockfile.
- [x] Enforce one patch, minor, or major version bump per PR.
- [x] Build and test Linux, Windows, and both macOS architectures in GitHub Actions.
- [x] Package binaries with checksums and publish a release for each new version on `main`.
- [x] Document the version policy and release assets.
- [x] Validate scripts and workflows locally, then commit the release setup.
- [x] Confirm the first hosted build and `v0.0.1` release.
- [x] Check that each PR has exactly one release label matching its SemVer bump.
- [x] Rerun only the release-policy check on label edits and cancel superseded PR builds.
- [x] Teach the bundled skill to install or upgrade the CLI from a verified GitHub Release.
- [x] Add Apache-2.0 licensing, public-facing installation docs, contributor guidance, and GitHub About details.
- [x] Enable Dependabot vulnerability alerts for Rust dependencies.
- [x] Rewrite the README using the user-provided explanation of durable artifact bookmarks.

## Public launch follow-ups

- [ ] After changing visibility to public, create an active `main` ruleset that requires PRs and the release-policy, lint, and four platform build checks.
- [ ] After changing visibility to public, verify unauthenticated release downloads and remove the temporary private-access wording from the README and agent skill.

## Later milestones

- [ ] Add a stdio MCP interface for register, index, search, and get.
- [x] Ship an agent skill that applies the registration policy in `AGENTS.md`.
- [ ] Evaluate lexical retrieval before adding optional embeddings.
