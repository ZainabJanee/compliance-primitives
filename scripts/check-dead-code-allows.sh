#!/usr/bin/env bash
# scripts/check-dead-code-allows.sh — Require every #[allow(dead_code)] in a
# PR diff to have a tracking issue reference in an adjacent comment.
#
# What "adjacent" means here:
#   The line immediately above or the same line as #[allow(dead_code)] must
#   contain a GitHub issue reference of the form #NNN or
#   https://github.com/.../.../issues/NNN.
#
# Running modes:
#   CI (default)  — checks only lines added by the current PR diff
#                   (git diff against the merge-base with origin/main).
#   --all         — scans every #[allow(dead_code)] in the whole source tree,
#                   not just the diff. Useful for auditing existing code.
#
# Exit codes:
#   0 — every #[allow(dead_code)] found has a linked issue
#   1 — one or more #[allow(dead_code)] are missing a linked issue
#
# Usage:
#   ./scripts/check-dead-code-allows.sh          # CI mode (diff only)
#   ./scripts/check-dead-code-allows.sh --all    # full-tree scan

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

SCAN_ALL=0
for arg in "$@"; do
  case "$arg" in
    --all) SCAN_ALL=1 ;;
    *) echo "error: unknown argument '$arg'" >&2; exit 1 ;;
  esac
done

ISSUE_PATTERN='#[0-9]\+\|https://github\.com/[^/]\+/[^/]\+/issues/[0-9]\+'

failures=()

check_file_line() {
  local file="$1"
  local lineno="$2"

  # Read the line above (lineno-1) and the line itself (lineno).
  # awk lines are 1-indexed; lineno is already 1-indexed from grep.
  local prev_line=""
  if [ "$lineno" -gt 1 ]; then
    prev_line=$(sed -n "$((lineno - 1))p" "$file")
  fi
  local this_line
  this_line=$(sed -n "${lineno}p" "$file")

  # Check if either line contains an issue reference.
  if echo "$prev_line$this_line" | grep -q "$ISSUE_PATTERN"; then
    return 0  # has a linked issue
  fi

  failures+=("${file}:${lineno}")
}

if [ "$SCAN_ALL" -eq 1 ]; then
  # Scan every Rust source file in the workspace.
  echo "Scanning all Rust source files for unlinked #[allow(dead_code)]..."
  while IFS=: read -r file lineno _rest; do
    check_file_line "$file" "$lineno"
  done < <(grep -rn '#\[allow(dead_code)\]' contracts/ examples/ src/ 2>/dev/null || true)
else
  # CI mode: only look at lines added by this PR (lines starting with '+' in
  # the diff, excluding the diff header lines '+++').
  echo "Checking PR diff for unlinked #[allow(dead_code)]..."

  # Determine the merge-base to diff against.
  BASE_REF="${GITHUB_BASE_REF:-main}"
  if git rev-parse "origin/${BASE_REF}" &>/dev/null; then
    MERGE_BASE=$(git merge-base HEAD "origin/${BASE_REF}")
  elif git rev-parse "${BASE_REF}" &>/dev/null; then
    MERGE_BASE=$(git merge-base HEAD "${BASE_REF}")
  else
    echo "warning: cannot find base ref '${BASE_REF}'; falling back to HEAD~1" >&2
    MERGE_BASE=$(git rev-parse HEAD~1 2>/dev/null || git rev-parse HEAD)
  fi

  # Parse the unified diff to find added #[allow(dead_code)] lines with their
  # source file and line number so we can inspect the preceding line.
  CURRENT_FILE=""
  CURRENT_NEW_LINE=0

  while IFS= read -r diff_line; do
    case "$diff_line" in
      +++ b/*)
        CURRENT_FILE="${diff_line#+++ b/}"
        ;;
      @@*)
        # Extract the new-file starting line from the @@ hunk header.
        # Format: @@ -old_start,old_count +new_start,new_count @@
        NEW_START=$(echo "$diff_line" | sed -n 's/^@@ -[0-9,]* +\([0-9]*\).*/\1/p')
        CURRENT_NEW_LINE="${NEW_START:-0}"
        ;;
      +*)
        # An added line.
        stripped="${diff_line:1}"  # remove leading '+'
        if echo "$stripped" | grep -q '#\[allow(dead_code)\]'; then
          if [ -n "$CURRENT_FILE" ] && [ -f "$CURRENT_FILE" ]; then
            check_file_line "$CURRENT_FILE" "$CURRENT_NEW_LINE"
          fi
        fi
        CURRENT_NEW_LINE=$((CURRENT_NEW_LINE + 1))
        ;;
      -*)
        # A removed line — doesn't advance the new-file line counter.
        ;;
      *)
        # Context line — advances both counters.
        CURRENT_NEW_LINE=$((CURRENT_NEW_LINE + 1))
        ;;
    esac
  done < <(git diff "$MERGE_BASE" HEAD -- '*.rs')
fi

# ---------------------------------------------------------------------------
# Report
# ---------------------------------------------------------------------------
if [ "${#failures[@]}" -eq 0 ]; then
  echo "✓ All #[allow(dead_code)] attributes have a linked issue reference."
  exit 0
fi

echo ""
echo "✗ The following #[allow(dead_code)] attributes are missing a linked issue reference:"
for loc in "${failures[@]}"; do
  echo "  $loc"
done
echo ""
echo "Each #[allow(dead_code)] must have a GitHub issue reference (#NNN or a"
echo "full issues URL) in the same line or the line immediately above it."
echo ""
echo "Example:"
echo "  // Kept for future event emission — see #457"
echo "  #[allow(dead_code)]"
echo "  pub struct CheckFailure { ... }"
exit 1
