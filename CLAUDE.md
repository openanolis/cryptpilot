# CLAUDE.md

## Rust-Go Sync

When modifying the Rust `cryptpilot-verity`, `verity-core`, or `verity-fuse` code, always evaluate whether the corresponding Go library (`verity-go/`) needs the same change. If the change affects core algorithms (hash computation, merkle tree, descriptor format) or metadata structures (FlatBuffers schema, serialization), apply the equivalent change to the Go code in the same commit.

## Documentation Sync

When creating or modifying features, commands, or interfaces, always evaluate whether the corresponding documentation (README.md, CLAUDE.md, or other .md files under the project) needs to be updated. If the change introduces new commands, modifies existing behavior, adds configuration options, or changes usage examples, update the relevant documentation in the same commit.

## Excluded Paths

Never commit files under `docs/superpowers/` or `.claude/` to git. These are Claude session artifacts and should be kept local only. Add them to `.gitignore` if not already present.

## Git Commit Requirements

General commit norms (author/committer from local git config, no `Co-Authored-By:`, `Assisted-by:` as the only accepted AI attribution, no session URLs or AI footers, never commit plan/spec files or anything gitignored) are stated in the global agent instructions and apply here; this section lists only what differs in this project:

- **Always** add a `Signed-off-by:` trailer with the author's own identity, taken from the local git config. Use `git commit -s` (which appends it from the configured identity). This Developer Certificate of Origin trailer is required on every commit.
- **Always** use `--no-gpg-sign`: this repo's commits are unsigned by policy, overriding the global default of respecting `commit.gpgsign`. Consequently the pre-push rule is stricter here: no commit may carry a `gpgsig` header at all (not just an unverified one); strip it before pushing.

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

## Release (make bump-version)

Releases are driven by the global `release-version` skill; this section holds the project-specific facts that skill delegates to the project's docs.

- `make bump-version-{major,minor,patch}` regenerates version info in 6 places: `Cargo.toml`, `Cargo.lock`, `cryptpilot.spec` (`Version:` plus an auto-collected `%changelog` from commit subjects since the last tag), `debian/changelog`, and the three `APPLICATION/*/buildspec.yml` files. Version bump commits use `git commit -s --no-gpg-sign`.
- **Never manually edit version information**: not the `cryptpilot.spec` `Version:`/`Release:`/`%changelog`, not `debian/changelog`, not the `Cargo.toml` version. `make bump-version-*` owns all of it; a hand-written changelog entry would carry a wrong release number and duplicate the auto-collected commits. Edit the spec only for packaging logic (`BuildRequires`/`Requires`, `%build` flags).
- The repo is GitHub `openanolis/cryptpilot`; default branch is **`master`** (not `main`), and PRs target it. There is no `origin` remote; derive the push remote at runtime from the branch's tracking config.
- Pushing the `v<X.Y.Z>` tag triggers the release pipelines in `.github/workflows/` (vendored-source tarball, RPM/DEB packages, GitHub release with SLSA provenance). They build from the tag and publish nothing unreviewed, so the tag is pushed immediately after the PR opens, in parallel with PR CI. The tarball asset is named `cryptpilot-<X.Y.Z>-vendored-source.tar.gz`; discover the full asset list from the previous release.
- Shortly after a PR opens, the **`ostest-bot`** GitHub account comments with a table of openanolis images it will build and the line "如已确认，请回复 **/build** 进行构建". Reply `/build` as a PR comment to confirm. Wait for the bot's reply containing the 镜像制作中心 (cr.openanolis.cn) build URL and include it in the release report.
- Known non-blocking CI failure: `test-convert` on `alinux3` with `uki_stub_version=258` fails with `StartImage` "Load Error" at all RAM sizes (a known stub-version CI gap, not a regression). Accept it only after confirming the failure mode matches; `alinux4` + stub-258 rows passing is the gate. Any other failing check must be investigated before reporting.
