# ComplianceCheck Trait Design

> **Status: Proposed — not yet implemented.**
>
> This document describes a *draft* design for a shared `ComplianceCheck`
> trait. **No contract in this workspace currently implements
> `is_compliant`.** The three compliance primitives (`allowlist-token`,
> `denylist-gate`, `jurisdiction-flag`) each expose their own check
> function (`is_allowed`, `check`, and `is_permitted_jurisdiction`
> respectively), and there is no `contracts/compliance-check` crate.
> See [#635](https://github.com/stellar-compliance-kit/compliance-primitives/issues/635)
> for the tracking issue.

## Overview

This document describes the design for a shared `ComplianceCheck` trait
that would provide a unified interface across the three compliance
primitives (`allowlist-token`, `denylist-gate`, `jurisdiction-flag`).

## Problem Statement

The three compliance contracts have inconsistent calling conventions:

- `allowlist-token.is_allowed(address) -> bool`
- `denylist-gate.check(address) -> bool`
- `jurisdiction-flag.is_permitted_jurisdiction(address, allowed_codes) -> bool`

This inconsistency makes it difficult for external contracts to compose
these primitives polymorphically. The `jurisdiction-flag` in particular
requires additional parameters (`allowed_codes`), making it impossible
to call all three with the same call pattern.

## Proposed Solution: ComplianceCheck Trait

### Design Approach

Introduce a shared `ComplianceCheck` trait that standardizes the check
interface across the three contracts:

```rust
pub trait ComplianceCheck {
    /// Check if an address passes this compliance check.
    /// Returns true if the address is compliant, false otherwise.
    fn is_compliant(env: Env, address: Address) -> bool;
}
```

### Key Design Decisions

1. **Additive Implementation**: The new trait would be added WITHOUT
   modifying existing contract functions. All current contracts maintain
   backward compatibility.

2. **Minimum Interface**: The trait uses only the most minimal information
   (`address -> bool`) to work universally across all three primitives.

3. **Address-Only Semantics**:
   - `allowlist-token`: returns `is_allowed(address)`
   - `denylist-gate`: returns `check(address)`
   - `jurisdiction-flag`: `is_compliant` returns `true` if the address
     has ANY jurisdiction code set (indicating the address has been
     flagged/verified). It does **not** check against a permitted-codes
     list — that remains the role of `is_permitted_jurisdiction`. Note
     that this is purely a "has a jurisdiction been assigned?" check, not
     a "is the assigned jurisdiction permitted?" check.

### Alternatives Considered

1. **Generic Trait with Associated Types**: Would be more flexible but
   requires type complexity at call sites.
   - **Rejected**: Soroban contracts don't support generic trait
     implementations well.

2. **Trait with Multiple Methods**: Could have both `is_compliant` and
   specialized check methods.
   - **Rejected**: Adds complexity; the goal is a unified interface.

3. **Wrapper Contracts**: Create separate wrapper contracts around each
   primitive.
   - **Rejected**: Adds deployment overhead and makes debugging harder.

4. **Runtime Polymorphism via Enum**: Pass contract type as enum parameter.
   - **Rejected**: Less idiomatic Rust; doesn't leverage Soroban's
     contract system.

## Proposed Implementation Strategy

### Phase 1: Add Trait Definition

Create a new shared module or keep in existing library code that defines
the trait. This would live in a new `contracts/compliance-check` crate
(or as part of an existing crate behind a Cargo feature flag).

### Phase 2: Implement for Each Contract

Add `#[contractimpl]` blocks that implement the trait for each contract:
- `AllowlistToken` — delegates to `is_allowed`
- `DenylistGate` — delegates to `check`
- `JurisdictionFlag` — returns `true` if any code is set (see
  "Address-Only Semantics" above)

### Phase 3: Client Generation

Use `#[contractclient]` to generate clients that can call the
`is_compliant` function on any of the three contracts.

### Phase 4: Integration Example

Update `rwa-compliance-flow` example to demonstrate:

```rust
// All three can be called with the same pattern:
let allowlist_ok = allowlist_client.is_compliant(&alice);
let denylist_ok  = denylist_client.is_compliant(&alice);
let jurisdiction_ok = jurisdiction_client.is_compliant(&alice);

// All pass -> address is compliant
if allowlist_ok && denylist_ok && jurisdiction_ok {
    // Safe to transfer
}
```

## Current State

None of the three contracts implement `is_compliant` today. The
`rwa-compliance-flow` example defines `ComplianceCheckInterface` as a
`#[contractclient]` trait (an ABI descriptor for cross-contract calls),
but this is **not** an implementation — it is a client stub that would
only work if the target contract actually exposes an `is_compliant`
function.

## Backward Compatibility (when implemented)

- ✅ All existing contract functions (`is_allowed`, `check`,
  `is_permitted_jurisdiction`) would remain unchanged
- ✅ All existing tests would continue to pass without modification
- ✅ The new `is_compliant` method is purely additive
- ✅ Calling contracts can use either the unified interface or the
  existing specialized functions

## Trade-offs

### Simplicity vs. Flexibility

The unified `(address) -> bool` interface sacrifices some flexibility
(e.g., `jurisdiction-flag` loses access to permitted-codes list) for:
- Easier composition
- Uniform calling convention
- Reduced cognitive load for integrators

### Jurisdiction-Flag Special Case

`jurisdiction-flag` requires additional context (permitted codes) that
doesn't fit the minimal interface. The proposed behavior is:
- `is_compliant` returns `true` if the address has ANY jurisdiction code
  set (i.e., the address has been KYC'd/verified with some jurisdiction)
- The check for specific permitted codes remains in
  `is_permitted_jurisdiction`

This is different from `is_permitted_jurisdiction` — it answers "has
this address been assigned a jurisdiction?" not "is its jurisdiction on
the allowlist?".

## Future Enhancements

1. **Permit Storage**: Allow contracts to store permitted jurisdiction
   lists and have `is_compliant` check against them.
2. **Metadata**: Extend trait to include a method returning compliance
   check type/description.
3. **Composable Checks**: Define a `ComplianceCheckCompositor` trait
   for AND/OR logic.
4. **Event Standardization**: Standard events emitted on compliance
   check failures.

## Testing Strategy (when implemented)

- Verify each contract's `is_compliant` behaves consistently with its
  specialized check
- Test that external contracts can call any of the three through the
  unified interface
- Ensure all existing tests still pass
- Add new tests specifically for the `is_compliant` method on each
  contract
