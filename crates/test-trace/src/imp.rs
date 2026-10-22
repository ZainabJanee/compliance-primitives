extern crate std;

use core::fmt::Write as _;
use soroban_sdk::testutils::ContractEvents;
use soroban_sdk::xdr::{self, ContractEventBody, ScVal};
use soroban_sdk::{Address, Env, IntoVal, TryFromVal, Val, Vec};
use std::string::String;

/// Render a list of XDR contract events as a readable, multi-line string.
pub fn format_xdr_events(events: &[xdr::ContractEvent]) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "{} event(s):", events.len());
    for (i, event) in events.iter().enumerate() {
        let contract = match &event.contract_id {
            Some(id) => std::format!("{id}"),
            None => String::from("<none>"),
        };
        let _ = writeln!(out, "  [{i}] contract: {contract}");
        if !matches!(event.type_, xdr::ContractEventType::Contract) {
            let _ = writeln!(out, "      type:   {:?}", event.type_);
        }
        let ContractEventBody::V0(body) = &event.body;
        let topics: std::vec::Vec<String> = body.topics.iter().map(format_scval).collect();
        let _ = writeln!(out, "      topics: [{}]", topics.join(", "));
        let _ = writeln!(out, "      data:   {}", format_scval(&body.data));
    }
    out
}

/// Render captured `env.events().all()` output as a readable string.
pub fn format_events(events: &ContractEvents) -> String {
    format_xdr_events(events.events())
}

/// Print every event captured so far in `env`, under a `label` header.
///
/// Output is captured by the test harness and shown if the test fails (or
/// always with `cargo test -- --nocapture`).
pub fn print_events(env: &Env, label: &str) {
    use soroban_sdk::testutils::Events as _;
    std::println!("--- events: {label} ---\n{}", format_events(&env.events().all()));
}

/// Like `assert_eq!(actual, expected)` for events, but on mismatch panics
/// with both sides pretty-printed instead of a raw XDR dump.
///
/// `expected` uses the same `(contract, topics, data)` tuple shape the SDK
/// accepts for `assert_eq!(env.events().all(), ...)`.
#[track_caller]
pub fn assert_events_eq(env: &Env, actual: &ContractEvents, expected: &Vec<(Address, Vec<Val>, Val)>) {
    if *actual == *expected {
        return;
    }
    let expected_xdr = to_xdr_events(env, expected);
    panic!(
        "contract events mismatch\n\n== actual ==\n{}\n== expected ==\n{}",
        format_events(actual),
        format_xdr_events(&expected_xdr),
    );
}

fn to_xdr_events(env: &Env, expected: &Vec<(Address, Vec<Val>, Val)>) -> std::vec::Vec<xdr::ContractEvent> {
    expected
        .iter()
        .map(|(contract, topics, data)| {
            let topics: std::vec::Vec<ScVal> = topics
                .iter()
                .map(|t| ScVal::try_from_val(env, &t).unwrap_or(ScVal::Void))
                .collect();
            xdr::ContractEvent {
                ext: xdr::ExtensionPoint::V0,
                type_: xdr::ContractEventType::Contract,
                contract_id: contract_id_of(env, &contract),
                body: ContractEventBody::V0(xdr::ContractEventV0 {
                    topics: topics.try_into().unwrap_or_default(),
                    data: ScVal::try_from_val(env, &data).unwrap_or(ScVal::Void),
                }),
            }
        })
        .collect()
}

/// `Address::contract_id` is crate-private in the SDK, so go through the
/// `ScVal` conversion to get the XDR contract id.
fn contract_id_of(env: &Env, address: &Address) -> Option<xdr::ContractId> {
    let val: Val = address.into_val(env);
    match ScVal::try_from_val(env, &val) {
        Ok(ScVal::Address(xdr::ScAddress::Contract(id))) => Some(id),
        _ => None,
    }
}

fn format_scval(v: &ScVal) -> String {
    match v {
        ScVal::Void => String::from("void"),
        ScVal::Bool(b) => std::format!("{b}"),
        ScVal::U32(n) => std::format!("u32:{n}"),
        ScVal::I32(n) => std::format!("i32:{n}"),
        ScVal::U64(n) => std::format!("u64:{n}"),
        ScVal::I64(n) => std::format!("i64:{n}"),
        ScVal::U128(p) => std::format!("u128:{}", ((p.hi as u128) << 64) | p.lo as u128),
        ScVal::I128(p) => std::format!("i128:{}", ((p.hi as i128) << 64) | p.lo as i128),
        ScVal::Symbol(s) => std::format!("sym:{}", s.0.to_utf8_string_lossy()),
        ScVal::String(s) => std::format!("{:?}", s.0.to_utf8_string_lossy()),
        ScVal::Address(a) => std::format!("addr:{a}"),
        ScVal::Vec(Some(items)) => {
            let parts: std::vec::Vec<String> = items.0.iter().map(format_scval).collect();
            std::format!("[{}]", parts.join(", "))
        }
        ScVal::Vec(None) => String::from("[]"),
        ScVal::Map(Some(entries)) => {
            let parts: std::vec::Vec<String> = entries
                .0
                .iter()
                .map(|e| std::format!("{}: {}", format_scval(&e.key), format_scval(&e.val)))
                .collect();
            std::format!("{{{}}}", parts.join(", "))
        }
        ScVal::Map(None) => String::from("{}"),
        other => std::format!("{other:?}"),
    }
}
