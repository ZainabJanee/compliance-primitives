# multisig-governed-allowlist

Reference example demonstrating how to compose `multisig-admin` as the `admin`
of an `allowlist-token` contract, so that every admin operation on the allowlist
requires M-of-N signer approval instead of a single key.

## Pattern

```
┌─────────────┐  add_to_allowlist(multisig_addr, alice)
│  Initiator  │ ────────────────────────────────────────────►
└─────────────┘                                         allowlist-token
                                                              │
                                                  admin.require_auth()
                                                              │
                                                 (Soroban routes to)
                                                              ▼
                                                      multisig-admin
                                                 __check_auth(signatures)
                                                              │
                                            ┌─────────────────┴──────────┐
                                            │ count valid signers in set  │
                                            │ count >= threshold?         │
                                            └──── yes ────────────────────┘
                                                              │
                                                 authorization granted
                                                              │
                                                 alice added to allowlist
```

No changes to `allowlist-token` are needed. The Soroban auth framework
automatically calls `__check_auth` on the admin contract when `require_auth()`
is called on a contract address.

## Deployment (testnet sketch)

```sh
# 1. Deploy the underlying SEP-41 token
stellar contract deploy --wasm <token.wasm> --source <key> --network testnet
# → TOKEN_ID

# 2. Deploy multisig-admin
stellar contract deploy \
  --wasm target/wasm32v1-none/release/multisig_admin.wasm \
  --source <key> --network testnet
# → MULTISIG_ID

# 3. Initialize multisig-admin with 3 signers, threshold 2
stellar contract invoke --id $MULTISIG_ID --source <key> --network testnet \
  -- initialize \
  --signers '["SIGNER_A","SIGNER_B","SIGNER_C"]' \
  --threshold 2

# 4. Deploy allowlist-token with multisig as admin
stellar contract deploy \
  --wasm target/wasm32v1-none/release/allowlist_token.wasm \
  --source <key> --network testnet
# → ALLOWLIST_ID

stellar contract invoke --id $ALLOWLIST_ID --source <key> --network testnet \
  -- initialize \
  --admin $MULTISIG_ID \
  --token $TOKEN_ID

# 5. Add an address — SIGNER_A and SIGNER_B must both sign the transaction
stellar contract invoke --id $ALLOWLIST_ID \
  --source <key> --network testnet \
  -- add_to_allowlist \
  --admin $MULTISIG_ID \
  --address <ALICE>
```

## Tests

| Test | What it covers |
|------|----------------|
| `test_multisig_set_as_allowlist_admin` | Multisig address is stored as the allowlist admin |
| `test_add_to_allowlist_via_multisig_admin` | `add_to_allowlist` succeeds when multisig is admin |
| `test_remove_from_allowlist_via_multisig_admin` | `remove_from_allowlist` routes through multisig auth |
| `test_multisig_can_manage_multiple_allowlist_entries` | Multiple addresses managed through one multisig |
| `test_pause_and_unpause_via_multisig_admin` | Pause/unpause also gated by multisig auth |
| `test_check_auth_grants_auth_at_threshold` | 2-of-3 threshold met with 2 valid signers |
| `test_check_auth_rejects_below_threshold` | 1 signer fails 2-of-3 threshold |
| `test_check_auth_outsider_does_not_count` | Non-signer address does not count toward threshold |

## Related examples

- [`multisig-aggregator`](../multisig-aggregator) — same pattern for `compliance-aggregator`
- [`multisig-audit-trail`](../multisig-audit-trail) — pairs the proposal workflow with `audit-log`
- [`allowlist-token-usage`](../allowlist-token-usage) — standalone allowlist usage without multisig
