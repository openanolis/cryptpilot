# CLAUDE.md

## Rust-Go Sync

When modifying the Rust `cryptpilot-verity`, `verity-core`, or `verity-fuse` code, always evaluate whether the corresponding Go library (`verity-go/`) needs the same change. If the change affects core algorithms (hash computation, merkle tree, descriptor format) or metadata structures (FlatBuffers schema, serialization), apply the equivalent change to the Go code in the same commit.

## Documentation Sync

When creating or modifying features, commands, or interfaces, always evaluate whether the corresponding documentation (README.md, CLAUDE.md, or other .md files under the project) needs to be updated. If the change introduces new commands, modifies existing behavior, adds configuration options, or changes usage examples, update the relevant documentation in the same commit.

## Excluded Paths

Never commit files under `docs/superpowers/` or `.claude/` to git. These are Claude session artifacts and should be kept local only. Add them to `.gitignore` if not already present.

## Git Commit Requirements

When creating or amending commits:

- **Author and committer** must always be taken from the local git config (`git config user.name` / `git config user.email`). Never use Claude's own identity.
- **Never** add `Co-Authored-By:` trailers of any kind.
- **Always** add a `Signed-off-by:` trailer with the author's own identity, taken from the local git config (`Signed-off-by: Your Name <you@example.com>`). Use `git commit -s` (which appends it from the configured identity) or add it by hand at the end of the commit message. This Developer Certificate of Origin trailer is required on every commit.
- **Always** add an `Assisted-by:` trailer to every commit message that Claude authored or co-authored. The trailer is the *only* accepted form of AI attribution. Format:

  ```
  Assisted-by: AGENT_NAME:MODEL_VERSION [TOOL1] [TOOL2] ...
  ```

  Where `AGENT_NAME` is the AI tool name (e.g. `Claude`), `MODEL_VERSION` is the specific model version used (e.g. `claude-opus-4-8`), and the optional bracketed `[TOOL]` entries are specialized analysis tools employed in producing the change (e.g. `coccinelle`, `sparse`, `smatch`, `clang-tidy`). Basic development tools (`git`, `gcc`, `make`, editors) must **not** be listed. Place the trailer as the last line(s) of the commit message body, separated by a blank line from the rest of the message. Example:

  ```
  Assisted-by: Claude:claude-opus-4-8 clang-tidy
  ```

  Only one `Assisted-by:` trailer per commit. If no specialized tool was used, omit the bracketed list entirely (`Assisted-by: Claude:claude-opus-4-8`).
- **Never** include any Claude session URLs, session IDs, or links to claude.ai in commit messages or PR descriptions. Commit messages should only describe the code changes.
- **Never** include "🤖 Generated with [Claude Code](https://claude.com/claude-code)" or similar AI attribution footers in PR descriptions or commit messages.
- **Always** use `--no-gpg-sign` to avoid GPG signing.
- **Never commit plan or spec files** (e.g. `docs/*-plan.md`, `docs/*-design.md`, `docs/*-spec.md`, or anything under `docs/superpowers/`). These should be gitignored (already covered by `.gitignore`) and kept local only.
- **Never commit any file that is already gitignored** — if a file matches `.gitignore`, it is intentionally local-only.
- **Never manually edit version information in `cryptpilot.spec`** — do not touch the `Version:` or `Release:` fields, and do not add version-stamped `%changelog` entries by hand. Version/release bumps across the whole repo (`Cargo.toml`, `Cargo.lock`, `APPLICATION/*/buildspec.yml`, debian build files, and the RPM spec `Version` + `%changelog`) are produced at a specific release stage by `make bump-version-{major,minor,patch}`. That target regenerates the spec changelog from commit subjects since the last tag, so a hand-written entry would both carry a wrong release number and duplicate the auto-collected commits. Edit the spec only for packaging logic (e.g. `BuildRequires`/`Requires`, `%build` flags); leave versioning to `make bump-version-*`.

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
- Run Go tests: `cd verity-go && go test -race -v ./...`

## Pre-Push / Pre-PR Checks

Before pushing or creating a pull request, always run the relevant checks and ensure they pass.

**Always run (regardless of what changed):**
```bash
cargo fmt --check
cargo build        # or `make clippy` / `cargo clippy` if lints are relevant
```

**When modifying verity-related code** (`cryptpilot-verity`, `verity-core`, `verity-fuse`, or `verity-go`):
```bash
# Rust verity tests
cargo test -p cryptpilot-verity -p verity-core -p verity-fuse

# Go verity tests
cd verity-go && go build ./... && go test -race -v ./...
```

For changes outside the verity subsystem, run the tests relevant to the affected packages only. If system dependencies are missing (e.g., `libcryptsetup`), the CI pipeline serves as the authoritative check, but `cargo fmt --check` must always pass locally.

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
