# artmark

**Early / v0.1:** artmark is usable, but the CLI, JSON output, and SQLite schema are not yet stable compatibility contracts.

**A local bookmark manager for your agents.**

![artmark connects artifacts from one agent session to a local search card and a live source in a later session](assets/hero-image.png)

Your agent sees useful artifacts all day: Google Docs, Figma designs, GitHub issues, PDFs, web pages, local files, and more.

The problem is that the **session is temporary, but the artifacts are not**.

You may give an agent ten useful links while working on something, but weeks later a new session doesn't reliably know they existed. You end up searching old chats, finding the links again, and rebuilding the prompt.

**artmark makes them findable across agent sessions.**

It keeps a small, searchable local catalog of the artifacts you and your agents encounter, so later you can ask:

```text
Find the doc where we discussed SAML migration.

What was that Figma file for the onboarding redesign?

Find the GitHub issue about webhook retries.
```

and your agent has somewhere durable to look.

Think of artmark as **bookmarks for agents** — a local Yellow Pages for your artifacts.

## How it works

artmark is the agent's local catalog. Your agent's existing harness and tools access Google Drive, Figma, GitHub, the web, or your filesystem. artmark itself does not fetch from those systems.

When you provide a durable artifact, the agent can register its pointer immediately. If you ask it to save the artifact, or it reads the artifact during its work, the agent uses its provider tool to inspect the source and creates a compact search card:

```text
You provide an artifact
        │
        ▼
agent registers its pointer in artmark
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
artmark indexes the card in local SQLite
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

An uninspected artifact can stay registered as a pointer. A content-derived search card is indexed only after the agent has inspected the source with its available tools. artmark stores the **pointer, search card, and retrieval hints**, never a copy of the source artifact.

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

artmark is an artifact registry. The source system still owns the artifact.

## Use artmark with an agent

The repository includes an [artmark skill](skills/artmark/SKILL.md) that teaches agents how to use the registry. Copy `skills/artmark` from this checkout or a release archive to `~/.codex/skills/artmark` for Codex or `~/.claude/skills/artmark` for Claude Code. The skill checks whether the CLI is available when first used in a session. Installing or upgrading the CLI is a separate setup step that you request explicitly.

`artmark doctor` checks for the Codex skill and suggests either installing it or running `artmark context init` when it is missing. The context command adds a managed artmark section to the `AGENTS.md` file in the current directory. It can be run again to refresh that section and preserves instructions outside its markers.

The agent workflow is:

```text
1. Register pointers for durable artifacts you provide.
2. When asked to save one, or when already reading it, resolve the source with an existing tool.
3. Create a short search card from what it learned and index that card in artmark.
4. When a task requires identifying an artifact by description, search artmark before a provider search, even if the provider is named. Then retrieve the current source through the provider tool.
```

At the end of a task, the agent asks which relevant artifacts discovered through research or linked from something you supplied should be remembered. It does not register those links without your approval. See [AGENTS.md](AGENTS.md) for the full registration policy.

## Local by design

artmark is intentionally small.

The catalog lives in:

```text
$HOME/.artmark/artmark.db (macOS and Linux)
%USERPROFILE%\.artmark\artmark.db (Windows)
```

Search uses SQLite FTS5.

There is:

```text
no server
no account
no cloud database
no provider credentials stored by artmark
no copy of your source documents
no embeddings required
```

Search works locally and offline once an artifact has been indexed.

The agent's provider tools remain responsible for retrieving current source content.

## Install

Download the latest prebuilt binary and its matching `.sha256` file from [GitHub Releases](https://github.com/sboorlagadda/artmark/releases/latest). These instructions use the `v0.1.0` filenames; substitute the filenames shown on the latest release if it is newer.

Starting with `v0.2.1`, each archive contains the executable, the artmark agent skill at `skills/artmark/SKILL.md`, and `LICENSE`.

Pick the archive for your machine:

| Platform | Archive suffix |
| --- | --- |
| Linux x86-64 | `x86_64-unknown-linux-gnu.tar.gz` |
| macOS Intel | `x86_64-apple-darwin.tar.gz` |
| macOS Apple Silicon | `aarch64-apple-darwin.tar.gz` |
| Windows x86-64 | `x86_64-pc-windows-msvc.zip` |

From a terminal in the directory containing both downloaded files, use the commands for your system. On Linux x86-64:

```bash
archive=artmark-v0.1.0-x86_64-unknown-linux-gnu.tar.gz
sha256sum -c "$archive.sha256"
mkdir -p "$HOME/.local/bin"
tar -xzf "$archive" -C "$HOME/.local/bin" artmark
export PATH="$HOME/.local/bin:$PATH"
artmark --version
```

On macOS, choose **one** archive name: `artmark-v0.1.0-aarch64-apple-darwin.tar.gz` for Apple Silicon or `artmark-v0.1.0-x86_64-apple-darwin.tar.gz` for Intel. Then run:

```bash
archive=artmark-v0.1.0-aarch64-apple-darwin.tar.gz  # change to Intel filename if needed
shasum -a 256 -c "$archive.sha256"
mkdir -p "$HOME/.local/bin"
tar -xzf "$archive" -C "$HOME/.local/bin" artmark
export PATH="$HOME/.local/bin:$PATH"
artmark --version
```

Add `$HOME/.local/bin` to your shell's `PATH` configuration if you want `artmark` available in future terminal sessions.

**macOS Gatekeeper:** The release binaries are currently **unsigned and not notarized**. macOS may block the first run with “Apple cannot check it for malicious software” or an unidentified-developer warning. After verifying the release checksum and attempting to run `artmark --version`, open **System Settings → Privacy & Security**, select **Open Anyway** for `artmark`, and confirm **Open**. If macOS reports that the binary is damaged or will harm your computer, do not override that warning; download a fresh archive and verify its checksum again. See [Apple's Gatekeeper instructions](https://support.apple.com/en-us/102445).

On Windows x86-64, in PowerShell from the directory containing the downloads:

```powershell
$archive = 'artmark-v0.1.0-x86_64-pc-windows-msvc.zip'
$expected = ((Get-Content "$archive.sha256") -split '\s+')[0]
$actual = (Get-FileHash $archive -Algorithm SHA256).Hash
if ($actual -ne $expected) { throw 'Checksum mismatch' }
$bin = Join-Path $HOME '.local\bin'
New-Item -ItemType Directory -Force $bin | Out-Null
Expand-Archive $archive -DestinationPath $bin -Force
$env:PATH = "$bin;$env:PATH"
artmark --version
```

Add `%USERPROFILE%\.local\bin` to your user `PATH` to use `artmark` in future terminals.

Rust users can also build from source:

```bash
git clone https://github.com/sboorlagadda/artmark.git
cd artmark
cargo install --locked --path .
```

## Try it

These are the CLI commands an agent uses. To try them yourself, register an artifact:

```bash
artmark add \
  'https://docs.google.com/document/d/ABC/edit' \
  --explicit \
  --json
