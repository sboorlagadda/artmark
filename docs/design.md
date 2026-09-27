# Why artmark is a Yellow Pages for artifacts

artmark is a local catalog of pointers to durable artifacts. It answers “have I seen something relevant before, and where can I find it again?” It does not answer questions about an artifact's current contents without retrieving the original source.

## The Yellow Pages model

Each catalog entry keeps a canonical identity, a locator, source-provided facts, and a compact search card. Several URLs can refer to one artifact: for example, Google Docs `/edit` and `/view` links resolve to the same document identity. Registration can be quick when only a pointer is known. Indexing adds a useful card after an agent has inspected the source.

```text
user supplies artifact → agent registers pointer → agent reads source with its tools
                    → agent writes search card → artmark indexes local metadata

later question → artmark search → matching pointer → agent retrieves live source
```

The SQLite database is durable. Its FTS5 index is derived from catalog rows and can be rebuilt with `artmark reindex`. Search works offline and does not require embeddings. Optional semantic indexing can be considered after lexical retrieval has been exercised with real artifacts.

## Search text is for rediscovery

`search_text` is an agent-written retrieval description. It should contain the artifact's purpose, important topics, project names, decisions, relevant terms, and reasonable synonyms someone might use months later. A good card helps a vague query find the right pointer. It is a compact interpretation, not a transcript or a copy of the source.

Source facts such as title, provider object ID, revision, and source timestamp belong in source metadata. Agent interpretations such as summary, topics, and `search_text` belong in catalog metadata. This separation lets a card be refreshed without changing the artifact's identity or presenting an inference as a provider fact.

## Why source contents are not cached

Keeping source copies would turn artmark into a synchronization service with stale documents, access-control decisions, larger backups, and more sensitive data to protect. The authoritative artifact already lives in Google Drive, Figma, GitHub, a local filesystem, or another provider. artmark retains only enough metadata to locate it again. Catalog text can still be sensitive, so agents should avoid secrets and unnecessary source excerpts.

## Why provider access belongs to the agent harness

The agent already has provider-specific tools and the user's authorization to use them. It can inspect a source when building a card and retrieve the current version when answering a later question. artmark itself never authenticates to providers, stores provider credentials, or fetches source documents. This keeps the registry useful across providers without embedding every provider's API and permission model in the CLI.

The current implementation is a Rust CLI over local SQLite. An MCP interface is planned after the CLI path is proven end to end; it will expose the same registry boundary rather than becoming a provider client.
