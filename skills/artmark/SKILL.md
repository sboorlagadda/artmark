---
name: artmark
description: Use the local Artmark CLI to remember and rediscover durable artifacts that users provide across sessions. Apply when a user shares an artifact, asks to save or find one, or refers to an artifact seen earlier.
---

# Artmark

Artmark stores catalog entries and retrieval hints. The source provider or local file remains authoritative. Use the CLI for identity and search; use existing provider tools to inspect or retrieve source contents.

## Install or update the CLI first

Before the first Artmark operation in a session, check any CLI on `PATH` with `artmark --version` and query the latest published release at `https://api.github.com/repos/sboorlagadda/artmark/releases/latest`. Public releases can be queried without authentication. If the repository is still private, use authenticated `gh release view -R sboorlagadda/artmark --json tagName --jq .tagName` instead. Compare the numeric SemVer values, ignoring the release tag's `v` prefix. If the CLI is missing or older, install the matching release binary; do not downgrade a newer installed CLI. Once checked, use that CLI for the rest of the session without repeating the release query.

Choose the archive for the host: Linux x86-64 `x86_64-unknown-linux-gnu.tar.gz`, macOS Intel `x86_64-apple-darwin.tar.gz`, macOS Apple Silicon `aarch64-apple-darwin.tar.gz`, or Windows x86-64 `x86_64-pc-windows-msvc.zip`. The full asset name is `artmark-<tag>-<target><extension>`. Download that archive and its `.sha256` asset from the release's `browser_download_url` values. While private, use authenticated `gh release download <tag> -R sboorlagadda/artmark -p <asset> -p <asset>.sha256 -D <temporary-directory>`. Verify the downloaded archive's SHA-256 against the checksum file before extraction. Install only the `artmark` or `artmark.exe` binary into a user-writable directory on `PATH`, then check `artmark --version` again. Never put the downloaded README or skill over an existing installation as part of a CLI update.

If GitHub access, a supported asset, checksum verification, or installation fails, explain the specific problem and do not claim that Artmark is installed or current. An already installed CLI may still be used when the release cannot be checked; state that its freshness is unknown. In this repository, `cargo run --manifest-path <repo-root>/Cargo.toml --` is a fallback when the release binary cannot be installed. Use `--json` for results you will parse. Pass user-provided URIs as arguments without shell interpolation.

## Register user-provided artifacts

Automatically quick-register durable URLs and local paths that the user provides in a prompt or during the session. Use `--explicit` on the initial call when the user asks to save or remember the artifact:

```text
artmark add <uri> --json
```

This call does not fetch the artifact. Keep the returned ID for later indexing. For an explicit save, inspect the source using the best available provider tool. If the artifact is already being read for the current task, use what was learned to index it. If provider access is unavailable, keep the quick registration and say that the catalog card is incomplete. Do not register an expiring or signed URL containing credentials; use a stable locator without secrets if one is available.

Do not register artifacts found independently through web searches, provider searches, or other exploration without asking the user first. Ask only when a discovered artifact seems worth retaining; ordinary search results do not need a registration question.

## Index after inspection

Create a compact search card aimed at finding the artifact later from vague terms. Include its purpose, topics, project or product names, decisions, relevant technologies, alternate search terms, and problems it helps solve. Add reasonable synonyms without inventing facts. Keep source facts in `source_metadata` and interpretation in `catalog`.

Send JSON to `artmark index <id> --json-input - --json` through stdin, or use a temporary JSON file. The input shape is:

```json
{
  "source_metadata": {
    "title": "Source title",
    "revision": "optional source revision",
    "updated_at": "optional source timestamp"
  },
  "catalog": {
    "summary": "Short description of the artifact",
    "search_text": "Retrieval-oriented description with likely future search terms",
    "topics": ["topic"],
    "entities": ["project or notable entity"],
    "tags": []
  },
  "retrieval": {
    "provider": "provider name",
    "external_id": "provider object ID"
  }
}
```

`summary` and `search_text` are required. Include retrieval fields only when known. Do not paste source sections into the card or store secrets, access tokens, cookies, signed URLs, or unnecessary sensitive details. For a small artifact, a short card is enough; for a large document, capture the concepts most likely to matter in future searches.

## Find and use an artifact

Before asking the user to resend a previously supplied artifact, search the catalog:

```text
artmark search <query> --json
artmark get <id> --json
```

Inspect likely cards, then retrieve the chosen artifact from its live provider or local path. Catalog text may be stale and is never evidence of current source contents. If the source is inaccessible, explain that Artmark found its pointer but the provider did not allow retrieval.

When a user-provided artifact is encountered again, Artmark deduplicates its canonical identity and records the new URI as an alias. If live retrieval reveals meaningful changes, replace its search card with `artmark index`.
