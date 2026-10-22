#!/usr/bin/env bash
# check-contract-line-count.sh
#
# Flags any contracts/*/src/lib.rs that exceeds the ~300-line guideline
# documented in CONTRIBUTING.md.
#
# This is a WARNING, not a hard failure: the script exits 0 even when
# contracts exceed the limit so it doesn't block merges, but it prints
# a visible nudge to consider splitting the contract.  Set
# HARD_FAIL=1 in the environment to turn it into a blocking failure.

set -euo pipefail

LINE_LIMIT="${LINE_LIMIT:-300}"
HARD_FAIL="${HARD_FAIL:-0}"
CONTRACTS_DIR="$(dirname "$0")/../contracts"

OVER_LIMIT=()

for lib in "$CONTRACTS_DIR"/*/src/lib.rs; do
  [ -f "$lib" ] || continue
  lines=$(wc -l < "$lib")
  contract=$(basename "$(dirname "$(dirname "$lib")")")
  if [ "$lines" -gt "$LINE_LIMIT" ]; then
    OVER_LIMIT+=("$contract ($lines lines)")
    echo "⚠  $contract/src/lib.rs: ${lines} lines (guideline: ~${LINE_LIMIT})"
  else
    echo "✓  $contract/src/lib.rs: ${lines} lines"
  fi
done

if [ "${#OVER_LIMIT[@]}" -gt 0 ]; then
  echo ""
  echo "The following contracts exceed the ~${LINE_LIMIT}-line guideline from CONTRIBUTING.md:"
  for entry in "${OVER_LIMIT[@]}"; do
    echo "  • $entry"
  done
  echo ""
  echo "Consider splitting large contracts into smaller, single-responsibility crates."
  if [ "$HARD_FAIL" = "1" ]; then
    exit 1
  fi
fi

exit 0
