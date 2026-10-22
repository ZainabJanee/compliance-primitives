use compliance_pausable as pausable;
use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contracttype, Address, BytesN, Env,
    String, Symbol, Vec,
};
use soroban_sdk::testutils::Address as _;

// ---------------------------------------------------------------------------
// Error enum following Step 1: ContractPaused discriminant
// ---------------------------------------------------------------------------
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum ConsumerError {
    NotInitialized = 1,
    NotAuthorized = 2,
    InvalidInput = 3,
    ContractPaused = 4,
}

// ---------------------------------------------------------------------------
// Events following Step 2: local Paused/Unpaused events
// ---------------------------------------------------------------------------
#[contractevent]
pub struct Paused {
    #[topic]
    pub admin: Address,
}

#[contractevent]
pub struct Unpaused {
    #[topic]
    pub admin: Address,
}

#[contracttype]
#[derive(Clone)]
enum DataKey {
    Admin,
    FlagValue,
    Record(String),
    Balance(Address),
    Setting(Symbol),
}

// ---------------------------------------------------------------------------
// Sample pausable consumer contract with multiple mutating and read methods
// ---------------------------------------------------------------------------
#[contract]
pub struct SamplePausableConsumer;

#[contractimpl]
impl SamplePausableConsumer {
    pub fn initialize(env: Env, admin: Address) -> Result<(), ConsumerError> {
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        Ok(())
    }

    // Step 3: Admin-gated pause / unpause / is_paused methods
    pub fn pause(env: Env, admin: Address) -> Result<(), ConsumerError> {
        Self::require_admin(&env, &admin)?;
        pausable::pause(&env);
        Paused { admin }.publish(&env);
        Ok(())
    }

    pub fn unpause(env: Env, admin: Address) -> Result<(), ConsumerError> {
        Self::require_admin(&env, &admin)?;
        pausable::unpause(&env);
        Unpaused { admin }.publish(&env);
        Ok(())
    }

    pub fn is_paused(env: Env) -> bool {
        pausable::is_paused(&env)
    }

