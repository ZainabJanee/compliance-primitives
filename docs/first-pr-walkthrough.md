# First PR Walkthrough

This document walks through one real `complexity: trivial` issue from first
read to merged PR, step by step. Use it as a template when you're picking
up your first issue in this repo.

The example: **[#302 — Add a CI check enforcing a CHANGELOG.md entry
alongside any workspace version bump](https://github.com/stellar-compliance-kit/compliance-primitives/issues/302)**,
resolved in [PR #305](https://github.com/stellar-compliance-kit/compliance-primitives/pull/305).

---

## Step 0 — Read CONTRIBUTING.md first

Before picking up any issue, skim [CONTRIBUTING.md](../CONTRIBUTING.md) end
to end. It covers the fork → branch → PR flow, complexity labels, code
style, and the `make test` / `make lint` gates CI enforces. The rest of this
walkthrough assumes you've done that.

---

## Step 1 — Read the issue carefully

Issue [#302](https://github.com/stellar-compliance-kit/compliance-primitives/issues/302):

> **Scope**: Prevent a PR from bumping `Cargo.toml`'s
> `workspace.package.version` without also adding a corresponding
> `CHANGELOG.md` entry, so the changelog doesn't silently fall behind actual
> releases.
>
> **Acceptance criteria**:
> - A CI check compares the diff of `Cargo.toml`'s version field against
>   `CHANGELOG.md` and fails if one changed without the other
> - The check is documented in `CONTRIBUTING.md`

Before writing a single line of code, ask: do you understand what "done"
looks like? Here, "done" means:

- A script (or CI step) that fails when `Cargo.toml`'s version was bumped
  without a matching `CHANGELOG.md` entry.
- A sentence in `CONTRIBUTING.md` telling contributors about this check.

The label says `complexity: trivial` — this should be a small,
self-contained change. If your plan looks like more than a single new
file plus a CI step and one documentation line, pause and re-read.

---

## Step 2 — Comment on the issue to claim it

Post a short comment on the issue before you start:

> "I'll take this one. Plan: add a `scripts/check-changelog.sh` that
> checks whether `Cargo.toml`'s version is mentioned in `CHANGELOG.md`,
> then wire it into `.github/workflows/ci.yml` as a new job."

This signals to others that the issue is being worked, and gives
maintainers a chance to flag if your plan is off-track before you write
any code.

---

## Step 3 — Fork (once) and set up remotes

If you haven't already forked the repository:

```sh
# Fork via GitHub UI, then clone your fork
git clone https://github.com/<your-username>/compliance-primitives.git
cd compliance-primitives

# Add the upstream remote so you can pull future changes
git remote add upstream https://github.com/stellar-compliance-kit/compliance-primitives.git
```

Your `git remote -v` should look like:

```
origin    https://github.com/<your-username>/compliance-primitives (fetch)
origin    https://github.com/<your-username>/compliance-primitives (push)
upstream  https://github.com/stellar-compliance-kit/compliance-primitives (fetch)
upstream  https://github.com/stellar-compliance-kit/compliance-primitives (push)
```

---

## Step 4 — Create a branch off main

Always branch off the latest `main`:

```sh
git checkout main
git pull upstream main       # sync with upstream before creating the branch
git checkout -b add-changelog-ci-check
```

Branch names should be short and descriptive. `add-changelog-ci-check`,
`fix-denylist-remove-fn`, `docs-first-pr-walkthrough` are all fine.
Avoid generic names like `fix` or `patch`.

---

## Step 5 — Make the change

For issue #302, the changes were:

**1. Add `scripts/check-changelog.sh`**

A short shell script that:
- Extracts the `version = "..."` value from `[workspace.package]` in `Cargo.toml`.
- Checks whether that version string appears in `CHANGELOG.md`.
- Exits with a non-zero status and a helpful message if it doesn't.

```bash
#!/usr/bin/env bash
set -euo pipefail

VERSION=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')
if ! grep -q "$VERSION" CHANGELOG.md; then
  echo "Error: Cargo.toml version $VERSION not found in CHANGELOG.md."
  echo "Add a CHANGELOG.md entry before bumping the version."
  exit 1
fi
echo "CHANGELOG.md is in sync with Cargo.toml version $VERSION."
```

Make it executable: `chmod +x scripts/check-changelog.sh`.

**2. Wire it into `.github/workflows/ci.yml`**

Add a new job `changelog-version-sync` that runs the script on every PR and push:

```yaml
changelog-version-sync:
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@v4
    - name: Check CHANGELOG.md and version sync
      run: ./scripts/check-changelog.sh
```

**3. Document it in `CONTRIBUTING.md`**

In the workflow steps section, add:

> If you've bumped the workspace version in `Cargo.toml`, add a
> corresponding entry to `CHANGELOG.md`. CI enforces this via
> `./scripts/check-changelog.sh`, which ensures every version bump is
> accompanied by a changelog entry.

**4. Add `CHANGELOG.md` itself**

Since this is the first time the repo has a `CHANGELOG.md`, create it with
the initial `v0.1.0` entry so the CI check passes immediately.

---

## Step 6 — Test locally before pushing

```sh
# Run the check script directly
./scripts/check-changelog.sh

# Run the full test suite
make test

# Run the linter
make lint
```

Both `make test` and `make lint` must pass before you open a PR. The
`check-changelog.sh` script itself is trivially testable — temporarily
bump the version in `Cargo.toml` without touching `CHANGELOG.md` and
confirm the script exits non-zero.

---

## Step 7 — Commit with a clear message

```sh
git add scripts/check-changelog.sh .github/workflows/ci.yml CONTRIBUTING.md CHANGELOG.md
git commit -m "ci: add changelog/version-sync check

Adds scripts/check-changelog.sh, which exits non-zero if
Cargo.toml's workspace version is not present in CHANGELOG.md.
Wired into CI as 'changelog-version-sync' job on every PR and push.
Adds initial CHANGELOG.md with a v0.1.0 entry.
Documents the requirement in CONTRIBUTING.md.

Closes #302"
```

Good commit messages: one-line summary under 72 characters, a blank line,
then a short explanation of *what* and *why*. Reference the issue number
with `Closes #302` so GitHub closes it automatically when the PR merges.

---

## Step 8 — Push the branch to your fork

```sh
git push -u origin add-changelog-ci-check
```

---

## Step 9 — Open a PR against `main` on the upstream repo

```sh
gh pr create \
  --repo stellar-compliance-kit/compliance-primitives \
  --head <your-username>:add-changelog-ci-check \
  --base main \
  --title "ci: add CHANGELOG.md / version-sync check" \
  --body "Adds scripts/check-changelog.sh and wires it into CI.

The script exits non-zero if Cargo.toml's workspace.package.version
is not present in CHANGELOG.md, preventing silent version bumps.
Also adds the initial CHANGELOG.md and documents the requirement in
CONTRIBUTING.md.

Closes #302"
```

Or use the GitHub UI: go to your fork, switch to the branch, click
**Compare & pull request**, and change the base repository to
`stellar-compliance-kit/compliance-primitives` and base branch to `main`.

Fill in the PR title and description following the template in
`.github/PULL_REQUEST_TEMPLATE.md`.

---

## Step 10 — Respond to review feedback

Maintainers will review the PR. Common feedback on trivial issues:

- "Nit: the script can be simplified to one line" — make the change,
  `git commit --amend` (if the PR has no other commits yet and hasn't been
  reviewed; otherwise add a new commit) and `git push --force-with-lease`.
- "Please also add a test for the script" — add a test script or a
  `@test` case and push an additional commit.

Once all feedback is addressed and CI passes, a maintainer will merge
the PR and close the issue.

---

## What made this a `complexity: trivial` issue

Looking back at issue #302:

- **One new file** (`scripts/check-changelog.sh`) — a short shell script,
  no Rust, no new contract logic.
- **One CI step** added to an existing workflow.
- **One sentence** added to `CONTRIBUTING.md`.
- **One new file** (`CHANGELOG.md`) that was needed anyway and couldn't
  break anything.
- No changes to any contract's public API, no changes to any `DataKey`
  enum, no storage-layout implications.

A good `complexity: trivial` PR has a diff under ~50 lines of meaningful
change and touches only one area of the codebase. If your diff grows
beyond that while working on a trivial issue, check whether the issue
scope has crept or whether you can split the extra work into a separate PR.

---

## Where to find more `complexity: trivial` issues

Filter the issue tracker:
[`label:"complexity: trivial"`](https://github.com/stellar-compliance-kit/compliance-primitives/labels/complexity%3A%20trivial)

Issues also tagged [`good first issue`](https://github.com/stellar-compliance-kit/compliance-primitives/labels/good%20first%20issue)
are explicitly flagged as suitable for a first contribution — start there
if you're unsure.
