# circuit-breaker

An emergency control that lets an authorized operator stop and resume protected flows.

| Method | Purpose |
| --- | --- |
| `initialize(admin)` | Configure the administrator. |
| `is_tripped() -> bool` | Read the current breaker state. |
| `trip(admin)` | Enter the emergency-tripped state. |
| `reset(admin)` | Resume normal operation. |
| `upgrade(admin, new_wasm_hash)` | Admin-only. Move the contract code to a new WASM hash; storage is preserved. See the "Upgradeability" section in `contracts/circuit-breaker/src/lib.rs`. |
