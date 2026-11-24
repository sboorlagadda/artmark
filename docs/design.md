# Why artmark is a Yellow Pages for artifacts

artmark is a local catalog of pointers to durable artifacts. It answers “have I seen something relevant before, and where can I find it again?” It does not answer questions about an artifact's current contents without retrieving the original source.

## The Yellow Pages model

Each catalog entry keeps a canonical identity, a locator, source-provided facts, and a compact search card. Several URLs can refer to one artifact: for example, Google Docs `/edit` and `/view` links resolve to the same document identity. Saving an artifact inspects the source with the agent's tools and writes the pointer and search card together by default, so saved entries are immediately discoverable. Pointer-only registration remains available as a fallback when a source cannot be inspected yet.

```text
user asks to save or approves at task end → agent reads source with its tools
                    → agent writes search card → artmark saves pointer and indexes card

later question → artmark search → matching pointer → agent retrieves live source
```

The SQLite database is durable. Its FTS5 index (`tokenize = 'porter unicode61'`) is derived from catalog rows and can be rebuilt with `artmark reindex`. Search works offline and does not require embeddings. Optional semantic indexing can be considered only if real usage shows recall failures beyond the lexical evaluation in [search-evaluation.md](search-evaluation.md).

## Schema migrations and local usage tracking

Database schema changes are versioned with SQLite `PRAGMA user_version` and applied in order inside immediate transactions that verify foreign keys and FTS5 integrity before committing. Before upgrading an existing on-disk database to a newer schema version, artmark creates a consistent standalone backup beside the database (`<db>.v<N>.bak`, using SQLite `VACUUM INTO` so WAL changes are included) and refuses to open databases from a newer unsupported schema version. If a migration fails, the transaction rolls back automatically and the pre-migration backup remains available for manual recovery.

To help users see whether saved artifacts are actually rediscovered across sessions, `artmark get` increments local `access_count` and updates `last_accessed_at` in SQLite (surfaced in `artmark stats`). Search candidates, internal reads, and `artmark get --no-track` do not increment lookup counts. All counts remain strictly local with no telemetry.

## Search text is for rediscovery

`search_text` is an agent-written retrieval description. It should contain the artifact's purpose, important topics, project names, decisions, relevant terms, and reasonable synonyms someone might use months later. A good card helps a vague query find the right pointer. It is a compact interpretation, not a transcript or a copy of the source.

Source facts such as title, provider object ID, revision, and source timestamp belong in source metadata. Agent interpretations such as summary, topics, and `search_text` belong in catalog metadata. This separation lets a card be refreshed without changing the artifact's identity or presenting an inference as a provider fact.

## Why source contents are not cached

Keeping source copies would turn artmark into a synchronization service with stale documents, access-control decisions, larger backups, and more sensitive data to protect. The authoritative artifact already lives in Google Drive, Figma, GitHub, a local filesystem, or another provider. artmark retains only enough metadata to locate it again. Catalog text can still be sensitive, so agents should avoid secrets and unnecessary source excerpts.

## Why provider access belongs to the agent harness

The agent already has provider-specific tools and the user's authorization to use them. It can inspect a source when building a card and retrieve the current version when answering a later question. artmark itself never authenticates to providers, stores provider credentials, or fetches source documents. This keeps the registry useful across providers without embedding every provider's API and permission model in the CLI.

The current implementation is a Rust CLI over local SQLite. An MCP interface is planned after the CLI path is proven end to end; it will expose the same registry boundary rather than becoming a provider client.

Registration checks locators for recognizable credentials before saving them.
The [credential validation policy](credential-validation.md) defines supported
forms, conservative rejection rules, processing limits, and acceptance tests.
