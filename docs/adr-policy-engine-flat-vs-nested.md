# ADR: flat vs nested policy tree in `policy-engine`

**Status**: accepted  
**Date**: 2026-09  
**Issue**: [#410](https://github.com/stellar-compliance-kit/compliance-primitives/issues/410)

---

## Context

`policy-engine` evaluates a compliance decision for a proposed transfer by
running a list of checks (denylist, jurisdiction, allowlist) against both the
sender and recipient, then combining their results with a single
`CombineOp` — either `All` (every check must pass) or `Any` (at least one
must pass).

This is an intentionally **flat** design: one operator, one list.  It is
documented in the module-level doc comment:

> A two-variant `CombineOp` enum (`All` / `Any`) covers both without the
> overhead of an AST, which would be over-engineered for this domain and
> would add significant complexity to the storage, serialization, and
> auditing story.

As the workspace has grown to nine primitives and multiple composition
examples, contributors have periodically asked whether a **nested/grouped**
policy tree would be a better fit — for example, allowing:

```
(denylist AND jurisdiction) OR circuit-breaker-clear
```

rather than the current flat equivalent, which would require two separate
policy-engine instances composed by the caller.

This ADR captures the decision so future proposals have a documented baseline
to argue against or build on, rather than re-litigating the tradeoff from
scratch each time.

---

## Decision

**Keep the flat model.**  A single `CombineOp` over a flat `Vec<CheckKind>` is
the right shape for this contract.  Nested/grouped trees are not added.

---

## Rationale

### 1. Real compliance stacks rarely need nesting

Every realistic compliance requirement seen in the RWA and stablecoin space
reduces to one of two shapes:

- **AND**: "this address must satisfy all of: denylist clear, permitted
  jurisdiction, and KYC provider A".
- **OR**: "this address must satisfy at least one of: KYC provider A, KYC
  provider B" (dual-provider redundancy).

Mixed-depth trees like `(A AND B) OR (C AND D)` are theoretically possible
but do not map to any real-world compliance requirement the workspace
currently models.  When a real case arises that genuinely requires depth-2
logic, the right response is to model it explicitly (see §4 below), not to
add a general tree to every policy instance.

### 2. Nested trees complicate on-chain storage and serialization

Soroban's `#[contracttype]` macro serializes types as XDR.  A recursive or
self-referencing type like:

```rust
// ❌ Not supported by #[contracttype] — XDR requires fixed-size types
pub enum PolicyNode {
    Check(CheckKind),
    Group { op: CombineOp, children: Vec<PolicyNode> },
}
```

is not directly expressible because XDR does not support recursive types.  A
depth-bounded workaround (e.g. `Vec<Vec<CheckKind>>`) is possible but removes
the structural clarity that makes the flat model easy to audit: you would need
to know the intended nesting depth to interpret a serialized policy tree.

### 3. Nested trees complicate auditing and off-chain tooling

Issuers and auditors reading a deployed policy must be able to reconstruct the
full logic from `get_policy()`.  A flat list of checks with a single operator
is unambiguous.  A nested tree adds combinatorial depth that must be traversed
to determine what the policy actually evaluates — increasing the surface area
for misinterpretation and reducing the value of the on-chain policy record.

The existing `PolicyResult` event already logs every `evaluate` decision.  The
simpler the stored policy, the easier it is to reconcile an emitted event with
the policy that produced it.

### 4. Composition at the caller level is the right decomposition point

When a caller genuinely needs `(A AND B) OR C` semantics, the correct
approach is:

1. Deploy two policy-engine instances: `engine_ab` (All) for `A AND B` and
   `engine_c` (Any / single check) for the fallback `C`.
2. In the caller's contract, evaluate `engine_ab` first; if it fails, fall
   through to `engine_c`.

This keeps each policy-engine instance simple, independently auditable, and
independently upgradeable.  It also means neither instance needs to know about
the other — a cleaner separation of concerns than encoding the relationship
inside a single contract's storage.

The `compliance-aggregator` contract already demonstrates this composition
pattern for batching multiple primitive calls.

### 5. The circuit-breaker already handles the most common "nested override" case

The most frequently requested nesting use case is:

> "Bypass all checks if the circuit-breaker is frozen."

This is already handled by the `circuit-breaker` integration in `evaluate`:
when a configured circuit-breaker is frozen, `evaluate` returns `false`
immediately, regardless of the check list or `CombineOp`.  This provides the
operational override semantics without requiring a general tree.

---

## Consequences

- **Adding a new check type** (e.g. a new primitive) remains straightforward:
  add a new `CheckKind` variant and a matching `#[contracttype]` wrapper struct
  (per the naming convention documented in issue #408).
- **Callers who need depth-2 logic** must compose two policy-engine instances
  themselves.  This is a deliberate constraint: it pushes complexity into the
  caller, where it is visible and auditable, rather than hiding it in the
  storage format.
- **Future proposals to add nesting** should address the XDR serialization
  constraint (§2), the auditing complexity (§3), and provide a concrete
  compliance requirement that cannot be modelled by caller-level composition
  (§4).  Without all three, the flat model remains the right choice.

---

## Alternatives considered

### A. Depth-2 groups: `Vec<Vec<CheckKind>>` with two `CombineOp`s

Would support `(A AND B) OR C` style logic.  Rejected because:

- Doubles the storage layout complexity.
- The outer-group/inner-group distinction is not self-evident from the type.
- `get_policy()` would need to return a more complex type, breaking existing
  off-chain tooling and the `PolicyNode` structure.

### B. A tagged `PolicyTree` enum with `Leaf` and `Node` variants

A fully general tree.  Rejected because of the XDR recursion limitation (§2)
and the auditing concerns (§3).  Also rejected on the grounds that no current
use case requires more than two levels.

### C. Two separate `CombineOp` fields (inner and outer)

`CombineOp` for individual checks, plus a second `CombineOp` for groups of
checks.  Rejected for the same reasons as alternative A, with the additional
problem that the grouping structure would still need to be encoded somehow,
reintroducing the same complexity.

---

## References

- Module-level doc comment in `contracts/policy-engine/src/lib.rs` (§ AND/OR
  design choice).
- `contracts/compliance-aggregator` — caller-level composition pattern.
- `contracts/circuit-breaker` — operational override without tree nesting.
- Issue #407 (`swap_checks`) — reordering checks to optimize short-circuit cost
  without needing nested groups.
- Issue #408 — `#[contracttype]` named-field-variant limitation (informs the
  XDR constraint in §2 above).
- Issue #409 — short-circuit benchmark (quantifies the cost of full evaluation
  vs. early exit, motivating check ordering rather than tree depth).
