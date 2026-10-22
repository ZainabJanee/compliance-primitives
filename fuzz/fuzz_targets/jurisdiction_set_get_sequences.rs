#![no_main]

//! Byte-driven counterpart of `fuzz_jurisdiction_set_get_sequences` in
//! `contracts/jurisdiction-flag/src/fuzz.rs`.
//!
//! Each 2-byte chunk is one `set_jurisdiction` call: `chunk[0] % 5` picks the
//! address and `chunk[1] % 7` picks the code. The oracle is last-write-wins
//! plus `is_permitted_jurisdiction` consistency, as in the in-crate harness.

use jurisdiction_flag::{JurisdictionFlag, JurisdictionFlagClient};
use libfuzzer_sys::fuzz_target;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{vec, Address, Env, String, Vec};

const CODES: [&str; 7] = ["US", "CA", "GB", "DE", "JP", "FR", "AU"];
const PERMITTED: [&str; 3] = ["US", "CA", "GB"];
const MAX_ADDRESSES: usize = 5;
const MAX_OPS: usize = 64;

fuzz_target!(|data: &[u8]| {
    let env = Env::default();
    env.mock_all_auths();

    let issuer = Address::generate(&env);
    let contract_id = env.register(JurisdictionFlag, ());
    let client = JurisdictionFlagClient::new(&env, &contract_id);
    client.initialize(&issuer);

    let addresses: [Address; MAX_ADDRESSES] = core::array::from_fn(|_| Address::generate(&env));
    let codes: [String; 7] = CODES.map(|code| String::from_str(&env, code));
    let allowed_codes: Vec<String> = vec![
        &env,
        String::from_str(&env, PERMITTED[0]),
        String::from_str(&env, PERMITTED[1]),
        String::from_str(&env, PERMITTED[2]),
    ];

    // Model: last set code per address index (None until first write).
    let mut model: [Option<usize>; MAX_ADDRESSES] = [None; MAX_ADDRESSES];

    for chunk in data.chunks_exact(2).take(MAX_OPS) {
        let addr_i = chunk[0] as usize % MAX_ADDRESSES;
        let code_i = chunk[1] as usize % codes.len();
        client.set_jurisdiction(&issuer, &addresses[addr_i], &codes[code_i]);
        model[addr_i] = Some(code_i);
    }

    for (addr_i, address) in addresses.iter().enumerate() {
        let got = client.get_jurisdiction(address);
        match model[addr_i] {
            None => assert_eq!(got, None, "addr={addr_i}: expected None after no writes"),
            Some(code_i) => assert_eq!(
                got,
                Some(codes[code_i].clone()),
                "addr={addr_i}: last-write-wins mismatch"
            ),
        }

        let permitted = client.is_permitted_jurisdiction(address, &allowed_codes);
        let expected_permitted = match &got {
            Some(code) => allowed_codes.iter().any(|c| c == *code),
            None => false,
        };
        assert_eq!(
            permitted, expected_permitted,
            "addr={addr_i}: is_permitted_jurisdiction inconsistent with get"
        );
    }
});
