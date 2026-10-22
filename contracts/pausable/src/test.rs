use crate::{
    is_paused, pause, pause_with_reason, paused_since, pause_reason, require_not_paused,
    require_paused, unpause,
};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{contract, contractimpl, Address, Env, String};

/// Minimal harness contract so we can exercise the pausable helpers inside a
/// real Soroban `Env`. All calls go through `env.register(Harness, ())`,
/// which gives each helper a proper contract context with instance storage.
#[contract]
struct Harness;

#[contractimpl]
impl Harness {
    pub fn is_paused(env: Env) -> bool {
        crate::is_paused(&env)
    }

    pub fn pause(env: Env) {
        crate::pause(&env);
    }

    pub fn pause_with_reason(env: Env, reason: Option<String>) {
        crate::pause_with_reason(&env, reason);
    }

    pub fn unpause(env: Env) {
        crate::unpause(&env);
    }

    pub fn paused_since(env: Env) -> Option<u64> {
        crate::paused_since(&env)
    }

    pub fn pause_reason(env: Env) -> Option<String> {
        crate::pause_reason(&env)
    }

    /// Calls `require_not_paused` and returns `true` if the call succeeds
    /// (i.e. the contract is not paused). Used to verify the happy path
    /// without a separate entry point.
    pub fn check_not_paused(env: Env) -> bool {
        crate::require_not_paused(&env);
        true
    }

    /// Calls `require_paused` and returns `true` if the call succeeds
    /// (i.e. the contract is currently paused).
    pub fn check_paused(env: Env) -> bool {
        crate::require_paused(&env);
        true
    }
}

// ─── is_paused default ──────────────────────────────────────────────────────

#[test]
fn test_is_paused_default_is_false() {
    let env = Env::default();
    let id = env.register(Harness, ());
    let client = HarnessClient::new(&env, &id);
    assert!(!client.is_paused());
}

// ─── pause ──────────────────────────────────────────────────────────────────

#[test]
fn test_pause_sets_flag() {
    let env = Env::default();
    let id = env.register(Harness, ());
    let client = HarnessClient::new(&env, &id);

    assert!(!client.is_paused());
    client.pause();
    assert!(client.is_paused());
}

#[test]
fn test_pause_is_idempotent() {
    let env = Env::default();
    let id = env.register(Harness, ());
    let client = HarnessClient::new(&env, &id);

    client.pause();
    client.pause(); // second call must not panic or corrupt state
    assert!(client.is_paused());
}

// ─── unpause ────────────────────────────────────────────────────────────────

#[test]
fn test_unpause_clears_flag() {
    let env = Env::default();
    let id = env.register(Harness, ());
    let client = HarnessClient::new(&env, &id);

    client.pause();
    assert!(client.is_paused());

    client.unpause();
    assert!(!client.is_paused());
}

#[test]
fn test_unpause_when_not_paused_is_noop() {
    let env = Env::default();
    let id = env.register(Harness, ());
    let client = HarnessClient::new(&env, &id);

    // Never paused — unpause must not panic
    client.unpause();
    assert!(!client.is_paused());
}

#[test]
fn test_pause_unpause_roundtrip() {
    let env = Env::default();
    let id = env.register(Harness, ());
    let client = HarnessClient::new(&env, &id);

    client.pause();
    assert!(client.is_paused());
    client.unpause();
    assert!(!client.is_paused());
    client.pause();
    assert!(client.is_paused());
}

// ─── require_not_paused ─────────────────────────────────────────────────────

#[test]
fn test_require_not_paused_succeeds_when_active() {
    let env = Env::default();
    let id = env.register(Harness, ());
    let client = HarnessClient::new(&env, &id);

    // Not paused — must not panic
    assert!(client.check_not_paused());
}

#[test]
#[should_panic]
fn test_require_not_paused_panics_when_paused() {
    let env = Env::default();
    let id = env.register(Harness, ());
    let client = HarnessClient::new(&env, &id);

    client.pause();
    // This call must panic because the contract is paused
    client.check_not_paused();
}

// ─── require_paused ─────────────────────────────────────────────────────────

