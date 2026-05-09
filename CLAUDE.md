# CLAUDE.md

## Git Commit Requirements

When creating or amending commits:

- **Author and committer** must always be taken from the local git config (`git config user.name` / `git config user.email`). Never use Claude's own identity.
- **Never** add `Co-Authored-By:` trailers of any kind.
- **Never** include any Claude session URLs, session IDs, or links to claude.ai in commit messages or PR descriptions. Commit messages should only describe the code changes.
- **Never** include `🤖 Generated with [Claude Code](https://claude.com/claude-code)` or similar AI assistant references in PR descriptions or commit messages.
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

## Testing

- Run `make test` in `cryptpilot-verity/` to execute the full integration test suite
  (format, dump, verify, open/FUSE mount, tamper detection, close).
- Run `cargo test -p verity-fuse -p verity-core` for unit tests (requires `cd verity-core && python3 make_testfiles.py` first).

## FUSE Dependency

The `fuser` crate in workspace `Cargo.toml` uses `default-features = false` to avoid
linking `libfuse3.so.3`. This allows `cryptpilot-verity` to run on systems without
libfuse3 installed, as long as `/dev/fuse` and the FUSE kernel module are available.
The pure-Rust FUSE implementation communicates directly with the kernel via `/dev/fuse`.

## Pre-Push Checks

Before pushing, verify that no commits in the push carry a gpgsig header or a Claude committer identity:

```bash
for sha in $(git log --format="%H" origin/$(git rev-parse --abbrev-ref HEAD)..HEAD 2>/dev/null); do
    git cat-file -p "$sha" | grep -q "^gpgsig" && echo "ERROR: commit $sha has gpgsig — rewrite with filter-branch before pushing" && exit 1
    git log -1 --format="%ce" "$sha" | grep -qi "anthropic" && echo "ERROR: commit $sha has Claude committer — rewrite with filter-branch before pushing" && exit 1
done
echo "Pre-push checks passed"
```

If any commit fails, rewrite the committer with:

```bash
git filter-branch -f --env-filter '
  if [ "$GIT_COMMITTER_EMAIL" = "noreply@anthropic.com" ]; then
    export GIT_COMMITTER_NAME="$(git config user.name)"
    export GIT_COMMITTER_EMAIL="$(git config user.email)"
  fi
' <base-commit>..HEAD
```
