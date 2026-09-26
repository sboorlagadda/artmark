# Artmark

Artmark is a local catalog that helps agents rediscover artifacts. It stores identities, retrieval hints, and compact search cards. The original document, design, issue, or file stays in its source system.

This first release is a Rust CLI backed by SQLite and FTS5. It works offline and does not fetch artifacts or require provider credentials. MCP and semantic embeddings are planned later.

## Build

```bash
cargo install --path .
```

The default database is `~/.artmark/artmark.db`. Artmark creates the directory with `0700` permissions and the database with `0600` permissions on Unix. Use `--database PATH` or `ARTMARK_DB` to select another database.

## Use

Register a user-provided artifact without fetching it:

```bash
artmark add 'https://docs.google.com/document/d/ABC/edit' --explicit --json
```

The response contains an `art_...` ID. An agent that has inspected the source can add a search card:

```json
{
  "source_metadata": {
    "title": "Enterprise Authentication Architecture",
    "revision": "3",
    "updated_at": "2026-09-25T18:15:00Z"
  },
  "catalog": {
    "summary": "Architecture for enterprise identity migration.",
    "search_text": "Covers SAML migration, SCIM provisioning, Okta integration, identity provider discovery, and legacy tenant rollout. Useful for enterprise SSO and customer authentication migration questions.",
    "topics": ["SAML", "SCIM", "enterprise SSO"],
    "entities": ["Project Phoenix"]
  },
  "retrieval": {
    "provider": "google_drive",
    "external_id": "ABC"
  }
}
```

Save that JSON as `card.json`, then run:

```bash
artmark index art_... --json-input card.json
artmark search 'SAML migration' --provider google_drive --json
artmark get art_... --json
```

`--json-input -` reads the card from stdin. `search` returns compact cards. `get` returns the full local catalog entry, including retrieval hints, but neither command fetches the upstream artifact.

Other commands:

```bash
artmark recent --limit 20
artmark stats --json
artmark doctor --json
artmark reindex
artmark forget art_...
```

`reindex` rebuilds FTS5 entirely from the local catalog. `forget` removes only the Artmark entry. Google Docs edit and view URLs, for example, map to one canonical artifact with both URLs kept as aliases. Artmark also recognizes Google Sheets and Slides, Drive files, Figma files, GitHub repositories/issues/PRs, Notion pages, Slack threads, local paths, and generic web URLs.

## Agent behavior

The bundled Codex skill is at [skills/artmark/SKILL.md](skills/artmark/SKILL.md). To make it available outside this repository, copy the `skills/artmark` directory to `~/.codex/skills/artmark` and make sure the `artmark` CLI is on `PATH`. When working from this checkout, the skill can use `cargo run --manifest-path <repo-root>/Cargo.toml --` before the binary is installed.

The policy in [AGENTS.md](AGENTS.md) is to automatically quick-register durable artifacts supplied by the user. Explicit saves and user artifacts already read for a task should be indexed. An agent should ask before registering artifacts it found independently through searches. The skill applies this policy through the CLI.

Search cards should describe an artifact for later retrieval. They should never copy whole source documents or include credentials. The CLI rejects common content and credential fields in source metadata and retrieval hints and limits catalog size.

## CLI contract

`--json` keeps stdout valid JSON and sends diagnostics to stderr. Exit codes are `0` for success, `2` for invalid command arguments, `3` for a missing artifact, `4` for database or index failure, and `6` for invalid catalog input. FTS5 search works without embeddings. Search scores rank results locally; they are not probabilities.

The SQLite database is the durable record. SQLite may create temporary WAL sidecar files while Artmark is running. Use SQLite's backup mechanism or a stopped database when making a consistent copy.
