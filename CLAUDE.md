# CLAUDE.md

## Git Commit Requirements

When creating or amending commits:

- **Author and committer** must always be taken from the local git config (`git config user.name` / `git config user.email`). Never use Claude's own identity.
- **Never** add `Co-Authored-By:` trailers of any kind.
- **Never** include any Claude session URLs, session IDs, or links to claude.ai in commit messages or PR descriptions. Commit messages should only describe the code changes.
- **Always** use `--no-gpg-sign` to avoid GPG signing.

## Pre-Commit Checks

Before creating any commit, always run and ensure the following pass:

```bash
make clippy        # Rust lints (wraps cargo clippy)
cargo fmt --check  # Formatting check
cargo build        # Compilation
```

Fix any errors or warnings reported before proceeding with the commit.

> **Note**: `make clippy` and `cargo build` require system libraries (`libcryptsetup`,
> `libdevmapper`, etc.) that may not be present in all dev environments. If they fail
> solely due to missing system dependencies (not code errors), the CI pipeline will
> serve as the authoritative check. `cargo fmt --check` must always pass locally.
