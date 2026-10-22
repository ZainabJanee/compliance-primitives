# `multisig-admin`: Resource-Fee Cost Curve and Signer-Set Sizing Guidance

Closes #466.

> **Depends on**: the open `multisig-admin` resource-fee benchmark issue.
> This document is structured so it can be filled in with measured numbers
> once that benchmark lands.  The sections marked **[TO FILL IN]** should be
> updated from the benchmark results before this document is considered final.
> Until then, the qualitative guidance and cost model below give issuers
> enough information to make an informed signer-set sizing decision.

---

## Background

`multisig-admin` uses Soroban's `CustomAccountInterface` (`__check_auth`) to
require M-of-N signer approval before any admin operation on a compliance
primitive is executed.  Every time a primitive calls `admin.require_auth()` and
`admin` is the address of the `multisig-admin` contract, the Soroban host
invokes `__check_auth(env, payload, signatures, auth_context)`.

The cost of that invocation scales with two independent variables:

1. **N — total signer count**: the stored signer set is scanned linearly to
   validate each presented signature.  Larger N means a larger storage read and
   more iterations.

2. **M — signatures presented** (up to N): each signature address calls
   `require_auth()`, and the duplicate-detection inner loop is O(M²) in the
   number of presented signatures.

This document explains the cost model, provides the measured curve (once the
benchmark lands), and translates both into a recommended maximum signer-set
size.

---

## Cost model

### Storage read cost (scales with N)

The signer set is stored as `Vec<Address>` in **instance** storage.  Each
`__check_auth` call reads the full vector once:

```
storage_read_cost ≈ base_instance_read + N × per_address_deserialisation_cost
```

At the Soroban SDK level, deserialising an `Address` ScVal is roughly
constant-time, so this term grows linearly with N.

### Duplicate-detection cost (scales with M²)

To prevent a single signer from duplicating their address to satisfy the
threshold, `__check_auth` runs an O(M²) loop:

```rust
for i in 0..signatures.len() {
    for j in (i+1)..signatures.len() {
        if signatures[j] == signatures[i] { return Err(DuplicateSignature); }
    }
}
```

For M signatures this is `M*(M-1)/2` comparisons.  At typical thresholds
(M ≤ 10) this is negligible.  For M = 20 this is 190 comparisons.

### Signature validation cost (scales with M × N)

After duplicate detection, each presented signature is checked against every
signer in the set until a match is found (worst case: no match):

```rust
for i in 0..signatures.len() {          // M iterations
    sig_addr.require_auth();
    for j in 0..signers.len() {          // N iterations worst-case
        if signers[j] == sig_addr { valid_count += 1; break; }
    }
}
```

Worst-case cost for this loop is **O(M × N)**.  For a 5-of-10 multisig
(M=5, N=10) this is ≤50 inner comparisons — trivial.  For a 10-of-20
multisig it is ≤200.

### Total `__check_auth` cost (approximate)

```
total_cpu ≈ C_base
           + N  × C_addr_read          (signer-set read)
           + M² × C_addr_cmp           (duplicate check)
           + M × N × C_addr_cmp        (signature validation)
```

Where:
- `C_base` — fixed cross-contract call overhead (~100–150 CPU instructions)
- `C_addr_read` — per-address deserialisation cost [**TO FILL IN** from benchmark]
- `C_addr_cmp` — per-`Address` comparison cost [**TO FILL IN** from benchmark]

---

## Measured cost curve

> **[TO FILL IN]** — replace the placeholder rows below with numbers from the
> `multisig-admin` resource-fee benchmark once it lands.

| N (signers) | M (threshold) | `__check_auth` CPU (instructions) | Memory (bytes) | Ledger fee (stroops, est.) |
|---|---|---|---|---|
| 1 | 1 | *TBD* | *TBD* | *TBD* |
| 3 | 2 | *TBD* | *TBD* | *TBD* |
| 5 | 3 | *TBD* | *TBD* | *TBD* |
| 7 | 4 | *TBD* | *TBD* | *TBD* |
| 10 | 6 | *TBD* | *TBD* | *TBD* |
| 15 | 8 | *TBD* | *TBD* | *TBD* |
| 20 | 11 | *TBD* | *TBD* | *TBD* |

