# Primitive Selection Guide: Choosing the Right Compliance Composition

When building an RWA or stablecoin issuer on Stellar/Soroban, you'll need to compose one or more compliance primitives into your transfer logic. This guide helps you pick which primitives fit your use case and shows concrete examples for each.

## Quick Reference

| Use Case | Primitives | Example | Complexity |
|----------|-----------|---------|-----------|
| Simple ban list for high-risk addresses | `denylist-gate` | [denylist-gate-sep41](#denylist-only-minimal-compliance) | Trivial |
| KYC-verified addresses only | `allowlist-token` | [allowlist-token-usage](#allowlist-only-kyc-verified) | Low |
| Jurisdiction restrictions only | `jurisdiction-flag` | [jurisdiction-flag-consumer](#jurisdiction-only-regional) | Low |
| Denylist + jurisdiction restrictions | `denylist-gate` + `jurisdiction-flag` | [jurisdiction-denylist-consumer](#denylist--jurisdiction-regulated) | Medium |
| Full compliance: allowlist + denylist + jurisdiction | `allowlist-token` + `denylist-gate` + `jurisdiction-flag` | [rwa-token](#full-composition-maximum-compliance) | High |
| Dynamic policy (add/remove checks without redeploy) | `policy-engine` + primitives | See [Policy-Engine section](#policy-engine-dynamic-composition) | High |

## Decision Tree

```
START
  ↓
Do you need to ban specific addresses?
  ├─ NO  →  Do you need address KYC verification?
  │           ├─ NO  →  Do you need jurisdiction controls?
  │           │          ├─ NO  →  No primitives needed (unlikely for RWA)
  │           │          └─ YES →  jurisdiction-flag only
  │           └─ YES →  allowlist-token ± denylist-gate
  └─ YES →  denylist-gate
            Do you also need address KYC verification?
              ├─ NO  →  Just denylist-gate
              └─ YES →  denylist-gate + allowlist-token
```

## Use Case Profiles

### Denylist-Only: Minimal Compliance

**Scenario**: You have a small list of blocked addresses (sanctions, known fraud rings) but want to let most users transact freely. Typical for newer issuers or those with minimal regulatory burden.

**Primitives**: `denylist-gate`

**Key characteristics**:
- Fail-fast: reject blocked addresses immediately
- Simple admin: just add/remove addresses as needed
- No KYC or jurisdiction tracking
- Minimal overhead (~250 CPU instructions per transfer)

**Example**: [`denylist-gate-sep41`](../examples/denylist-gate-sep41) — a full SEP-41 token that gates transfers through a denylist check.

**When to use this**:
- You're required to have a sanctions list but no other compliance controls
- You're cost-optimizing for high-volume transfers
- You plan to upgrade to stronger compliance later

**Checklist**:
- [ ] Identify your initial blocklist (sanctions, known bad actors)
- [ ] Deploy `denylist-gate` contract
- [ ] Add initial addresses to the denylist
- [ ] Integrate `denylist_gate.check(address)` into your token's `transfer` function
- [ ] Set up off-chain monitoring to stay in sync with sanctions lists

---

### Allowlist-Only: KYC-Verified

**Scenario**: Only users who have passed KYC verification can hold and transfer your token. Common for regulated stablecoins or permissioned RWA platforms where you need tight control over the user base.

**Primitives**: `allowlist-token`

**Key characteristics**:
- Comprehensive control: only allowlisted addresses can send or receive
- Wrapper pattern: sits in front of an existing SEP-41 token
- Built-in admin: add/remove addresses with simple function calls
- Moderate overhead (~400 CPU instructions per transfer)

**Example**: [`allowlist-token-usage`](../examples/allowlist-token-usage) — a walkthrough of deploying and using the allowlist wrapper.

**When to use this**:
- Every user must pass KYC before being added to the allowlist
- You have direct control over the user list (not delegating to third parties)
- Transfer restrictions are customer-specific, not based on geography/sanctions

**Checklist**:
- [ ] Deploy the underlying SEP-41 token (your real token or a Stellar Asset Contract)
- [ ] Deploy `allowlist-token` pointing to that underlying token
- [ ] Establish a KYC pipeline (e.g., third-party KYC service)
- [ ] Approve users one by one or in batches using `add_to_allowlist`
- [ ] Communicate the gating to users (only allowlisted addresses can receive transfers)

---

### Jurisdiction-Only: Regional Restrictions

**Scenario**: You need to prevent users in certain jurisdictions from holding your token (e.g., due to export controls or local regulations), but you're not doing full KYC or maintaining a blocklist.

**Primitives**: `jurisdiction-flag`

**Key characteristics**:
- Flexible: attach ISO country codes (or custom codes) to addresses
- Check at policy time: verify sender's jurisdiction against a permitted list
- Scalable: hundreds of thousands of addresses without per-address admin calls
- Moderate overhead (~150 CPU instructions, typically checked during setup not every transfer)

**Example**: [`jurisdiction-flag-consumer`](../examples/jurisdiction-flag-consumer) — demonstrates jurisdiction checks in a consuming token contract.

**When to use this**:
- You need broad, geography-based restrictions
- You're working with a third-party jurisdiction provider (e.g., regulatory oracle)
- You want to scale to millions of addresses without manual admin overhead

**Checklist**:
- [ ] Identify the list of allowed jurisdictions (e.g., ["US", "CA", "SG"])
- [ ] Deploy `jurisdiction-flag` contract
- [ ] Set up address → jurisdiction mapping (batch or stream from an oracle)
- [ ] In your token's `transfer`, call `is_permitted_jurisdiction(from, allowed_codes)` and `is_permitted_jurisdiction(to, allowed_codes)`
- [ ] Plan for jurisdiction updates: users may relocate, regulations may change

---

### Denylist + Jurisdiction: Regulated Markets

**Scenario**: You need both a sanctions blocklist (reject known bad actors) and jurisdiction controls (permit only users in allowed regions). Common for stablecoins or RWA issuers serving regulated markets.

**Primitives**: `denylist-gate` + `jurisdiction-flag`

**Key characteristics**:
- Defense-in-depth: multiple layers of compliance
- Fail-fast: denylist check first (cheaper to reject bad actors), then jurisdiction
- Audit trail: separate events for each type of denial
- Combined overhead (~400 CPU instructions per transfer)

**Example**: [`jurisdiction-denylist-consumer`](../examples/jurisdiction-denylist-consumer) — composes both denylist and jurisdiction checks.

**When to use this**:
- You're required to maintain a sanctions list (legal mandate)
- You're also required to respect jurisdiction boundaries (e.g., OFAC + regional export controls)
- You need clarity on which gate blocked a transfer (for compliance reporting)

**Checklist**:
- [ ] Set up a denylist (start with OFAC SDN list)
- [ ] Set up jurisdiction flag data (from oracle or manual configuration)
- [ ] In your token's `transfer`, call:
  ```
  denylist.check(from)?;
  denylist.check(to)?;
  jurisdiction.is_permitted_jurisdiction(from, allowed)?;
  jurisdiction.is_permitted_jurisdiction(to, allowed)?;
  ```
- [ ] Plan for updates: maintain both denylist and jurisdiction mappings over time

---

### Full Composition: Maximum Compliance

**Scenario**: You're building a highly regulated RWA token and need every compliance layer: KYC verification (allowlist), sanctions checks (denylist), and jurisdiction controls. This is the "kitchen sink" approach and provides the strongest compliance posture.

**Primitives**: `allowlist-token` + `denylist-gate` + `jurisdiction-flag`

**Key characteristics**:
- Comprehensive: all compliance vectors covered
- Clear audit trail: different error types for each failure mode
- Highest overhead (~650 CPU instructions per transfer)
- Most complex to manage: three separate admin systems

**Example**: [`rwa-token`](../examples/rwa-token) — composes all three primitives in a single token contract.

**When to use this**:
- You're issuing a regulated security or stablecoin
- You have in-house compliance or legal teams overseeing the token
- You can afford the CPU overhead and want the strongest compliance defense

**Checklist**:
- [ ] Set up KYC pipeline and allowlist
- [ ] Set up OFAC/sanctions denylist
- [ ] Set up jurisdiction flag data
- [ ] Decide on check order (recommend: allowlist → denylist → jurisdiction)
- [ ] Plan for admin overhead: you'll be managing three systems
- [ ] Set up off-chain compliance monitoring for all three gates
- [ ] Document the policy for end users

---

## Policy-Engine: Dynamic Composition

If you need to adjust your compliance policy *without redeploying your token contract*, use the `policy-engine` contract. It lets you add, remove, or reorder compliance checks on-chain.

**Primitives**: `policy-engine` + any combination of (`denylist-gate`, `jurisdiction-flag`)

**Key characteristics**:
- Dynamic: change policy without redeploying
- Composable: add/remove checks without code changes
- Small overhead: ~5% more CPU than direct primitive calls
- Auditability: `PolicyResult` events show the full policy outcome

**Pattern**:
```rust
// Once (at setup):
policy_engine.initialize(admin, CombineOp::All);
policy_engine.add_check(admin, CheckKind::Denylist { contract: denylist_id });
policy_engine.add_check(admin, CheckKind::Jurisdiction { contract: jurisdiction_id, allowed_codes });

// On every transfer:
let passed = policy_engine.evaluate(from, to)?;
if !passed {
  return Err(Error::ComplianceCheckFailed);
}
```

**When to use this**:
- Your compliance requirements are likely to evolve
- You want to reduce code churn and redeployments
- You're managing multiple tokens with slightly different policies
- You value clean, auditable policy logs

---

## Comparison Matrix

| Factor | Denylist Only | Allowlist Only | Jurisdiction Only | Denylist + Jurisdiction | Full Composition | Policy-Engine |
|--------|---------------|----------------|--------------------|----------------------|------------------|---------------|
| **CPU overhead** | ~250 | ~400 | ~150 | ~400 | ~650 | ~420 (5% over direct) |
| **Admin complexity** | Low | Low | Medium | High | Very High | High |
| **Scalability** | High | Medium | Very High | Very High | Medium | Very High |
| **Audit trail** | Single gate | Single gate | Single gate | Two gates | Three gates | One engine |
| **Policy flexibility** | Low | Low | Low | Low | Low | Very High |
| **Maturity** | Production | Production | Production | Production | Production | Experimental |

---

## Implementation Roadmap

A common evolution for issuers:

1. **Start with denylist-only** (Phase 1, weeks 1-2)
   - Minimal compliance, minimal overhead
   - Satisfies basic sanctions requirements
   - Reference: `denylist-gate-sep41`

2. **Add KYC with allowlist** (Phase 2, weeks 3-4)
   - Wrap the token with `allowlist-token`
   - Set up KYC pipeline
   - Reference: `allowlist-token-usage`

3. **Add jurisdiction controls** (Phase 3, weeks 5-6)
   - Deploy `jurisdiction-flag`
   - Integrate into token's transfer logic alongside denylist
   - Reference: `jurisdiction-denylist-consumer`

4. **Full composition** (Phase 4, optional, weeks 7+)
   - If your target markets require it
   - Reference: `rwa-token`

5. **Policy-engine** (Phase 5, optional, ongoing)
   - Once policy stabilizes, migrate to policy-engine for flexibility
   - No token redeployment needed
   - Zero downtime policy updates

---

## FAQ

**Q: Can I mix these primitives with my own custom checks?**
A: Yes. The primitives are meant to be building blocks. You can add your own checks before or after the primitive calls, or use policy-engine as one input to a larger decision.

**Q: What if I need to change which primitives I'm using?**
A: For allowlist-only → denylist+allowlist, you can add denylist checks to your existing token contract. For more complex changes, policy-engine avoids redeployment. Otherwise, you'll need to migrate (see [MIGRATION.md](./MIGRATION.md)).

**Q: Can I use allowlist-token with policy-engine?**
A: Not directly — allowlist-token is a wrapper token contract, not a primitive gate. Policy-engine works with denylist-gate and jurisdiction-flag. If you want allowlist + policy-engine composition, either use allowlist-token standalone or implement allowlist logic inside policy-engine (outside the scope of these primitives).

**Q: How do I stay in sync with sanctions lists?**
A: Set up an off-chain monitoring system that polls your denylist contract and compares it against authoritative sources (e.g., OFAC SDN list). Update your denylist regularly (weekly recommended). See SECURITY.md for considerations.

**Q: Will using primitives freeze my token's behavior?**
A: No. You can always add more primitives later, or migrate to policy-engine if you want flexibility without redeployment.

---

## Multisig Governance: Standalone vs. Built-in

This workspace provides two independent M-of-N multisig implementations. Nothing
in the code explains when to use one over the other, so this section does.

### The two implementations

**`multisig-admin` (standalone contract)**

`multisig-admin` is a separate deployable contract that implements Soroban's
`CustomAccountInterface`. When it is set as the `admin` address of any compliance
primitive (`allowlist-token`, `denylist-gate`, `jurisdiction-flag`, etc.), the host
automatically calls `MultisigAdmin::__check_auth` whenever the primitive calls
`admin.require_auth()`. The primitive itself requires no modification.

It also provides a `propose`/`approve`/`execute` workflow for staged,
multi-step governance actions.

**`denylist-gate` built-in multisig (issue #26 mode)**

`denylist-gate` has its own `initialize_multisig`/`add_signer`/`remove_signer`
entrypoints baked directly into the contract. Admin calls (`add_to_denylist`,
`remove_from_denylist`) verify M-of-N inline, without a cross-contract call.

### Tradeoff table

| Aspect | `multisig-admin` (standalone) | Built-in multisig (e.g. denylist-gate) |
|---|---|---|
| Primitive changes needed | **None** — works with any contract that uses `require_auth()` | Each primitive must be modified to add the logic |
| Reusability | **One deployment governs all primitives** | Per-primitive; each contract manages its own signer set independently |
| Extra deployment | Yes — one extra contract address to deploy and fund | None |
| Upgrade path | Swap the `admin` address on each governed primitive | Redeploy the primitive with updated built-in logic |
| Auth overhead | One cross-contract call per admin operation | Inline — no cross-contract hop |
| Proposal workflow | Yes — `propose`/`approve`/`execute` with expiry | Not available in the built-in mode |
| Audit trail | On-chain events from the standalone contract | Events only at the primitive level |

### When to use `multisig-admin` (standalone)

- You want **a single multisig policy** to govern multiple primitives
  (e.g. the same 3-of-5 set controls `allowlist-token`, `denylist-gate`,
  and `jurisdiction-flag` simultaneously).
- You don't control the primitive's source code, or you prefer not to fork
  and maintain a modified version.
- You want the `propose`/`approve`/`execute` staged workflow so governance
  actions can be reviewed before they take effect.
- You need a clean, reusable audit trail at the governance layer, separate
  from the individual primitive's event stream.

### When to use built-in multisig (e.g. `denylist-gate`'s own mode)

- You are governing **only one primitive** and the extra deployment overhead
  is not worth the added flexibility.
- You want the tightest possible integration — no cross-contract call, lower
  compute cost per admin transaction.
- You control the primitive's source and are comfortable modifying and
  redeploying it when the signer set or threshold changes.
- You do not need a staged proposal workflow.

### Are these meant to converge?

No. They serve different points on the flexibility/simplicity spectrum and can
coexist: for example, you could govern `allowlist-token` and `jurisdiction-flag`
through a single `multisig-admin` instance while letting `denylist-gate` manage
its own built-in signer set separately.

The two implementations will not be merged into one; the standalone approach is
architecturally cleaner for multi-primitive governance, while the built-in approach
remains the right choice for single-primitive deployments that prioritize simplicity.

---

## Next Steps

1. Identify your regulatory requirements (KYC? Sanctions? Jurisdictions?)
2. Pick the matching use case profile above
3. Follow the checklist for your profile
4. Reference the example code
5. Test thoroughly on testnet before mainnet

Questions? [Open an issue](https://github.com/stellar-compliance-kit/compliance-primitives/issues).
