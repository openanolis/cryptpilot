---
name: release-version
description: Use when releasing a new cryptpilot version (cutting a major, minor, or patch release). The caller MUST state the version level — major, minor, or patch. Covers the full sequence: make bump-version-{major,minor,patch}, PR to gh/master, immediate tag push, ostest-bot /build trigger, and waiting for CI + GitHub release vendored tarball + openanolis image build.
---

# Release a cryptpilot Version

## Overview

Cut a cryptpilot release end-to-end: bump version → PR to `gh/master` → **immediately** push the tag (do NOT wait for CI) → reply `/build` to trigger the openanolis image build → wait for three conditions → report to the caller. The caller merges the PR; you never merge.

## Inputs (REQUIRED)

The caller must explicitly state the **version level**: `major`, `minor`, or `patch`. If they did not, stop and ask before doing anything. Examples of valid invocation:

- "发布 0.9.2（patch）" / "cut a patch release"
- "发 minor 版本" / "minor release"

Derive the target version from the Makefile (`make bump-version-<level>` prints `0.9.1 -> 0.9.2`). Do not hardcode the version number — read it from `Cargo.toml`. `make bump-version-*` only steps the patch/minor/major by +1; if the requested target is more than one step above the current `Cargo.toml` version, stop and confirm with the caller how to reach it (two sequential bumps, or a direct jump) — do not silently hand-edit versions.

## Remotes

This repo has **no `origin`** remote. Use:
- `gh` → GitHub (`openanolis/cryptpilot`)

Default branch is **`master`** (not `main`). PRs target `master`.

## The Flow

### 1. Bump the version

```bash
make bump-version-patch   # or -major / -minor
```

This regenerates version info across **6 places**: `Cargo.toml`, `Cargo.lock`, `cryptpilot.spec` (`Version:` + an auto-collected `%changelog` from commit subjects since the last tag), `debian/changelog`, and the three `APPLICATION/*/buildspec.yml` files (`cryptpilot-fde`, `cryptpilot-crypt`, `cryptpilot-verity`). **Never hand-edit the RPM spec `Version:`/`Release:`/`%changelog` or `debian/changelog`** — the Makefile owns them (see `CLAUDE.md`).

Verify all changed and `Cargo.lock` carries the new `cryptpilot` version:
```bash
git status --short
grep -n '^name = "cryptpilot"' -A1 Cargo.lock   # should show new version
```

This is a version-only change — `cargo fmt`/`make clippy`/`cargo build` add no signal here and may hit missing-system-dependency failures (`libcryptsetup`, `libdevmapper`). Skip them for the version bump itself; CI on the PR will validate. (`cargo fmt --check` is still required for any non-version code change, but a pure `make bump-version-*` diff touches no Rust source.)

### 2. Commit

Per `CLAUDE.md`: author/committer from local git config, `Signed-off-by:` trailer (DCO, via `git commit -s`), `Assisted-by:` trailer (the *only* accepted AI attribution), `--no-gpg-sign`, **never** `Co-Authored-By:` or any Claude session URL/identity.

```bash
git add Cargo.toml Cargo.lock cryptpilot.spec debian/changelog \
  APPLICATION/cryptpilot-fde/buildspec.yml \
  APPLICATION/cryptpilot-crypt/buildspec.yml \
  APPLICATION/cryptpilot-verity/buildspec.yml
git commit -s --no-gpg-sign -m "Bump <level> version to <X.Y.Z>" \
  -m "Regenerate version info via make bump-version-<level>." \
  -m "Assisted-by: Claude:glm-5.2"
```

Run the pre-push trailer check from `CLAUDE.md` (no `gpgsig`, no `Co-Authored-By`, no anthropic committer email):
```bash
for sha in $(git log --format="%H" gh/master..HEAD 2>/dev/null); do
    git cat-file -p "$sha" | grep -q "^gpgsig" && echo "ERROR: $sha has gpgsig" && exit 1
    git log -1 --format="%ce" "$sha" | grep -qi "anthropic" && echo "ERROR: $sha has Claude committer" && exit 1
done
echo "Pre-push checks passed"
```

### 3. Push branch + open PR (do NOT merge)

```bash
git push gh bump-version-<X.Y.Z>
gh pr create --repo openanolis/cryptpilot --base master --head bump-version-<X.Y.Z> \
  --title "Bump <level> version to <X.Y.Z>" --body "<summary>"
```

**Never merge the PR.** The caller merges. State this in your status report.

### 4. Push the tag — IMMEDIATELY, do not wait for CI

This is the key step agents get wrong. **Push the tag right after the PR is open**, not after CI goes green. Reason: the tag triggers a full second set of `push`-event workflows (the release pipeline, defined in `.github/workflows/build-rpm.yml`) that build all release assets — running them in parallel with the PR's `pull_request` CI saves ~15 min. Waiting for CI first serializes the two for no benefit.

```bash
git tag -a v<X.Y.Z> -m "Bump <level> version to <X.Y.Z>"
git push gh v<X.Y.Z>
```

The tag push triggers (separate from the PR checks), all on ref `v<X.Y.Z>`:
- `create-tarball` (produces `cryptpilot-<X.Y.Z>-vendored-source.tar.gz` via `make create-tarball`)
- `build` (RPM, x86_64 + aarch64)
- `test` and `test-convert` (the convert/boot matrix)
- `release` (uses `softprops/action-gh-release@v2`, SLSA provenance, uploads to GitHub Release `v<X.Y.Z>`)
- `update-release-notes` (regenerates the GitHub Release body from commit log since the previous tag)
- plus `build-deb.yml`, `build-docker.yml`, `clippy.yml`, `rust-fmt.yml`, `shellcheck.yml`, `test.yml` on the tag ref.

