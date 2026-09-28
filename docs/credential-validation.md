# Credential validation for registration

Issue #5 is enforced by identity construction, which `Registry::add` calls before
starting a write transaction. The CLI and future registration interfaces share
this boundary. Validation is local: it makes no provider or network requests.

## What is inspected

- Inspect the original locator before URL parsing can remove dot segments or
  other text. The original URI and alias are both covered, even when the
  canonical identity drops queries or fragments for a known provider.
- Inspect HTTP(S) and file URLs, scheme-relative URLs, query values, fragments,
  and explicit URLs embedded in paths. Nested relative locators are inspected
  without resolving them against a base URL or discarding their original text.
- Inspect percent-encoded forms, including encoded names, delimiters and nested
  URLs. Also inspect URL interpretations with backslashes and embedded ASCII
  tabs/newlines normalized. Decoding is for validation only; it does not change
  the locator or canonical identity that is saved.
- Bare filesystem paths retain filesystem semantics. Their URL-shaped contents
  are inspected; a filename such as `report?token=notes` is not itself a URL.
  Safe UNC and double-slash paths remain supported.

## Rejection policy

Reject URL userinfo and recognized credential parameter names, case-insensitively
and allowing hyphen/underscore spelling variants:

- `token`, `access_token`, `refresh_token`, `id_token`, `auth_token`, `oauth_token`,
  `bearer_token`, `session_token`, `api_token`;
- `key`, `api_key`, `access_key`, `access_key_id`, `secret`, `secret_key`,
  `client_secret`, `password`, `passwd`, `pwd`, `auth`, `auth_key`, `authorization`,
  `credential`, `signature`, `sig`, `oauth_signature`, `jwt`, `session`,
  `session_id`, `saml_response`;
- AWS `X-Amz-*` and `AWSAccessKeyId`, Google `X-Goog-*`, `GoogleAccessId` and
  `resourcekey`, and Azure's `sig` signature parameter.

These names are reserved conservatively even if their value is empty or could
have a non-secret meaning. Other names ending in `token`, such as `page_token`,
are not automatically authentication parameters.

Reject `code` on recognizable authentication paths: a path containing an
`oauth` or `oauth2` segment, or ending in `authorize`, `callback`, `cb`, `login`,
`signin`, `auth`, or `sso`, before or after dot-segment normalization. Apply this
to fragment routes too. `state` alone does
not establish OAuth context: `/products?code=ABC&state=CA` remains registerable.
Plain anchors such as `#token` remain valid.

This is a deterministic check for recognizable credentials, not a classifier of
arbitrary secrets. Custom callback paths, unknown provider parameter names, and
opaque path/query values cannot be identified reliably without provider-specific
knowledge. Agents must continue to supply stable locators without credentials.

## Bounds and errors

Use an iterative queue with duplicate suppression, at most eight percent-decoding
passes, a 64 KiB input limit, a 1 MiB total inspection budget and 1,024 distinct
inspection tasks. Exceeding a limit rejects the input with a distinct, static
validation-limit error. No error includes locator text or parameter values.

## Completion criteria

- Test credential families across query, fragment, relative/absolute nested URL,
  path and encoding forms, including parser normalization. Pair these with safe
  query, anchor, nested URL and filesystem cases.
- Test `Registry::add` directly: rejection must leave the complete logical
  database unchanged, both empty and populated, including aliases and FTS.
- Test CLI rejection, exit status and redaction, and accepted query identity and
  deduplication. Retain regressions from PR #20's reviews.
- Run the repository's required local and CI checks before merge. Evaluate new
  review findings against this contract and a reproduction.
