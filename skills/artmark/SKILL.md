---
name: artmark
description: Use the local Artmark CLI to remember and find durable artifacts across agent sessions. Apply when the user provides an artifact, asks to save or find one, or refers to one seen earlier.
---

# Artmark

Artmark is a local bookmark manager for agents.

It keeps durable catalog entries for pointers that would otherwise be lost with the current session: Google Docs, Figma files, GitHub repositories/issues/PRs, PDFs, web pages, local files, and similar durable objects.

Artmark stores enough information to **find an artifact again**. It does not store or replace the artifact itself.

The source provider or local file remains authoritative.

Use existing agent tools such as Google Drive, Figma, GitHub, browser, or filesystem tools to inspect and retrieve source contents. Use Artmark for durable identity, retrieval metadata, and search.

Artmark catalog text is for discovery. Do not treat it as evidence of the current contents of the source artifact.

## Check CLI availability

Before the first Artmark operation in a session, run:

```text
artmark --version
```

Do this at most once per session.

If `artmark` is unavailable, do not automatically download, install, upgrade, or replace executables as a side effect of using this skill.

Continue the user's main task when possible. If the user explicitly asked to save something, state clearly that it was not persisted because Artmark is unavailable.

If the user explicitly asks to install or upgrade Artmark, handle that as a separate setup task using the project's installation instructions.

Use `--json` for output that will be parsed. Pass user-provided URIs as arguments without shell interpolation.

## Register artifacts the user provides

Automatically quick-register durable artifact locators supplied directly by the user during the conversation.

Examples include Google Drive documents, Figma files, GitHub objects, Notion pages, stable web pages, PDFs, and durable local files.

Quick registration is intentionally cheap and does not require fetching the artifact:

```text
artmark add <uri> --json
```

If the user explicitly asks to save, remember, bookmark, or Artmark the artifact, mark that intent:

```text
artmark add <uri> --explicit --json
```

Keep the returned `art_...` ID for possible indexing.

Do not register temporary or credential-bearing locators such as signed URLs, authentication callbacks, temporary download URLs, session URLs, or paths to obviously temporary files. Prefer a stable canonical locator without secrets when one is available.

Do not register ordinary artifacts discovered independently through web searches, provider searches, repository exploration, or other agent research. If a discovered artifact appears especially valuable for future work, ask the user before retaining it. Do not interrupt ordinary research to ask about every result.

## Decide when to index

Registration and indexing are different operations.

A user-provided artifact that is merely mentioned can remain quick-registered.

For a user-provided artifact, or one the user approved for registration, index it when either of these is true:

1. The user explicitly asked to save or remember it and the source can be inspected.
2. The artifact is already being read or materially used for the current task.

Do not perform expensive provider retrieval solely to enrich every incidental link.

If the artifact's contents are already available from work being performed in the current task, reuse that information rather than fetching it again.

If provider access is unavailable, keep the quick registration. Do not invent catalog metadata.

## Inspect with the existing agent harness

When indexing is appropriate, use the best available tool for the source.

For example, use an available Drive tool for Google Drive artifacts, Figma tooling for Figma artifacts, GitHub tooling for GitHub objects, filesystem tools for local files, and web/browser tooling for ordinary web artifacts.

Artmark itself should not be expected to authenticate to those systems.

Inspect only enough of the source to understand what the artifact is and make it discoverable later.

Do not let Artmark indexing interfere with completing the user's primary task.

## Create a retrieval-oriented search card

Create a compact search card designed to help a future agent find the artifact when the user remembers it only vaguely.

The goal is **rediscovery**, not comprehensive summarization.

Capture the artifact's purpose, important topics, project or product names, decisions or questions, relevant technologies, meaningful entities, problems it helps solve, and terminology that the user might reasonably search for later.

Include useful synonyms or alternate terminology when they improve retrieval, but do not invent unsupported facts.

Keep source-provided facts in `source_metadata`. Keep agent-generated interpretation in `catalog`.

Send the card with:

```text
artmark index <id> --json-input - --json
```

using stdin, or provide an equivalent temporary JSON file.

The input shape is:

```json
{
  "source_metadata": {
    "title": "Source title",
    "revision": "optional source revision",
    "updated_at": "optional source timestamp"
  },
  "catalog": {
    "summary": "Short description of the artifact",
    "search_text": "Retrieval-oriented description containing concepts and likely future search terms",
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

`summary` and `search_text` are required.

Include retrieval fields only when known.

`search_text` should answer the question:

> What might the user vaguely remember about this artifact months from now?

For a small artifact, a short card is sufficient. For a large artifact, capture the concepts that distinguish it and are most likely to matter in future searches.

Do not copy substantial source sections into the card.

Do not store passwords, access tokens, cookies, API keys, signed URLs, authentication material, or unnecessary sensitive source data.

Avoid transient session context such as "the file we are working on right now." Prefer durable descriptions such as project names, topics, technologies, decisions, and artifact purpose.

## Find artifacts from previous sessions

When the user refers to an artifact they have seen before, or before asking them to resend a previously supplied artifact, search Artmark first:

```text
artmark search <query> --json
```

Use known provider, kind, topic, or other filters when they materially improve the search.

Inspect likely candidates with:

```text
artmark get <id> --json
```

If one result clearly matches, use its locator and retrieval hints to retrieve the live artifact through the appropriate provider tool.

If several results remain genuinely ambiguous, use their catalog cards to narrow the choice and ask the user only if necessary.

If Artmark has no plausible result, continue with other appropriate search methods or ask the user for the artifact when necessary.

## Use the live source

Once Artmark identifies an artifact, retrieve its current contents from the authoritative provider when the task depends on what the artifact actually says or contains.

For example:

```text
Artmark search
    ↓
find gdrive:doc:ABC
    ↓
Google Drive tool
    ↓
current Google Doc
```

Do not answer substantive questions about current source contents solely from `summary` or `search_text`.

The Artmark card may be stale. Its purpose is to locate the source.

If the source is inaccessible, explain that Artmark found the artifact's pointer but the current provider could not be accessed.

## Refresh opportunistically

When a user-provided artifact appears again, normal registration may update aliases and last-seen information while preserving its canonical identity.

If live retrieval shows that the artifact has changed enough that its existing catalog card would make future retrieval misleading or incomplete, regenerate the search card and run:

```text
artmark index <id> --json-input - --json
```

Do not refresh metadata merely because a timestamp or minor source detail changed.

Artmark does not require background synchronization.

## Failure behavior

Artmark is persistence infrastructure, not the user's primary task.

If registration or indexing fails, report the failure briefly when persistence matters and continue the user's main work whenever possible.

Never claim that an artifact was saved, indexed, updated, or retrieved unless the corresponding Artmark operation succeeded.
