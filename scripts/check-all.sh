#!/usr/bin/env bash
# scripts/check-all.sh — Run cargo check -p <crate> over every workspace
# member, continuing past individual failures, then print a pass/fail summary.
#
# Background: a single `cargo check --workspace` stops at the first error,
# so a broken crate masks all subsequent crates. This script loops each crate
# independently so you get the full picture of what's broken in one pass.
#
# Usage:
#   ./scripts/check-all.sh [--test] [--clippy]
#
# Options:
#   --test    Also run `cargo test -p <crate>` for each member.
#   --clippy  Also run `cargo clippy -p <crate> --all-targets -- -D warnings`
#             for each member.
#
# Exit codes:
#   0 — all crates passed every requested check
#   1 — one or more crates failed at least one check

set -uo pipefail  # note: no -e so we can continue past failures

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

RUN_TEST=0
RUN_CLIPPY=0
for arg in "$@"; do
  case "$arg" in
    --test)   RUN_TEST=1 ;;
    --clippy) RUN_CLIPPY=1 ;;
    *) echo "error: unknown argument '$arg'" >&2; exit 1 ;;
  esac
done

# ---------------------------------------------------------------------------
# Collect workspace members from Cargo.toml
# ---------------------------------------------------------------------------
# Extract lines inside [workspace] members = [...] that look like quoted paths.
MEMBERS=()
in_members=0
while IFS= read -r line; do
  if echo "$line" | grep -qE '^\[workspace\]'; then
    in_members=0
  fi
  if echo "$line" | grep -qE '^members\s*='; then
    in_members=1
  fi
  if [ "$in_members" -eq 1 ]; then
    member=$(echo "$line" | sed -n 's/.*"\(.*\)".*/\1/p')
    if [ -n "$member" ]; then
      MEMBERS+=("$member")
    fi
    if echo "$line" | grep -q '\]'; then
      in_members=0
    fi
  fi
done < Cargo.toml

if [ "${#MEMBERS[@]}" -eq 0 ]; then
  echo "error: could not parse any workspace members from Cargo.toml" >&2
  exit 1
fi

echo "Found ${#MEMBERS[@]} workspace members."
echo ""

# ---------------------------------------------------------------------------
# Per-crate results tracking
# ---------------------------------------------------------------------------
# Arrays parallel to MEMBERS: store "pass" or "fail:<step>" per crate.
declare -A CHECK_RESULT
declare -A TEST_RESULT
declare -A CLIPPY_RESULT

OVERALL_PASS=1

run_step() {
  local crate="$1"
  local step_name="$2"
  shift 2
  local cmd=("$@")

  printf "  %-10s " "$step_name"
  if "${cmd[@]}" > /tmp/check-all-output 2>&1; then
    echo "✓"
    echo "pass"
  else
    echo "✗"
    cat /tmp/check-all-output | head -20
    OVERALL_PASS=0
    echo "fail"
  fi
}

# ---------------------------------------------------------------------------
# Main loop
# ---------------------------------------------------------------------------
for member_path in "${MEMBERS[@]}"; do
  # Derive the crate name from its Cargo.toml (name = "...").
  cargo_toml="${member_path}/Cargo.toml"
  if [ ! -f "$cargo_toml" ]; then
    echo "warning: $cargo_toml not found, skipping" >&2
    continue
  fi
  crate_name=$(grep -E '^name\s*=' "$cargo_toml" | head -1 | sed 's/name\s*=\s*"\(.*\)"/\1/')
  if [ -z "$crate_name" ]; then
    crate_name="$(basename "$member_path")"
  fi

  echo "── $crate_name ($member_path)"

  CHECK_RESULT[$crate_name]=$(run_step "$crate_name" "check" cargo check -p "$crate_name" --all-targets 2>&1)

  if [ "$RUN_TEST" -eq 1 ]; then
    TEST_RESULT[$crate_name]=$(run_step "$crate_name" "test" cargo test -p "$crate_name" 2>&1)
  fi

  if [ "$RUN_CLIPPY" -eq 1 ]; then
    CLIPPY_RESULT[$crate_name]=$(run_step "$crate_name" "clippy" \
      cargo clippy -p "$crate_name" --all-targets -- -D warnings 2>&1)
  fi
done

# ---------------------------------------------------------------------------
# Summary table
# ---------------------------------------------------------------------------
echo ""
echo "┌──────────────────────────────────────────────────────────────────┐"
printf "│ %-20s %-10s" "Crate" "check"
[ "$RUN_TEST" -eq 1 ]   && printf " %-10s" "test"
[ "$RUN_CLIPPY" -eq 1 ] && printf " %-10s" "clippy"
echo " │"
echo "├──────────────────────────────────────────────────────────────────┤"

for member_path in "${MEMBERS[@]}"; do
  cargo_toml="${member_path}/Cargo.toml"
  [ -f "$cargo_toml" ] || continue
  crate_name=$(grep -E '^name\s*=' "$cargo_toml" | head -1 | sed 's/name\s*=\s*"\(.*\)"/\1/')
  [ -z "$crate_name" ] && crate_name="$(basename "$member_path")"

  check_icon="✓"
  [[ "${CHECK_RESULT[$crate_name]:-}" == *fail* ]] && check_icon="✗"

  printf "│ %-20s %-10s" "$crate_name" "$check_icon"

  if [ "$RUN_TEST" -eq 1 ]; then
    test_icon="✓"
    [[ "${TEST_RESULT[$crate_name]:-}" == *fail* ]] && test_icon="✗"
    printf " %-10s" "$test_icon"
  fi

  if [ "$RUN_CLIPPY" -eq 1 ]; then
    clippy_icon="✓"
    [[ "${CLIPPY_RESULT[$crate_name]:-}" == *fail* ]] && clippy_icon="✗"
    printf " %-10s" "$clippy_icon"
  fi

  echo " │"
done

echo "└──────────────────────────────────────────────────────────────────┘"
echo ""

if [ "$OVERALL_PASS" -eq 1 ]; then
  echo "✓ All checks passed."
  exit 0
else
  echo "✗ One or more checks failed. See output above for details."
  exit 1
fi
