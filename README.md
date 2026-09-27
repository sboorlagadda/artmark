# Artmark

**A local bookmark manager for your agents.**

Your agent sees useful artifacts all day: Google Docs, Figma designs, GitHub issues, PDFs, web pages, local files, and more.

The problem is that the **session is temporary, but the artifacts are not**.

You may give an agent ten useful links while working on something, but weeks later a new session doesn't reliably know they existed. You end up searching old chats, finding the links again, and rebuilding the prompt.

**Artmark makes them findable across agent sessions.**

It keeps a small, searchable local catalog of the artifacts you and your agents encounter, so later you can ask:

```text
Find the doc where we discussed SAML migration.

What was that Figma file for the onboarding redesign?

Find the GitHub issue about webhook retries.
```

and your agent has somewhere durable to look.

Think of Artmark as **bookmarks for agents** — a local Yellow Pages for your artifacts.

## How it works

Artmark is the agent's local catalog. Your agent's existing harness and tools access Google Drive, Figma, GitHub, the web, or your filesystem. Artmark itself does not fetch from those systems.

When you provide a durable artifact, the agent can register its pointer immediately. If you ask it to save the artifact, or it reads the artifact during its work, the agent uses its provider tool to inspect the source and creates a compact search card:

```text
You provide an artifact
        │
        ▼
agent registers its pointer in Artmark
        │
        ▼
agent resolves and reads the source
        │
        │ using its harness tools:
        │ Drive / Figma / GitHub / browser / filesystem
        ▼
agent creates a compact search card
        │
        ▼
Artmark indexes the card in local SQLite
```

Later:

```text
"Find the architecture doc about SAML migration"
        │
        ▼
  artmark search
        │
        ▼
 Enterprise Authentication Architecture
 Google Doc · gdrive:doc:ABC
        │
        ▼
agent retrieves the current document
through its Google Drive tool
```

An uninspected artifact can stay registered as a pointer. A content-derived search card is indexed only after the agent has inspected the source with its available tools. Artmark stores the **pointer, search card, and retrieval hints**, never a copy of the source artifact.

The original Google Doc stays in Google Drive; the Figma file stays in Figma; the GitHub issue stays in GitHub.

When an agent needs current details, it goes back to the authoritative source using those tools.

## More than a URL

A traditional bookmark might remember:

```text
https://docs.google.com/document/d/ABC/edit
```

That is not enough for an agent to find the document six months later when you ask:

```text
Where was the doc where we talked about migrating
old enterprise customers to SAML?
```

After the agent's tool has read the source, the agent can store a compact **search card**:

```json
{
  "source_metadata": {
    "title": "Enterprise Authentication Architecture"
  },
  "catalog": {
    "summary": "Architecture for enterprise identity migration.",
    "search_text": "SAML migration, SCIM provisioning, Okta integration, legacy tenant rollout, enterprise SSO, identity federation.",
    "topics": [
      "SAML",
      "SCIM",
      "enterprise SSO"
    ]
  },
  "retrieval": {
    "provider": "google_drive",
    "external_id": "ABC"
  }
}
```

`search_text` is a retrieval-oriented description, not a cached copy of the document. It helps answer:

> **What might I vaguely remember about this artifact later?**

The search card gives a later session enough context to find the artifact again:

```text
I've seen this artifact before.
This is what it is about.
This is where it lives.
This is how to retrieve it again.
```

Artmark is an artifact registry. The source system still owns the artifact.

## Use Artmark with an agent

The repository includes an [Artmark skill](skills/artmark/SKILL.md) that teaches a Codex agent how to use the registry. Copy `skills/artmark` from this checkout or a release archive to `~/.codex/skills/artmark`. The skill checks whether the CLI is available when first used in a session. Installing or upgrading the CLI is a separate setup step that you request explicitly.

The agent workflow is:

```text
1. Register pointers for durable artifacts you provide.
2. When asked to save one, or when already reading it, resolve the source with an existing tool.
3. Create a short search card from what it learned and index that card in Artmark.
4. In a later session, search Artmark, then retrieve the current source through the provider tool.
```

