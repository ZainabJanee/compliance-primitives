//! # compliance-check
//!
//! A `#![no_std]` crate that defines the shared [`ComplianceCheck`] trait used
//! for **compile-time composition** of Soroban compliance contracts.
//!
//! ## Distinction from `ComplianceCheckInterface` in `rwa-compliance-flow`
//!
//! There are two related but purposefully different abstractions in this workspace:
//!
//! | Abstraction | Where defined | Mechanism | Use case |
//! |---|---|---|---|
//! | [`ComplianceCheck`] (this crate) | `contracts/compliance-check` | Plain Rust trait — no `#[contractclient]` | Compile-time composition: unit tests, generic helpers, same-crate callers |
//! | `ComplianceCheckInterface` | `examples/rwa-compliance-flow` | `#[contractclient]` macro | Real cross-contract XDR calls at runtime |
//!
//! **`ComplianceCheck`** (this trait) is for code that _imports_ the contract
//! crate directly (e.g. integration tests or other contracts in the same build)
//! and calls the method as an ordinary Rust function.  Because it requires no
//! XDR or host-function overhead it is the right choice for unit tests and
//! in-process generic helpers such as `fn check_all(checks: &[impl ComplianceCheck])`.
//!
//! **`ComplianceCheckInterface`** (in `rwa-compliance-flow`) uses the
//! `#[contractclient]` macro to generate an XDR-based client stub suitable for
//! real cross-contract invocations on Soroban — i.e. calling a _deployed_
//! contract from another deployed contract.  It cannot be used for compile-time
//! generics.
//!
//! See `examples/rwa-compliance-flow/src/lib.rs` for the `ComplianceCheckInterface`
//! definition and the detailed rationale for keeping the two separate.
//!
//! ## Usage
//!
//! ```ignore
//! use compliance_check::ComplianceCheck;
//!
//! // A generic helper that works with ANY compliance contract:
//! fn all_pass<C: ComplianceCheck>(env: &Env, check: &C, address: &Address) -> bool {
//!     check.is_compliant(env, address.clone())
//! }
//! ```
#![no_std]

use soroban_sdk::{Address, Env};

/// Shared compile-time trait for compliance contracts that expose a simple
/// `is_compliant(address) -> bool` check.
///
/// Implementing this trait on a contract struct allows generic callers (tests,
/// other contracts linked at compile time) to use all three compliance
/// primitives interchangeably without cross-contract XDR overhead.
///
/// ## Implementing contracts
///
/// - `allowlist-token` — returns `is_allowed(address)`
/// - `denylist-gate`   — returns `check(address)` (true = not denied)
/// - `jurisdiction-flag` — returns whether `address` has a permitted
///   jurisdiction code (uses the contract's stored default allowed list)
///
/// ## Difference from `ComplianceCheckInterface`
///
/// `ComplianceCheckInterface` in `examples/rwa-compliance-flow` is a
/// `#[contractclient]`-backed interface for _runtime_ cross-contract calls.
/// This trait is the _compile-time_ counterpart for in-process composition.
pub trait ComplianceCheck {
    /// Returns `true` if `address` passes this compliance check, `false`
    /// otherwise.
    fn is_compliant(env: &Env, address: Address) -> bool;
}

#[cfg(test)]
mod tests;
