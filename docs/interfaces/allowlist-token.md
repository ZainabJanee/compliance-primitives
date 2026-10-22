# allowlist-token

A SEP-41-compatible token wrapper that gates transfers against an on-chain allowlist.

| Method | Purpose |
| --- | --- |
| `initialize(admin, token)` | Configure the administrator and underlying token. |
| `add_to_allowlist(admin, address, expiration_ledger: Option<u32>)` | Add an address to the allowlist. With `Some(n)` the entry auto-expires after ledger `n` (valid while sequence `<= n`); `None` never expires. |
| `remove_from_allowlist(admin, address)` | Remove an address from the allowlist. |
| `is_allowed(address) -> bool` | Read whether an address is currently allowed. Expired entries read as `false`. |
| `get_allowlist_entry(address) -> Option<AllowlistEntry>` | Read the raw stored entry, including its `expiration_ledger`. |
| `transfer(from, to, amount)` | Transfer only when the compliance policy permits it. |

This is a reference interface; confirm the deployed contract configuration before invoking write methods.
