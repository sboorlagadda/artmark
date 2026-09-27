## Release version

Every PR to `main` increments the Artmark CLI version once. Update `version` in `Cargo.toml`, then run `cargo check` to update `Cargo.lock`.

- **Patch** (`0.0.1` → `0.0.2`): fixes, documentation, and internal changes.
- **Minor** (`0.0.1` → `0.1.0`): a new feature or a breaking change during initial `0.x` development.
- **Major** (`0.0.1` → `1.0.0`): the first stable public contract; after `1.0`, use it for breaking changes.

The `Version bump` check verifies the exact next version against the PR base. Multiple commits in one PR share one bump.