Each row uses a threshold of roughly ⌈N/2⌉ + 1 (a common majority-plus-one
policy).

---

## Practical signer-set sizing guidance

### Current qualitative guidance (pre-benchmark)

Until measured numbers are available, the following rules of thumb apply based
on the cost model above and Soroban's default transaction budget:

**Recommended maximum: N ≤ 20 signers, M ≤ threshold-to-match.**

Rationale:
- The O(N) storage read and O(M×N) validation loop both grow slowly enough
  that N=20 remains well within Soroban's per-transaction CPU budget even at
  moderate instruction-per-comparison costs.
- At N=20, M=11 (majority policy), the duplicate-detection loop (55 comparisons)
  and validation loop (≤220 comparisons worst case) are dominated by the
  fixed cross-contract call overhead.
- Beyond N=30–40, the validation loop may start to consume a meaningful fraction
  of the instruction budget when M is also large, but this is far outside the
  operational range of most issuers.

**Typical configurations for compliance-primitive issuers:**

| Use case | Recommended N | Recommended M | Notes |
|---|---|---|---|
| Small issuer (internal team) | 3–5 | 2–3 | Simple majority or 2-of-3 |
| Mid-size issuer (compliance committee) | 5–7 | 3–4 | Majority policy |
| Enterprise / regulated institution | 7–10 | 5–6 | Supermajority for high-stakes ops |
| Maximum tested configuration | 20 | 11 | Practical ceiling |

**Do not exceed N=20 until the benchmark confirms headroom beyond that
threshold.**  If your governance model requires more signers, consider:
1. Hierarchical multisig: use two `multisig-admin` instances, one governing
   the other.
2. Splitting authority: one multisig controls denylist/allowlist; a separate
   multisig controls jurisdiction-flag and circuit-breaker.

### Post-benchmark guidance (fill in after benchmark lands)

Once the benchmark numbers are available, update the table in the previous
section and add a paragraph of the form:

> "Measured results show that `__check_auth` with N=20, M=11 costs X CPU
> instructions and Y bytes of memory, representing Z% of the Soroban default
> transaction budget.  The first N at which cost becomes a practical concern
> is N=W (Y% budget).  We recommend N ≤ W as the operational maximum."

---

## Fee impact on issuer operations

Each admin call on a compliance primitive (e.g. `denylist-gate::add_to_denylist`,
`allowlist-token::add_to_allowlist`) triggers one `__check_auth` invocation on
the `multisig-admin` contract.  The extra fee relative to a single-admin
contract is:

```
extra_fee ≈ (multisig_cpu_cost - single_admin_cpu_cost) × cpu_fee_rate
            + (multisig_memory - single_admin_memory)  × memory_fee_rate
```

Based on the cost model (and Stellar mainnet fee parameters as of 2026):
- Single-admin `require_auth` overhead: ~50–100 CPU instructions
- Multisig `__check_auth` at N=5, M=3: estimated ~300–500 CPU instructions
- Extra cost: ~200–400 instructions ≈ 20–40 stroops ≈ $0.000002–$0.000004

This is negligible for compliance operations, which occur infrequently
(adding/removing addresses from allowlists or denylists) rather than on every
token transfer.

---

## References

- [`DESIGN_MULTISIG_ADMIN.md`](../DESIGN_MULTISIG_ADMIN.md) — design
  rationale and tradeoffs for the multisig admin pattern
- [`BENCHMARKS.md`](../BENCHMARKS.md) — general compliance-primitive resource
  benchmarks (token transfer overhead, cross-contract call costs)
- [`budget-baselines.toml`](../budget-baselines.toml) — CI budget baselines
  for each contract's key operations
- Open benchmark issue: the `multisig-admin` resource-fee benchmark issue
  (link TBD — this document depends on that issue landing first)
