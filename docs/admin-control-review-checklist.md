# Security review checklist: admin-control contracts

`multisig-admin` and `circuit-breaker` have a much larger blast radius
than the other primitives in this workspace: both are designed to sit in
the `admin`/`issuer` slot of one or more other contracts (or, for
`circuit-breaker`, to be checked before every gated transfer), so a bug in
either one doesn't just corrupt its own state — it can silently disable or
bypass compliance enforcement everywhere it's composed. Changes to these
two crates need a closer look than a read-only view function change
elsewhere in the workspace.

This checklist is a companion to [`SECURITY.md`](../SECURITY.md) (which
covers *reporting* a vulnerability) and to
[`CONTRIBUTING.md`](../CONTRIBUTING.md)'s general PR requirements. Use it
when reviewing (or opening) a PR that touches `contracts/multisig-admin`
or `contracts/circuit-breaker`.

## `multisig-admin`

- [ ] **Every state-mutating function still calls `require_auth()`.**
      `add_signer`, `remove_signer`, and `update_threshold` must each call
      `env.current_contract_address().require_auth()` so the change itself
      re-enters `__check_auth` and requires the *current* threshold — a
      new admin function that skips this would let a single caller change
      the signer set unilaterally.
- [ ] **`__check_auth` counts *distinct* signers, not signature entries.**
      The current implementation increments `valid_count` once per entry
      in the caller-supplied `signatures: Vec<Address>` that matches the
      stored signer set, without deduplicating that vector first. Confirm
      any change preserves (or explicitly adds) a check that the same
      signer address can't be listed more than once to inflate the count
      toward `threshold`.
- [ ] **`threshold` invariants hold after every mutation.** `initialize`,
      `remove_signer`, and `update_threshold` all validate
      `1 <= threshold <= signers.len()`. A new mutation path (e.g. a batch
      signer update) must re-validate this instead of trusting a stale
      threshold.
- [ ] **`signature_payload` / `auth_context` are still unused (or, if a
      change starts using them, that it's intentional).** The reference
      `__check_auth` ignores both — it authorizes based purely on signer
      identity, not on *what* is being authorized. If a change introduces
      context-specific policy (e.g. a higher threshold for
      `remove_signer` than for a primitive's `add_to_denylist`), verify it
      actually inspects `auth_context` rather than adding dead parameters.
- [ ] **No unbounded loops over `signers` or `signatures`.** Both are
      Soroban `Vec<Address>` values; a large signer set or an oversized
      `signatures` argument increases the resource cost of every
      authorization. Confirm there's a reasonable bound (or that adding
      one is considered) before merging a change that removes the
      informal "small signer set" assumption.
- [ ] **Test coverage includes a below-threshold and an at-threshold
      case**, plus a case with a duplicate signer address in `signatures`,
      for any change to `__check_auth` or the signer-set functions.

## `circuit-breaker`

- [ ] **`freeze`/`unfreeze` still require the stored admin's auth** via
      `require_admin`, and any new admin-mutating function does too.
- [ ] **Fail-open vs. fail-closed default is preserved or the change to it
      is explicit and called out in the PR description.** `is_frozen()`
      currently returns `false` (not frozen) via `unwrap_or(false)` when
      the contract has never been initialized — i.e. an undeployed or
      not-yet-initialized breaker **fails open**, not closed. Since
      consumers are expected to treat `is_frozen() == true` as "stop", a
      change that alters this default (or adds a new default-returning
      read path) has direct compliance-bypass implications and needs
      explicit sign-off, not just a passing test.
- [ ] **There is currently no admin-rotation function.** If a PR adds one
      (e.g. `set_admin`), confirm it requires the *current* admin's auth,
      consider whether in-flight `Frozen` state should be preserved across
      a rotation, and add the same test coverage `freeze`/`unfreeze` have
      (happy path + non-admin rejection). The existing two-step
      `propose_admin`/`accept_admin` flow now also has `cancel_admin_proposal`
      — verify cancellation requires the current admin and that accepting
      after cancellation correctly returns `NoPendingAdmin`.
- [ ] **Consumers that check the breaker do so *before* any state
      mutation.** This isn't enforced by the breaker contract itself, but
      a review of a `circuit-breaker`-consuming change should confirm the
      `is_frozen()` call happens before the consumer's own compliance
      checks or balance changes (see
      [`docs/emergency-freeze-design.md`](./emergency-freeze-design.md)),
      so a frozen breaker actually stops the transfer rather than merely
      logging that it should have.

## After the review

Note in the PR (or the review comment) which of the above you checked and
which don't apply to the change — this checklist is meant to speed up
review, not to require restating every line on every PR.