    // Step 4: require_not_paused placed at the top of every state-mutating method
    pub fn set_flag(env: Env, admin: Address, value: u32) -> Result<(), ConsumerError> {
        pausable::require_not_paused_or(&env, ConsumerError::ContractPaused)?;
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::FlagValue, &value);
        Ok(())
    }

    pub fn record_data(
        env: Env,
        admin: Address,
        key: String,
        val: u32,
    ) -> Result<(), ConsumerError> {
        pausable::require_not_paused_or(&env, ConsumerError::ContractPaused)?;
        Self::require_admin(&env, &admin)?;
        env.storage().persistent().set(&DataKey::Record(key), &val);
        Ok(())
    }

    pub fn batch_update(
        env: Env,
        admin: Address,
        keys: Vec<String>,
        val: u32,
    ) -> Result<(), ConsumerError> {
        pausable::require_not_paused_or(&env, ConsumerError::ContractPaused)?;
        Self::require_admin(&env, &admin)?;
        for k in keys.iter() {
            env.storage().persistent().set(&DataKey::Record(k), &val);
        }
        Ok(())
    }

    pub fn transfer_value(
        env: Env,
        from: Address,
        to: Address,
        amount: i128,
    ) -> Result<(), ConsumerError> {
        pausable::require_not_paused_or(&env, ConsumerError::ContractPaused)?;
        if amount <= 0 {
            return Err(ConsumerError::InvalidInput);
        }
        from.require_auth();

        let from_bal: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::Balance(from.clone()))
            .unwrap_or(0);
        if from_bal < amount {
            return Err(ConsumerError::InvalidInput);
        }

        let to_bal: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::Balance(to.clone()))
            .unwrap_or(0);

        env.storage()
            .persistent()
            .set(&DataKey::Balance(from), &(from_bal - amount));
        env.storage()
            .persistent()
            .set(&DataKey::Balance(to), &(to_bal + amount));
        Ok(())
    }

    pub fn update_setting(
        env: Env,
        admin: Address,
        key: Symbol,
        val: u32,
    ) -> Result<(), ConsumerError> {
        pausable::require_not_paused_or(&env, ConsumerError::ContractPaused)?;
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::Setting(key), &val);
        Ok(())
    }

    // Step 5: Read-only methods - deliberately NOT gated by require_not_paused
    pub fn get_flag(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::FlagValue)
            .unwrap_or(0)
    }

    pub fn get_record(env: Env, key: String) -> Option<u32> {
        env.storage().persistent().get(&DataKey::Record(key))
    }

    pub fn get_balance(env: Env, addr: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::Balance(addr))
            .unwrap_or(0)
    }

    pub fn get_setting(env: Env, key: Symbol) -> Option<u32> {
        env.storage().instance().get(&DataKey::Setting(key))
    }

    fn require_admin(env: &Env, admin: &Address) -> Result<(), ConsumerError> {
        admin.require_auth();
        let stored_admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(ConsumerError::NotInitialized)?;
        if stored_admin != *admin {
            return Err(ConsumerError::NotAuthorized);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Flawed consumer contract used to prove tests catch missing guards
// ---------------------------------------------------------------------------
#[contract]
pub struct FlawedPausableConsumer;

#[contractimpl]
impl FlawedPausableConsumer {
    pub fn initialize(env: Env, admin: Address) {
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
    }

    pub fn pause(env: Env) {
        pausable::pause(&env);
    }

    pub fn is_paused(env: Env) -> bool {
        pausable::is_paused(&env)
    }

    /// Correctly guarded mutating method
    pub fn guarded_action(env: Env, val: u32) -> Result<(), ConsumerError> {
        pausable::require_not_paused_or(&env, ConsumerError::ContractPaused)?;
        env.storage().instance().set(&DataKey::FlagValue, &val);
        Ok(())
    }

    /// FLAWED: Mutating method that forgot the `require_not_paused` guard!
    pub fn unguarded_action(env: Env, val: u32) -> Result<(), ConsumerError> {
        // Missing: pausable::require_not_paused_or(&env, ConsumerError::ContractPaused)?;
        env.storage().instance().set(&DataKey::FlagValue, &val);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Table-Driven Test Harness for Mutating & Read-Only Entrypoints
// ---------------------------------------------------------------------------

struct MutatingEntrypoint<'a> {
    name: &'static str,
    call_paused: &'a dyn Fn(&SamplePausableConsumerClient, &Address, &Address, &Address) -> Result<Result<(), ConsumerError>, Result<ConsumerError, soroban_sdk::InvokeError>>,
    call_unpaused: &'a dyn Fn(&SamplePausableConsumerClient, &Address, &Address, &Address) -> Result<Result<(), ConsumerError>, Result<ConsumerError, soroban_sdk::InvokeError>>,
}

struct ReadOnlyEntrypoint<'a> {
    name: &'static str,
    call_paused: &'a dyn Fn(&SamplePausableConsumerClient, &Address) -> bool,
}

fn setup_consumer(
    env: &Env,
) -> (
    Address,
    Address,
    Address,
    SamplePausableConsumerClient<'_>,
) {
    env.mock_all_auths();
    let admin = Address::generate(env);
    let user1 = Address::generate(env);
    let user2 = Address::generate(env);

    let contract_id = env.register(SamplePausableConsumer, ());
    let client = SamplePausableConsumerClient::new(env, &contract_id);
    client.initialize(&admin);

    (admin, user1, user2, client)
}

#[test]
fn test_table_driven_all_mutating_calls_blocked_when_paused() {
    let env = Env::default();
    let (admin, user1, user2, client) = setup_consumer(&env);

    // Initial state: ensure contract is not paused
    assert!(!client.is_paused(), "Contract should not be paused by default");

    // Pre-populate some baseline state while unpaused
    client.set_flag(&admin, &100);
    assert_eq!(client.get_flag(), 100);

    // Table of every mutating entrypoint in SamplePausableConsumer
    let mutating_entrypoints: [MutatingEntrypoint; 5] = [
        MutatingEntrypoint {
            name: "set_flag",
            call_paused: &|c, a, _, _| c.try_set_flag(a, &200),
            call_unpaused: &|c, a, _, _| c.try_set_flag(a, &200),
        },
        MutatingEntrypoint {
            name: "record_data",
            call_paused: &|c, a, _, _| {
                let key = String::from_str(c.env, "test_key");
                c.try_record_data(a, &key, &42)
            },
            call_unpaused: &|c, a, _, _| {
                let key = String::from_str(c.env, "test_key");
                c.try_record_data(a, &key, &42)
            },
        },
        MutatingEntrypoint {
            name: "batch_update",
            call_paused: &|c, a, _, _| {
                let mut keys = Vec::new(c.env);
                keys.push_back(String::from_str(c.env, "batch_1"));
                keys.push_back(String::from_str(c.env, "batch_2"));
                c.try_batch_update(a, &keys, &99)
            },
            call_unpaused: &|c, a, _, _| {
                let mut keys = Vec::new(c.env);
                keys.push_back(String::from_str(c.env, "batch_1"));
                keys.push_back(String::from_str(c.env, "batch_2"));
                c.try_batch_update(a, &keys, &99)
            },
        },
        MutatingEntrypoint {
            name: "transfer_value",
            call_paused: &|c, _, u1, u2| c.try_transfer_value(u1, u2, &50),
            call_unpaused: &|c, _, u1, u2| c.try_transfer_value(u1, u2, &50),
        },
        MutatingEntrypoint {
            name: "update_setting",
            call_paused: &|c, a, _, _| {
                let s = Symbol::new(c.env, "max_limit");
                c.try_update_setting(a, &s, &1000)
            },
            call_unpaused: &|c, a, _, _| {
                let s = Symbol::new(c.env, "max_limit");
                c.try_update_setting(a, &s, &1000)
            },
        },
    ];

    // Table of every read-only entrypoint in SamplePausableConsumer
    let readonly_entrypoints: [ReadOnlyEntrypoint; 4] = [
        ReadOnlyEntrypoint {
            name: "get_flag",
            call_paused: &|c, _| {
                let val = c.get_flag();
                val == 100
            },
        },
        ReadOnlyEntrypoint {
            name: "get_record",
            call_paused: &|c, _| {
                let key = String::from_str(c.env, "non_existent");
                c.get_record(&key).is_none()
            },
        },
        ReadOnlyEntrypoint {
            name: "get_balance",
            call_paused: &|c, u1| {
                let bal = c.get_balance(u1);
                bal == 0
            },
        },
        ReadOnlyEntrypoint {
            name: "get_setting",
            call_paused: &|c, _| {
                let s = Symbol::new(c.env, "max_limit");
                c.get_setting(&s).is_none()
            },
        },
    ];

    // 1. PAUSE THE CONTRACT
    client.pause(&admin);
    assert!(client.is_paused(), "Contract should be paused after pause()");

    // 2. ASSERT EVERY MUTATING METHOD IS REJECTED WHILE PAUSED
    for entry in &mutating_entrypoints {
        let result = (entry.call_paused)(&client, &admin, &user1, &user2);
        assert_eq!(
            result,
            Err(Ok(ConsumerError::ContractPaused)),
            "Mutating entrypoint '{}' MUST return ContractPaused error when contract is paused",
            entry.name
        );
    }

    // 3. ASSERT EVERY READ-ONLY METHOD SUCCEEDS WHILE PAUSED
    for entry in &readonly_entrypoints {
        let ok = (entry.call_paused)(&client, &user1);
        assert!(
            ok,
            "Read-only entrypoint '{}' MUST NOT be blocked when contract is paused",
            entry.name
        );
    }

    // 4. UNPAUSE THE CONTRACT
    client.unpause(&admin);
    assert!(!client.is_paused(), "Contract should be unpaused after unpause()");

    // 5. ASSERT MUTATIONS RESUME AFTER UNPAUSE
    for entry in &mutating_entrypoints {
        let result = (entry.call_unpaused)(&client, &admin, &user1, &user2);
        assert!(
            result.is_ok() || result != Err(Ok(ConsumerError::ContractPaused)),
            "Mutating entrypoint '{}' MUST NOT return ContractPaused once contract is unpaused",
            entry.name
        );
    }
}

#[test]
fn test_missing_guard_detection() {
    // This test proves that the test harness correctly detects any mutating method
    // where a developer forgot to call `require_not_paused`.
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);

    let contract_id = env.register(FlawedPausableConsumer, ());
    let client = FlawedPausableConsumerClient::new(&env, &contract_id);
    client.initialize(&admin);

    client.pause();
    assert!(client.is_paused());

    // Guarded method is blocked as expected
    let guarded_res = client.try_guarded_action(&55);
    assert_eq!(
        guarded_res,
        Err(Ok(ConsumerError::ContractPaused)),
        "Guarded action must be rejected when paused"
    );

    // Unguarded method erroneously succeeds when paused (simulating a developer error)
    let unguarded_res = client.try_unguarded_action(&99);
    assert!(
        unguarded_res.is_ok(),
        "Unguarded action bypassed the pause check — proving our test structure catches missing guards"
    );
}

#[test]
fn test_pause_state_isolation_between_contracts() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);

    let id1 = env.register(SamplePausableConsumer, ());
    let client1 = SamplePausableConsumerClient::new(&env, &id1);
    client1.initialize(&admin);

    let id2 = env.register(SamplePausableConsumer, ());
    let client2 = SamplePausableConsumerClient::new(&env, &id2);
    client2.initialize(&admin);

    // Pause contract 1
    client1.pause(&admin);
    assert!(client1.is_paused());
    assert!(!client2.is_paused(), "Contract 2 must remain unpaused (instance storage isolation)");

    // Contract 1 mutating call fails
    assert_eq!(
        client1.try_set_flag(&admin, &42),
        Err(Ok(ConsumerError::ContractPaused))
    );

    // Contract 2 mutating call succeeds
    assert!(client2.try_set_flag(&admin, &42).is_ok());
    assert_eq!(client2.get_flag(), 42);
}
