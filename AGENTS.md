# artmark contributor guide

Read `docs/design.md` and the relevant GitHub issues before changing product behavior. artmark is a local registry of pointers and retrieval-oriented catalog cards. It must not store source documents, fetch providers itself, or require provider credentials.

## Current delivery order

1. Finish the first public CLI release and verify clean installation.
2. Evaluate the shipped FTS5 search with real queries.
3. Define and add MCP after the CLI path works end to end.
4. Consider embeddings only after evaluating lexical retrieval; keep them optional.

Track work in GitHub issues and update or close them when tasks are completed. Make focused commits after a coherent piece of work passes its relevant checks. Create a branch and open a PR for each change to `main`; do not push changes directly to `main`.

The bundled agent workflow is in `skills/artmark/SKILL.md`; keep its CLI examples and registration policy aligned with this file and the shipped commands.

For every PR targeting `main` after the initial `0.0.1` release, advance the CLI version by one SemVer step in `Cargo.toml` and update `Cargo.lock`. Add exactly one matching `semver:patch`, `semver:minor`, or `semver:major` PR label. Follow the release guidance in `CONTRIBUTING.md`; a PR with multiple commits still gets one bump. Confirm that the `Release label and version`, `Format and lint`, `Dependency audit`, and all four build checks pass before merging.

## Agent registration policy

- Automatically register durable artifacts supplied by the user in prompts or during the session. Quick registration is sufficient when the artifact has not been inspected.
- Index a user-supplied artifact when the user explicitly asks to save it or when its contents were already read for the task.
- Ask at the end of the task which relevant durable artifacts discovered through searches, research, or links inside a user-supplied artifact the user wants registered. Do not interrupt ordinary research to ask about each result.
- Keep confirmed direct supersession relationships as brief, current wording in search-card summaries; do not create relation tags or accumulate version history there.
- Never treat catalog text as the current source; retrieve current details from the original provider.

## Implementation rules

- Keep source facts separate from agent-generated catalog data.
- Canonicalize known artifact URLs locally and deduplicate by canonical identity.
- Use prepared SQL statements, transactions for multi-table changes, and SQLite foreign keys.
- Search must work without embeddings or a network connection.
- Keep `--json` stdout valid JSON; send diagnostics to stderr.
- Tests should exercise behavior and persistence, especially canonicalization, deduplication, FTS search, and reindexing.
