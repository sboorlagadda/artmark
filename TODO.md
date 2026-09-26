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

## Later milestones

- [ ] Add a stdio MCP interface for register, index, search, and get.
- [ ] Ship an agent skill that applies the registration policy in `AGENTS.md`.
- [ ] Evaluate lexical retrieval before adding optional embeddings.