/// `require_paused` should succeed (not panic) when the contract IS paused.
#[test]
fn test_require_paused_succeeds_when_paused() {
    let env = Env::default();
    let id = env.register(Harness, ());
    let client = HarnessClient::new(&env, &id);

    client.pause();
    // check_paused calls require_paused — must not panic
    assert!(client.check_paused());
}

/// `require_paused` must panic when the contract is NOT paused.
#[test]
#[should_panic]
fn test_require_paused_panics_when_not_paused() {
    let env = Env::default();
    let id = env.register(Harness, ());
    let client = HarnessClient::new(&env, &id);

    // Not paused — require_paused must panic
    client.check_paused();
}

/// Verify symmetry: require_not_paused and require_paused are strict inverses.
#[test]
fn test_require_paused_and_require_not_paused_are_symmetric() {
    let env = Env::default();
    let id = env.register(Harness, ());
    let client = HarnessClient::new(&env, &id);

    // Initially not paused — only check_not_paused must succeed.
    assert!(client.check_not_paused());

    // After pause — only check_paused must succeed.
    client.pause();
    assert!(client.check_paused());

    // After unpause — only check_not_paused must succeed again.
    client.unpause();
    assert!(client.check_not_paused());
}

// ─── paused_since ───────────────────────────────────────────────────────────

#[test]
fn test_paused_since_none_when_not_paused() {
    let env = Env::default();
    let id = env.register(Harness, ());
    let client = HarnessClient::new(&env, &id);

    assert!(client.paused_since().is_none());
}

#[test]
fn test_paused_since_set_on_pause() {
    let env = Env::default();
    let id = env.register(Harness, ());
    let client = HarnessClient::new(&env, &id);

    client.pause();
    // paused_since should be set to the ledger timestamp
    let ts = client.paused_since();
    assert!(ts.is_some());
    // The default test env timestamp is 0; just verify it is Some.
    assert_eq!(ts, Some(env.ledger().timestamp()));
}

#[test]
fn test_paused_since_cleared_on_unpause() {
    let env = Env::default();
    let id = env.register(Harness, ());
    let client = HarnessClient::new(&env, &id);

    client.pause();
    assert!(client.paused_since().is_some());

    client.unpause();
    assert!(client.paused_since().is_none());
}

#[test]
fn test_paused_since_preserved_on_idempotent_pause() {
    let env = Env::default();
    let id = env.register(Harness, ());
    let client = HarnessClient::new(&env, &id);

    client.pause();
    let ts_first = client.paused_since();

    // Second pause call must not overwrite the original timestamp
    client.pause();
    let ts_second = client.paused_since();
    assert_eq!(ts_first, ts_second);
}

// ─── pause_reason ───────────────────────────────────────────────────────────

#[test]
fn test_pause_reason_none_when_not_paused() {
    let env = Env::default();
    let id = env.register(Harness, ());
    let client = HarnessClient::new(&env, &id);

    assert!(client.pause_reason().is_none());
}

#[test]
fn test_pause_with_reason_stores_reason() {
    let env = Env::default();
    let id = env.register(Harness, ());
    let client = HarnessClient::new(&env, &id);

    let reason = String::from_str(&env, "emergency maintenance");
    client.pause_with_reason(&Some(reason.clone()));

    assert!(client.is_paused());
    assert_eq!(client.pause_reason(), Some(reason));
}

#[test]
fn test_pause_reason_cleared_on_unpause() {
    let env = Env::default();
    let id = env.register(Harness, ());
    let client = HarnessClient::new(&env, &id);

    let reason = String::from_str(&env, "security incident");
    client.pause_with_reason(&Some(reason));
    assert!(client.pause_reason().is_some());

    client.unpause();
    assert!(client.pause_reason().is_none());
}

#[test]
fn test_pause_no_reason_leaves_reason_none() {
    let env = Env::default();
    let id = env.register(Harness, ());
    let client = HarnessClient::new(&env, &id);

    client.pause(); // no reason supplied
    assert!(client.is_paused());
    assert!(client.pause_reason().is_none());
}
