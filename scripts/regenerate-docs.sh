#!/usr/bin/env bash
# Build all workspace contracts to wasm and extract their Soroban XDR
# interface definitions (spec) for documentation purposes.
#
# Usage:
#   scripts/regenerate-docs.sh
#
# Prerequisites:
#   - stellar CLI installed and on PATH
#   - wasm32v1-none target added (`rustup target add wasm32v1-none`)
#
# Output:
#   docs/interfaces/<contract>.json   – contract spec in JSON (human-readable)
#   docs/interfaces/<contract>.xdr    – contract spec as base64-encoded XDR
#
# Re-run this script after changing any public function signature or
# interface-related attribute on a contract.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

DOCS_DIR="docs/interfaces"
WASM_DIR="target/wasm32v1-none/release"

# Ordered list of contract crate names (the nine deployable contracts; the
# pausable crate is a library helper and has no wasm of its own).  WASM file
# names are derived by replacing hyphens with underscores.
CRATES=(
  "allowlist-token"
  "audit-log"
  "circuit-breaker"
  "compliance-aggregator"
  "compliance-check"
  "denylist-gate"
  "jurisdiction-flag"
  "multisig-admin"
  "policy-engine"
)

# ── prerequisites ──────────────────────────────────────────────────────────

if ! command -v stellar &>/dev/null; then
  echo "error: stellar CLI not found on PATH; install from https://developers.stellar.org/docs/tools/developer-tools/cli" >&2
  exit 1
fi

# ── build ──────────────────────────────────────────────────────────────────

echo "==> Building all contracts to wasm (stellar contract build) …"
stellar contract build

# ── extract interfaces ─────────────────────────────────────────────────────

mkdir -p "$DOCS_DIR"

for crate in "${CRATES[@]}"; do
  wasm_name="${crate//-/_}.wasm"
  wasm_path="${WASM_DIR}/${wasm_name}"

  if [ ! -f "$wasm_path" ]; then
    echo "error: expected build artifact not found at $wasm_path" >&2
    exit 1
  fi

  echo "==> Extracting interface for ${crate} …"
  stellar contract inspect \
    --wasm "$wasm_path" \
    --output json \
    > "${DOCS_DIR}/${crate}.json"

  stellar contract inspect \
    --wasm "$wasm_path" \
    --output xdr-base64 \
    > "${DOCS_DIR}/${crate}.xdr"
done

# ── summary ────────────────────────────────────────────────────────────────

echo ""
echo "==> Done.  Generated files:"
printf "  %-35s %s\n" "JSON" "XDR"
printf "  %-35s %s\n" "----" "---"
for crate in "${CRATES[@]}"; do
  printf "  %-35s %s\n" \
    "${DOCS_DIR}/${crate}.json" \
    "${DOCS_DIR}/${crate}.xdr"
done

echo ""
echo "Commit these files to keep the docs up to date with the contracts."