The agent asks before registering artifacts it discovers independently through searches. See [AGENTS.md](AGENTS.md) for the full registration policy.

## Local by design

Artmark is intentionally small.

The catalog lives in:

```text
~/.artmark/artmark.db
```

Search uses SQLite FTS5.

There is:

```text
no server
no account
no cloud database
no provider credentials stored by Artmark
no copy of your source documents
no embeddings required
```

Search works locally and offline once an artifact has been indexed.

The agent's provider tools remain responsible for retrieving current source content.

## Install

Download the latest prebuilt binary from [GitHub Releases](https://github.com/sboorlagadda/artmark/releases/latest).

Pick the archive for your machine:

| Platform | Archive suffix |
| --- | --- |
| Linux x86-64 | `x86_64-unknown-linux-gnu.tar.gz` |
| macOS Intel | `x86_64-apple-darwin.tar.gz` |
| macOS Apple Silicon | `aarch64-apple-darwin.tar.gz` |
| Windows x86-64 | `x86_64-pc-windows-msvc.zip` |

Each archive has a matching `.sha256` file.

Verify the archive before extracting it. Then put `artmark` (or `artmark.exe`) on your `PATH` and run:

```bash
artmark --version
```

On Linux:

```bash
sha256sum -c ARCHIVE.sha256
```

On macOS:

```bash
shasum -a 256 -c ARCHIVE.sha256
```

On Windows, compare:

```powershell
(Get-FileHash ARCHIVE -Algorithm SHA256).Hash
```

with the digest in the checksum file.

Rust users can also build from source:

```bash
cargo install --path .
```

## Try it

These are the CLI commands an agent uses. To try them yourself, register an artifact:

```bash
artmark add \
  'https://docs.google.com/document/d/ABC/edit' \
  --explicit \
  --json
```

Artmark returns an `art_...` ID.

After inspecting the source, save the example search card above as `card.json` and index it:

```bash
artmark index art_... --json-input card.json
```

Find it later:

```bash
artmark search 'enterprise SSO migration' --json
```

Inspect the catalog entry:

```bash
artmark get art_... --json
```

`search` returns compact artifact cards.

`get` returns Artmark's catalog metadata and retrieval hints.

Neither command fetches the source artifact.

Other commands include:

```text
recent
stats
doctor
reindex
forget
```

`reindex` rebuilds the local FTS5 index from Artmark's catalog.

`forget` removes the Artmark entry. It never deletes or modifies the underlying artifact.

Run:

```bash
artmark --help
```

for all commands, options, and filters.

## Data and privacy

Artmark's default database is:

```text
~/.artmark/artmark.db
```

Use:

```text
--database PATH
```

or:

```text
ARTMARK_DB
```

to choose another database.

On Unix, Artmark creates the directory with `0700` permissions and the database with `0600` permissions.

Artmark does not store provider credentials.

It does not need Google, Figma, GitHub, or other provider authentication. Those responsibilities remain with the agent harness or tools that already have access.

Back up a stopped database or use SQLite's backup mechanism for a consistent copy.

## Agent-friendly CLI behavior

Commands support `--json`.

When JSON output is requested:

```text
stdout = machine-readable JSON
stderr = diagnostics
```

Exit codes are:

| Code | Meaning |
| ---: | --- |
| `0` | Success |
| `2` | Invalid arguments |
| `3` | Artifact not found |
| `4` | Database or index failure |
| `6` | Invalid catalog input |

Search scores are local ranking scores, not probabilities.

## Status

Artmark is early.

The CLI and local FTS5 catalog are available now.

Planned next steps include:

```text
stdio MCP interface
optional local semantic search
additional agent integrations
```

The core architecture will remain the same:

> **Artmark remembers enough to find the artifact. The source system still owns the artifact.**

## Contributing

Development setup, PR checks, and release versioning are documented in [CONTRIBUTING.md](CONTRIBUTING.md).

Issues, ideas, and pull requests are welcome.

Artmark is licensed under [Apache-2.0](LICENSE).