### 5. Trigger the openanolis image build (ostest-bot)

Shortly after the PR opens, the **`ostest-bot`** GitHub account comments, acknowledging the request and posting a table of images it will build (e.g. `cryptpilot-fde`, `cryptpilot-crypt`, `cryptpilot-verity` with tags `<X.Y.Z>、latest`) and the line: *"如已确认，请回复 ***/build*** 进行构建。"*

You must reply **`/build`** as a PR comment to confirm:
```bash
gh pr comment <PR-NUM> --repo openanolis/cryptpilot --body "/build"
```

Wait for the bot's confirmation reply, which looks like:
> @\<user\> ，您好，您的 PR 构建任务已提交，请前往 [镜像制作中心](https://cr.openanolis.cn/make_center/detail_info/<ID>?pr_type=github&tab_type=repo) 查看构建结果

That **镜像制作中心** URL is the openanolis build link you'll report to the caller. Save it.

### 6. Wait for three conditions

Wait until ALL three are satisfied before reporting:

1. **CI all passes** — both the PR `pull_request` checks AND the tag `push` workflows. Track via:
   ```bash
   gh pr checks <PR-NUM> --repo openanolis/cryptpilot --watch --interval 30   # PR-side
   gh run list --repo openanolis/cryptpilot --branch v<X.Y.Z> --limit 20      # tag-side
   ```
2. **GitHub release `v<X.Y.Z>` exists and contains the vendored source tarball** — asset named `cryptpilot-<X.Y.Z>-vendored-source.tar.gz` (produced by the `create-tarball`/`build` jobs; reference the `v0.9.1` release for the full asset list, which also includes `.src.rpm`, per-arch `.rpm`/`.deb` for `cryptpilot-{crypt,fde-guest,fde-host,verity}`, and SLSA provenance files):
   ```bash
   gh release view v<X.Y.Z> --repo openanolis/cryptpilot --json assets \
     --jq '.assets[].name'
   ```
3. **openanolis build triggered** — the ostest-bot replied with the 镜像制作中心 confirmation (step 5).

**Waiting strategy:** run `gh pr checks --watch` and a poll loop in the **background** (`run_in_background: true`); the harness re-invokes you on completion. Do not burn the prompt cache polling every few minutes. A single background poller that exits when all three conditions are met is ideal.

### 7. Report to the caller

Report these four items (the caller's checklist):
1. **PR link** — `https://github.com/openanolis/cryptpilot/pull/<NUM>`
2. **CI result** — all green (note any accepted known-infra failures, see below)
3. **Vendored tarball link** — `https://github.com/openanolis/cryptpilot/releases/download/v<X.Y.Z>/cryptpilot-<X.Y.Z>-vendored-source.tar.gz`
4. **openanolis 镜像制作中心 link** — the URL from the bot's confirmation comment

Remind the caller: **PR is not merged — for the caller to merge.**

## Known infra failures (do NOT treat as blockers)

Investigate each failure once to confirm the cause, then accept and note it in the report — do not let it stall the release.

| Check | Cause | Action |
|---|---|---|
| `test-convert` on `alinux3` + `uki_stub_version=258` (the `boot_matrix` rows) | `StartImage` "Load Error" on alinux3 at all RAM sizes with the pinned systemd stub 258; alinux4 boots fine. This is a known stub-version CI gap, not a regression. | Accept. alinux4 + stub-258 rows passing is the gate. Confirm the failure is the known `StartImage Load Error` pattern before accepting; if the failure mode differs, investigate. |

If **any other** check fails, investigate the root cause before reporting — do not silently call CI green. Per `CLAUDE.md` testing discipline: never hide a failing test; a flaky job gets rerun, a real failure gets fixed or surfaced.

## Common mistakes

| Mistake | Reality |
|---|---|
| "Wait for CI to pass, then push the tag" | No — push the tag **immediately** after the PR. The release pipeline must run in parallel with PR CI. |
| "Merge the PR once CI is green" | No — never merge. The caller merges. |
| "Hand-edit `cryptpilot.spec` Version/changelog or `debian/changelog`" | No — `make bump-version-*` owns all version info across the repo. Editing by hand duplicates/wrong-numbers the auto-generated changelog. |
| "Poll CI every 60s in the foreground" | Wastes prompt cache. Use a background `gh pr checks --watch` / poller; the harness notifies you on exit. |
| "Use `origin` remote" | No `origin` here. Push to `gh` (GitHub). |
| "Default branch is `main`" | It's `master`. |
| "Add `Co-Authored-By:` or a GPG signature" | Forbidden by `CLAUDE.md`. Use `git commit -s --no-gpg-sign` + `Assisted-by:` trailer only. |

## Red flags — STOP

- Pushing the tag only after CI went green → you serialized the release pipeline for nothing. Push immediately.
- Merging the PR → the caller merges, not you.
- Reporting "CI green" while a non-known check is failing → investigate first.
- Committing with `Co-Authored-By:` or a GPG signature → forbidden by `CLAUDE.md`; rewrite before pushing.
- Hand-editing `cryptpilot.spec` `Version:`/`Release:`/`%changelog` → owned by `make bump-version-*`.
