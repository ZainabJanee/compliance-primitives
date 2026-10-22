# policy-engine

A configurable policy composition contract that evaluates denylist and jurisdiction checks together.

| Method | Purpose |
| --- | --- |
| `initialize(admin, combine_op)` | Initialize governance and combine semantics. |
| `propose_upgrade(admin, new_wasm, delay_ledgers)` | Propose an upgrade that is installable after the ledger delay. |
| `commit_upgrade(admin)` | Install the pending upgrade after its delay. |
| `cancel_upgrade(admin)` | Cancel a pending upgrade. |
| `add_check(admin, check)` | Add a configured compliance check. |
| `remove_check(admin, index)` | Remove a configured check. |
| `evaluate(from, to) -> bool` | Return the combined policy decision. |
| `get_checks()` | Read the configured checks. |
| `get_op()` | Read whether checks are combined with all/any semantics. |
