// Copyright (c) 2026 Stellar Compliance Kit contributors
// SPDX-License-Identifier: MIT
// See the LICENSE file in the repository root for the full license text.

//! **`multisig-governed-allowlist`** — reference example composing
//! `multisig-admin` as the `admin` of an `allowlist-token` instance.
//!
//! ## What this example demonstrates
//!
//! `allowlist-token` stores an admin address and calls `admin.require_auth()`
//! before any privileged operation (`add_to_allowlist`, `remove_from_allowlist`,
//! `pause`, `unpause`). By setting the admin address to a deployed
//! `multisig-admin` contract, every admin action on the allowlist requires M-of-N
//! signer approval instead of a single key.
//!
//! The flow for **adding an address to the allowlist** under multisig governance:
//!
//! ```text
//! ┌─────────────┐  add_to_allowlist(multisig_addr, alice)
//! │  Initiator  │ ─────────────────────────────────────────────────►
//! └─────────────┘                                                allowlist-token
//!                                                                      │
//!                                                          admin.require_auth()
//!                                                                      │
//!                                                         (Soroban routes to)
//!                                                                      ▼
//!                                                              multisig-admin
//!                                                         __check_auth(signatures)
//!                                                                      │
//!                                                    ┌─────────────────┴──────────┐
//!                                                    │ count valid signers in set  │
//!                                                    │ count >= threshold?         │
//!                                                    └──── yes ────────────────────┘
//!                                                                      │
//!                                                         authorization granted
//!                                                                      │
//!                                                         alice added to allowlist
//! ```
//!
//! ## Key insight
//!
//! No changes to `allowlist-token` are needed. Any contract that uses
//! `address.require_auth()` for admin operations works identically with
//! `multisig-admin` — the Soroban auth framework calls `__check_auth` on the
//! admin contract transparently.
//!
//! ## Testnet deployment sketch
//!
//! ```sh
//! # 1. Deploy the underlying SEP-41 token (or use an existing Stellar Asset Contract)
//! stellar contract deploy --wasm <token.wasm> --source <key> --network testnet
//! # → TOKEN_ID
//!
//! # 2. Deploy multisig-admin
//! stellar contract deploy --wasm target/wasm32v1-none/release/multisig_admin.wasm \
//!   --source <key> --network testnet
//! # → MULTISIG_ID
//!
//! # 3. Initialize multisig-admin with your signer set (e.g. 2-of-3)
//! stellar contract invoke --id $MULTISIG_ID --source <key> --network testnet \
//!   -- initialize \
//!   --signers '["SIGNER_A","SIGNER_B","SIGNER_C"]' \
//!   --threshold 2
//!
//! # 4. Deploy allowlist-token with multisig as admin
//! stellar contract deploy --wasm target/wasm32v1-none/release/allowlist_token.wasm \
//!   --source <key> --network testnet
//! # → ALLOWLIST_ID
//!
//! stellar contract invoke --id $ALLOWLIST_ID --source <key> --network testnet \
//!   -- initialize \
//!   --admin $MULTISIG_ID \
//!   --token $TOKEN_ID
//!
//! # 5. Add an address to the allowlist (requires SIGNER_A + SIGNER_B to sign the tx)
//! stellar contract invoke --id $ALLOWLIST_ID \
//!   --source <key> --auth $MULTISIG_ID \
//!   --signers '["SIGNER_A","SIGNER_B"]' \
//!   --network testnet \
//!   -- add_to_allowlist \
//!   --admin $MULTISIG_ID \
//!   --address <ALICE>
//! ```

#![no_std]

use soroban_sdk::{contract, contractimpl, Env};

/// Placeholder contract — this crate exists for its tests and documentation.
/// The real composition happens between the deployed `multisig-admin` and
/// `allowlist-token` contracts; no wrapper contract is needed.
#[contract]
pub struct MultisigGovernedAllowlist;

#[contractimpl]
impl MultisigGovernedAllowlist {
    /// No-op entry point. All logic lives in the tests below and in the
    /// deployed `multisig-admin` + `allowlist-token` contracts.
    pub fn placeholder(_env: Env) {}
}

#[cfg(test)]
mod test;
