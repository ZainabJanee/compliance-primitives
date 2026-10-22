#!/usr/bin/env bash
# Build all workspace contracts and deploy one of them to testnet, printing
# the resulting contract ID.
#
# Usage:
#   scripts/deploy-testnet.sh <allowlist-token|denylist-gate|jurisdiction-flag|rwa-token>
#
# For the full RWA stack (all three primitives + rwa-token + initialize),
# prefer scripts/deploy-rwa-testnet.sh instead.
#
# The source testnet identity is read from $STELLAR_SOURCE (an identity
# already set up via `stellar keys generate`/`stellar keys address`),
# defaulting to "default" if unset.
#
# Requires stellar-cli compatible with soroban-sdk 27 (cli ≥ 23 / ideally 27.x).
set -euo pipefail

usage() {
  echo "Usage: $0 <allowlist-token|denylist-gate|jurisdiction-flag|rwa-token>" >&2
  exit 1
}

if [ "$#" -ne 1 ]; then
  usage
fi

CONTRACT_NAME="$1"

case "$CONTRACT_NAME" in
  allowlist-token | denylist-gate | jurisdiction-flag | rwa-token) ;;
  *)
    echo "error: unknown contract '$CONTRACT_NAME'" >&2
    usage
    ;;
esac

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

WASM_NAME="${CONTRACT_NAME//-/_}.wasm"
WASM_PATH="target/wasm32v1-none/release/${WASM_NAME}"
SOURCE_IDENTITY="${STELLAR_SOURCE:-default}"

echo "==> Building all contracts to wasm..."
stellar contract build

if [ ! -f "$WASM_PATH" ]; then
  echo "error: expected build artifact not found at $WASM_PATH" >&2
  exit 1
fi

echo "==> Deploying ${CONTRACT_NAME} (${WASM_PATH}) to testnet with source '${SOURCE_IDENTITY}'..."
CONTRACT_ID="$(stellar contract deploy \
  --wasm "$WASM_PATH" \
  --source "$SOURCE_IDENTITY" \
  --network testnet)"

echo "${CONTRACT_ID}"

# ---------------------------------------------------------------------------
# Multisig signer-rotation example (denylist-gate)
#
# The single-admin flow above only deploys the contract. The denylist-gate
# contract additionally supports a multisig authority: `initialize_multisig`
# seeds the signer set, and `add_signer`/`remove_signer` rotate it. This
# example walks through initializing a 2-of-2 multisig and then rotating one
# signer on a live testnet deployment.
#
# Run it explicitly after deploying denylist-gate:
#
#   scripts/deploy-testnet.sh denylist-gate
#   scripts/deploy-testnet.sh --multisig-example <CONTRACT_ID>
#
# The signer identities are read from $STELLAR_SIGNER_A / $STELLAR_SIGNER_B
# (existing `stellar keys` identities), defaulting to "signer-a"/"signer-b".
# The rotation target is read from $STELLAR_SIGNER_C, defaulting to
# "signer-c".
# ---------------------------------------------------------------------------

multisig_example() {
  local contract_id="$1"
  local signer_a="${STELLAR_SIGNER_A:-signer-a}"
  local signer_b="${STELLAR_SIGNER_B:-signer-b}"
  local signer_c="${STELLAR_SIGNER_C:-signer-c}"

  local addr_a addr_b addr_c
  addr_a="$(stellar keys address "$signer_a")"
  addr_b="$(stellar keys address "$signer_b")"
  addr_c="$(stellar keys address "$signer_c")"

  echo "==> Initializing 2-of-2 multisig on ${contract_id}..."
  echo "    signer A: ${addr_a}"
  echo "    signer B: ${addr_b}"
  stellar contract invoke \
    --id "$contract_id" \
    --source "$signer_a" \
    --network testnet \
    -- initialize_multisig \
    --signers "[\"${addr_a}\",\"${addr_b}\"]" \
    --threshold 2

  echo "==> Rotating signer B -> C (add_signer then remove_signer)..."
  echo "    signer C: ${addr_c}"
  stellar contract invoke \
    --id "$contract_id" \
    --source "$signer_a" \
    --network testnet \
    -- add_signer \
    --signer "${addr_c}"

  stellar contract invoke \
    --id "$contract_id" \
    --source "$signer_a" \
    --network testnet \
    -- remove_signer \
    --signer "${addr_b}"

  echo "==> Signer rotation complete: [${addr_a}, ${addr_c}]"
}

if [ "${1:-}" = "--multisig-example" ]; then
  if [ "$#" -ne 2 ]; then
    echo "Usage: $0 --multisig-example <CONTRACT_ID>" >&2
    exit 1
  fi
  multisig_example "$2"
fi