```

artmark returns an `art_...` ID.

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

`get` returns artmark's catalog metadata and retrieval hints.

Neither command fetches the source artifact.

Other commands include:

```text
recent
stats
doctor
context init
reindex
forget
```

`artmark context init` works without opening the artmark database. Run it from the project directory where the agent should receive the artmark guidance.

`reindex` rebuilds the local FTS5 index from artmark's catalog.

`forget` removes the artmark entry. It never deletes or modifies the underlying artifact.

Run:

```bash
artmark --help
```

for all commands, options, and filters.

## Data and privacy

artmark's default database is:

```text
$HOME/.artmark/artmark.db (macOS and Linux)
%USERPROFILE%\.artmark\artmark.db (Windows)
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

On Unix, artmark creates the directory with `0700` permissions and the database with `0600` permissions.

artmark does not store provider credentials.

It does not need Google, Figma, GitHub, or other provider authentication. Those responsibilities remain with the agent harness or tools that already have access.

Back up a stopped database or use SQLite's backup mechanism for a consistent copy.

See [SECURITY.md](SECURITY.md) for private vulnerability reporting and [docs/design.md](docs/design.md) for the registry design.

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

artmark is an early `v0.1` release. CLI behavior, JSON fields, and the local database schema may change before `1.0`.

The CLI and local FTS5 catalog are available now.

Planned next steps include:

```text
stdio MCP interface
optional local semantic search
additional agent integrations
```

The core architecture will remain the same:

> **artmark remembers enough to find the artifact. The source system still owns the artifact.**

## Contributing

Development setup, PR checks, and release versioning are documented in [CONTRIBUTING.md](CONTRIBUTING.md).

The [brand kit](docs/brand-kit.md) contains the logo and graphics for launch posts.

Issues, ideas, and pull requests are welcome.

artmark is licensed under [Apache-2.0](LICENSE).
