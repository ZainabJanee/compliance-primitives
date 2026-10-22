# multisig-admin

M-of-N administrative governance for protected contract actions.

| Method | Purpose |
| --- | --- |
| `initialize(signers, threshold)` | Configure signers and the approval threshold. |
| `propose(action)` | Create a pending administrative proposal. |
| `approve(signer, proposal_id)` | Approve a proposal as an authorized signer. |
| `execute(proposal_id)` | Execute a proposal after the threshold is met. |
| `upgrade(new_wasm_hash)` | Move the contract to a new WASM hash. Requires the M-of-N threshold. |

## Upgrade path

`upgrade(new_wasm_hash)` swaps the contract code via
`env.deployer().update_current_contract_wasm`, gated by
`env.current_contract_address().require_auth()` — i.e. the current M-of-N
signer threshold must approve it through `__check_auth`, the same as
`add_signer` or `update_threshold`.

1. Build and upload the new WASM:
   `stellar contract upload --wasm target/wasm32v1-none/release/multisig_admin.wasm`
   and note the returned hash.
2. Invoke `upgrade --new_wasm_hash <HASH>` on the multisig with authorization
   from at least `threshold` signers.
3. Verify with `get_signers` / `get_proposal` that state is intact.

The swap does not touch storage: the signer set, threshold, pending proposals
and the next proposal ID are preserved, and the contract address stays the
same, so every primitive that uses the multisig as its admin keeps working
without reconfiguration. An `Upgraded` event carrying the new WASM hash is
emitted. A future version that changes the storage layout must ship a
migration entrypoint (see [`STORAGE_VERSIONING.md`](../../STORAGE_VERSIONING.md)).
