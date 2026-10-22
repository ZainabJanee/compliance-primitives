#!/usr/bin/env bash
# Build each budgeted contract to wasm and check its binary size against
# wasm-size-budgets.toml. Run as the "wasm size budget" CI job
# (.github/workflows/ci.yml) and via `make check-wasm-size` locally.
#
# A contract's binary may grow up to 10% over its baseline before this
# fails (same tolerance convention as the per-function CPU/memory budgets
# checked by the `budget-regression` CI job / budget-baselines.toml).
#
# Contracts with no entry in wasm-size-budgets.toml are skipped with a
# warning rather than failing the build — see the comment at the top of
# that file for which contracts are currently excluded and why.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

BUDGETS_FILE="wasm-size-budgets.toml"
WASM_DIR="target/wasm32v1-none/release"
TOLERANCE="1.10" # +10%, matches assert_budget_within_threshold in contract tests

if [ ! -f "$BUDGETS_FILE" ]; then
  echo "error: $BUDGETS_FILE not found" >&2
  exit 1
fi

# All contract crate directories under contracts/.
mapfile -t ALL_CRATES < <(find contracts -mindepth 1 -maxdepth 1 -type d -exec basename {} \; | sort)

# Crates with a [<crate>] table in the budgets file.
mapfile -t BUDGETED_CRATES < <(grep -E '^\[[a-z0-9-]+\]' "$BUDGETS_FILE" | tr -d '[]' | sort)

echo "==> Contracts: ${#ALL_CRATES[@]} total, ${#BUDGETED_CRATES[@]} budgeted"
for crate in "${ALL_CRATES[@]}"; do
  if ! printf '%s\n' "${BUDGETED_CRATES[@]}" | grep -qx "$crate"; then
    echo "    skip: $crate (no budget entry in $BUDGETS_FILE)"
  fi
done
echo ""

if [ "${#BUDGETED_CRATES[@]}" -eq 0 ]; then
  echo "error: no budgeted contracts found in $BUDGETS_FILE" >&2
  exit 1
fi

echo "==> Building budgeted contracts to wasm …"
cargo build --target wasm32v1-none --release "${BUDGETED_CRATES[@]/#/-p}"
echo ""

get_field() {
  # get_field <crate> <field>  -- reads "field = value" from that crate's
  # table in the budgets TOML (values are bare strings/numbers, no quotes
  # needed for this file's schema).
  awk -v crate="[$1]" -v field="$2" '
    $0 == crate { in_table = 1; next }
    /^\[/ { in_table = 0 }
    in_table && $1 == field {
      sub(/^[^=]*=[ \t]*/, "");
      gsub(/"/, "");
      print;
      exit
    }
  ' "$BUDGETS_FILE"
}

fail=0
failures=()
printf "%-24s %10s %10s %10s  %s\n" "contract" "size" "budget" "limit" "status"
printf "%-24s %10s %10s %10s  %s\n" "--------" "----" "------" "-----" "------"

for crate in "${BUDGETED_CRATES[@]}"; do
  wasm_name="$(get_field "$crate" wasm)"
  max_bytes="$(get_field "$crate" max_bytes)"
  wasm_path="${WASM_DIR}/${wasm_name}"

  if [ ! -f "$wasm_path" ]; then
    echo "error: expected build artifact not found at $wasm_path" >&2
    fail=1
    continue
  fi

  size="$(stat -c%s "$wasm_path" 2>/dev/null || stat -f%z "$wasm_path")"
  limit="$(awk -v b="$max_bytes" -v t="$TOLERANCE" 'BEGIN { printf "%d", b * t }')"

  if [ "$size" -gt "$limit" ]; then
    over_limit=$((size - limit))
    over_budget=$((size - max_bytes))
    pct="$(awk -v s="$size" -v b="$max_bytes" 'BEGIN { printf "%.1f", (s - b) * 100 / b }')"
    status="FAIL (+${over_limit} B over limit)"
    failures+=("$crate: ${wasm_path} is ${size} bytes, ${over_limit} bytes over its ${limit}-byte limit (${over_budget} bytes / ${pct}% over its ${max_bytes}-byte baseline)")
    fail=1
  else
    status="ok"
  fi
  printf "%-24s %10s %10s %10s  %s\n" "$crate" "$size" "$max_bytes" "$limit" "$status"
done

echo ""
if [ "$fail" -ne 0 ]; then
  echo "==> wasm size budget check FAILED"
  for msg in "${failures[@]}"; do
    echo "error: $msg" >&2
    # Surface each failure as an annotation on the PR when run in Actions.
    if [ -n "${GITHUB_ACTIONS:-}" ]; then
      echo "::error title=wasm size budget::$msg"
    fi
  done
  echo "" >&2
  echo "If the growth is intentional, re-measure and update max_bytes in $BUDGETS_FILE" >&2
  echo "and explain the increase in the PR description." >&2
  exit 1
fi
echo "==> wasm size budget check passed"
