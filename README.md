# Artmark

**Your agent remembers where to find things.**

Artmark is a small, local catalog for links to documents, designs, issues, files, and other durable artifacts. Agents can register a pointer now and find it later with SQLite FTS5 search. Artmark keeps identities and compact search cards; the original content stays in its source system.

- **Local and offline:** search works without a server, provider credentials, or embeddings.
- **Easy to revisit:** canonical URLs and aliases point to one artifact, even when the link changes shape.
- **Agent friendly:** commands can return JSON, and the bundled skill handles registration and indexing.

The CLI is available today. A stdio MCP interface and optional semantic search are planned later.

## Install

Download the latest prebuilt binary from [GitHub Releases](https://github.com/sboorlagadda/artmark/releases/latest). Pick the archive for your machine:

| Platform | Archive suffix |
| --- | --- |
| Linux x86-64 | `x86_64-unknown-linux-gnu.tar.gz` |
| macOS Intel | `x86_64-apple-darwin.tar.gz` |
| macOS Apple Silicon | `aarch64-apple-darwin.tar.gz` |
| Windows x86-64 | `x86_64-pc-windows-msvc.zip` |

Each archive has a matching `.sha256` file. Verify it before extracting, then put `artmark` (or `artmark.exe`) on your `PATH` and run `artmark --version`. On Linux, run `sha256sum -c ARCHIVE.sha256`; on macOS, run `shasum -a 256 -c ARCHIVE.sha256`. On Windows, compare `(Get-FileHash ARCHIVE -Algorithm SHA256).Hash` with the digest in the checksum file. Replace `ARCHIVE` with the downloaded archive name. Release downloads require repository access until the repository becomes public.

Rust users can also build from this checkout with `cargo install --path .`.

## Try it

Register a link without fetching its contents:

```bash
artmark add 'https://docs.google.com/document/d/ABC/edit' --explicit --json
```

Artmark returns an `art_...` ID. After an agent reads the document from its original provider, it can save a short search card in `card.json`:

```json
{
  "source_metadata": { "title": "Enterprise Authentication Architecture" },
  "catalog": {
    "summary": "Architecture for enterprise identity migration.",
    "search_text": "SAML migration, SCIM provisioning, Okta integration, and legacy tenant rollout.",
    "topics": ["SAML", "SCIM", "enterprise SSO"]
  },
  "retrieval": { "provider": "google_drive", "external_id": "ABC" }
}
```

```bash
artmark index art_... --json-input card.json
artmark search 'SAML migration' --json
artmark get art_... --json
```

`search` returns compact cards. `get` returns the local catalog entry and retrieval hints. Neither command fetches the source document.

Other useful commands: `recent`, `stats`, `doctor`, `reindex`, and `forget`. `reindex` rebuilds FTS5 from the local catalog. `forget` removes the Artmark entry and leaves the original artifact untouched. Run `artmark --help` for options and filters.

## Use it with an agent

The bundled [Artmark skill](skills/artmark/SKILL.md) tells a Codex agent when to register user-provided artifacts, how to write a compact search card, and when to ask before saving something it discovered itself. Copy `skills/artmark` from this checkout or a release archive to `~/.codex/skills/artmark`. On first use each session, the skill checks the latest release and installs or upgrades the CLI when needed.

Artmark does not copy source documents or store provider credentials. The agent retrieves current content through the original provider when it needs to use an artifact. See [AGENTS.md](AGENTS.md) for the registration policy.

## Data and behavior

The default database is `~/.artmark/artmark.db`; use `--database PATH` or `ARTMARK_DB` to choose another path. On Unix, Artmark creates the directory with `0700` permissions and the database with `0600` permissions. Back up a stopped database or use SQLite's backup mechanism for a consistent copy.

`--json` keeps stdout valid JSON and sends diagnostics to stderr. Exit codes are `0` for success, `2` for invalid arguments, `3` for a missing artifact, `4` for database or index failure, and `6` for invalid catalog input. Search scores are local rankings, not probabilities.

## Contribute

Development setup, PR checks, and release versioning are in [CONTRIBUTING.md](CONTRIBUTING.md). Artmark is licensed under [Apache-2.0](LICENSE).
