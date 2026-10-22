#![no_main]

//! Byte-driven counterpart of `contracts/denylist-gate/src/fuzz_test.rs`.
//!
//! Each 2-byte chunk is one operation: `chunk[0] % 3` picks add / remove /
//! check and `chunk[1] % 5` picks the address. The "last write wins" oracle
//! is the same as the in-crate harness.

use denylist_gate::{DenylistGate, DenylistGateClient};
use libfuzzer_sys::fuzz_target;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, Env, Vec};

const MAX_ADDRESSES: usize = 5;
const MAX_SEQUENCE_LEN: usize = 64;

fuzz_target!(|data: &[u8]| {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let contract_id = env.register(DenylistGate, ());
    let client = DenylistGateClient::new(&env, &contract_id);
    client.initialize(&admin);

    let mut addresses = Vec::new(&env);
    for _ in 0..MAX_ADDRESSES {
        addresses.push_back(Address::generate(&env));
    }
    let mut denied = [false; MAX_ADDRESSES];

    for chunk in data.chunks_exact(2).take(MAX_SEQUENCE_LEN) {
        let addr_idx = chunk[1] as usize % MAX_ADDRESSES;
        let address = addresses.get(addr_idx as u32).unwrap();

        match chunk[0] % 3 {
            0 => {
                client.add_to_denylist(&admin, &address);
                denied[addr_idx] = true;
            }
            1 => {
                client.remove_from_denylist(&admin, &address);
                denied[addr_idx] = false;
            }
            _ => {
                assert_eq!(
                    client.check(&address),
                    !denied[addr_idx],
                    "addr_idx={addr_idx}"
                );
            }
        }
    }

    for idx in 0..MAX_ADDRESSES {
        let address = addresses.get(idx as u32).unwrap();
        assert_eq!(client.check(&address), !denied[idx], "final addr_idx={idx}");
    }
});
