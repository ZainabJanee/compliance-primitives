# Contract interfaces

This page lists the public interfaces exposed by the compliance primitives. The site version is generated from the Markdown files in this directory.

## allowlist-token

The wrapper forwards a transfer only when both participants are on the allowlist.

```text
add_to_allowlist(admin, address, expiration_ledger)
is_allowed(address) -> bool
transfer(from, to, amount) -> bool
```

## denylist-gate

A composable gate that other contracts call before moving funds.

```text
add_to_denylist(admin, address)
check(address) -> bool
```

## jurisdiction-flag

Stores issuer-controlled jurisdiction codes and checks permitted code lists.

```text
set_jurisdiction(issuer, address, code)
is_permitted_jurisdiction(address, allowed_codes) -> bool
```
