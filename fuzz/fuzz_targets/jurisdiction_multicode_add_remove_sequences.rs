#![no_main]

//! Byte-driven counterpart of `fuzz_jurisdiction_multicode_add_remove_sequences`
//! in `contracts/jurisdiction-flag/src/fuzz.rs`.
//!
//! Each 2-byte chunk is one op: `chunk[0] % 7` picks the code and the low bit
//! of `chunk[1]` picks add (0) or remove (1). After every op the listed codes
//! must match the membership model exactly.

use jurisdiction_flag::{JurisdictionFlag, JurisdictionFlagClient};
use libfuzzer_sys::fuzz_target;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, Env, String};

const CODES: [&str; 7] = ["US", "CA", "GB", "DE", "JP", "FR", "AU"];
const MAX_OPS: usize = 64;

fuzz_target!(|data: &[u8]| {
    let env = Env::default();
    env.mock_all_auths();

    let issuer = Address::generate(&env);
    let contract_id = env.register(JurisdictionFlag, ());
    let client = JurisdictionFlagClient::new(&env, &contract_id);
    client.initialize(&issuer);

    let address = Address::generate(&env);
    let codes: [String; 7] = CODES.map(|code| String::from_str(&env, code));
    let mut model = [false; 7];

    for (op, chunk) in data.chunks_exact(2).take(MAX_OPS).enumerate() {
        let code_i = chunk[0] as usize % codes.len();
        if chunk[1] & 1 == 0 {
            client.add_jurisdiction(&issuer, &address, &codes[code_i]);
            model[code_i] = true;
        } else {
            client.remove_jurisdiction(&issuer, &address, &codes[code_i]);
            model[code_i] = false;
        }

        let listed = client.list_jurisdictions(&address);
        let expected_count = model.iter().filter(|present| **present).count();
        assert_eq!(
            listed.len() as usize,
            expected_count,
            "op={op}: jurisdiction list size mismatch"
        );
        for (code_i, expected) in model.iter().enumerate() {
            assert_eq!(
                listed.iter().any(|listed_code| listed_code == codes[code_i]),
                *expected,
                "op={op} code={}: jurisdiction membership mismatch",
                CODES[code_i]
            );
        }
    }
});
