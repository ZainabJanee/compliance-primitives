//! Fuzz harness for the delegated-signature path of `add_to_allowlist_delegated`.
//!
//! # Approach
//!
//! Like `denylist-gate`'s `fuzz_test.rs`, this is an in-process harness (plain
//! `#[test]` plus a seeded xorshift PRNG) rather than `cargo-fuzz`: a
//! `#![no_std]` Soroban contract crate cannot easily host a libFuzzer target.
//!
//! # Invariant
//!
//! After a valid `set_delegated_admin_key`, feeding arbitrary `nonce`, `expiry`
//! and `signature` values into `add_to_allowlist_delegated` must only ever
//! produce success or an intended contract [`Error`] — never a host trap or
//! panic — and:
//! - an arbitrary (unsigned) signature never allowlists the target;
//! - a correctly signed request succeeds iff `expiry > now` and
//!   `nonce > last accepted nonce`, otherwise it fails with `ExpiredSignature`
//!   or `InvalidNonce` respectively;
//! - a correctly signed request with any single flipped signature bit fails
//!   with `NotAuthorized` (once expiry and nonce checks pass) and changes no
//!   state.
//!
//! # Running
//!
//! ```sh
//! cargo test -p allowlist-token fuzz_delegated
//! ```

use super::*;
use ed25519_dalek::SigningKey;
use soroban_sdk::testutils::ed25519::Sign;
use soroban_sdk::testutils::{Address as _, Ledger as _};
use soroban_sdk::{Address, BytesN, Env};

const DEFAULT_ITERATIONS: u32 = 200;
const SEQUENCE_LEN: usize = 16;

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }

    fn bytes64(&mut self) -> [u8; 64] {
        let mut out = [0u8; 64];
        for chunk in out.chunks_mut(8) {
            chunk.copy_from_slice(&self.next_u64().to_le_bytes());
        }
        out
    }

    /// A random `u64`, biased towards boundary values.
    fn boundary_u64(&mut self, now: u64) -> u64 {
        match self.below(10) {
            0 => 0,
            1 => 1,
            2 => u64::MAX,
            3 => u64::MAX - 1,
            4 => i64::MAX as u64,
            5 => now,
            6 => now.saturating_add(1),
            7 => now.saturating_sub(1),
            _ => self.next_u64(),
        }
    }
}

fn signing_key(seed: u8) -> SigningKey {
    let mut bytes = [0u8; 32];
    for (i, b) in bytes.iter_mut().enumerate() {
        *b = seed.wrapping_add(i as u8);
    }
    SigningKey::from_bytes(&bytes)
}

fn setup(env: &Env, key: &SigningKey) -> (Address, AllowlistTokenClient<'_>) {
    env.mock_all_auths();
    let admin = Address::generate(env);
    let token = Address::generate(env);
    let contract_id = env.register(AllowlistToken, ());
    let client = AllowlistTokenClient::new(env, &contract_id);
    client.initialize(&admin, &token);
    let pubkey = BytesN::from_array(env, &key.verifying_key().to_bytes());
    client.set_delegated_admin_key(&admin, &pubkey);
    (admin, client)
}

fn sign(env: &Env, key: &SigningKey, target: &Address, nonce: u64, expiry: u64) -> [u8; 64] {
    let action = Symbol::new(env, "add_to_allowlist");
    let message = AllowlistToken::delegated_action_message(env, target, &action, nonce, expiry);
    key.sign(&message).unwrap()
}

/// Intended failures of `add_to_allowlist_delegated` after key configuration.
fn is_intended_error(e: Error) -> bool {
    matches!(
        e,
        Error::NotAuthorized | Error::InvalidNonce | Error::ExpiredSignature
    )
}

#[test]
fn fuzz_delegated_arbitrary_signature_never_authorizes() {
    let mut rng = Rng::new(0x9E37_79B9_7F4A_7C15);
    let key = signing_key(0);

    for _ in 0..DEFAULT_ITERATIONS {
        let env = Env::default();
        let now = rng.below(1_000);
        env.ledger().set_timestamp(now);
        let (admin, client) = setup(&env, &key);
        let target = Address::generate(&env);

        let nonce = rng.boundary_u64(now);
        let expiry = rng.boundary_u64(now);
        let signature = BytesN::from_array(&env, &rng.bytes64());

        match client.try_add_to_allowlist_delegated(&admin, &target, &nonce, &expiry, &signature) {
            Ok(Ok(())) => panic!("arbitrary signature accepted (nonce={nonce}, expiry={expiry})"),
            Ok(Err(e)) => panic!("unexpected conversion error: {e:?}"),
            Err(Ok(e)) => assert!(is_intended_error(e), "unexpected contract error {e:?}"),
            Err(Err(e)) => panic!("host trap for nonce={nonce}, expiry={expiry}: {e:?}"),
        }
        assert!(!client.is_allowed(&target));
    }
}

#[test]
fn fuzz_delegated_signed_requests_follow_expiry_and_nonce_rules() {
    let mut rng = Rng::new(0xD1B5_4A32_D192_ED03);
    let key = signing_key(7);

    for _ in 0..DEFAULT_ITERATIONS {
        let env = Env::default();
        let now = rng.below(1_000);
        env.ledger().set_timestamp(now);
        let (admin, client) = setup(&env, &key);
        let mut last_nonce = 0u64;

        for _ in 0..SEQUENCE_LEN {
            let target = Address::generate(&env);
            let nonce = rng.boundary_u64(now);
            let expiry = rng.boundary_u64(now);
            let signature = BytesN::from_array(&env, &sign(&env, &key, &target, nonce, expiry));

            let result =
                client.try_add_to_allowlist_delegated(&admin, &target, &nonce, &expiry, &signature);

            if expiry <= now {
                assert_eq!(result, Err(Ok(Error::ExpiredSignature)));
                assert!(!client.is_allowed(&target));
            } else if nonce <= last_nonce {
                assert_eq!(result, Err(Ok(Error::InvalidNonce)));
                assert!(!client.is_allowed(&target));
            } else {
                assert_eq!(result, Ok(Ok(())), "nonce={nonce}, expiry={expiry}, now={now}");
                assert!(client.is_allowed(&target));
                last_nonce = nonce;
            }
        }
    }
}

#[test]
fn fuzz_delegated_tampered_signature_is_rejected_without_state_change() {
    let mut rng = Rng::new(0x1234_5678_9ABC_DEF1);
    let key = signing_key(21);

    for _ in 0..DEFAULT_ITERATIONS {
        let env = Env::default();
        let now = rng.below(1_000);
        env.ledger().set_timestamp(now);
        let (admin, client) = setup(&env, &key);
        let target = Address::generate(&env);

        let nonce = 1 + rng.below(1_000_000);
        let expiry = now + 1 + rng.below(1_000_000);
        let mut raw = sign(&env, &key, &target, nonce, expiry);
        let bit = rng.below(64 * 8) as usize;
        raw[bit / 8] ^= 1 << (bit % 8);
        let signature = BytesN::from_array(&env, &raw);

        let result =
            client.try_add_to_allowlist_delegated(&admin, &target, &nonce, &expiry, &signature);
        assert_eq!(result, Err(Ok(Error::NotAuthorized)), "flipped bit {bit}");
        assert!(!client.is_allowed(&target));

        // The rejected attempt must not have consumed the nonce: the untampered
        // signature for the same nonce still works.
        let good = BytesN::from_array(&env, &sign(&env, &key, &target, nonce, expiry));
        client.add_to_allowlist_delegated(&admin, &target, &nonce, &expiry, &good);
        assert!(client.is_allowed(&target));
    }
}
