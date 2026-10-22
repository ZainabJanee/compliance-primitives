# circuit-breaker-consumer

Minimal example token that calls `circuit-breaker.is_frozen()` before every transfer.

## Pattern

This example demonstrates the emergency-stop integration: resolve the breaker's address from storage, ask it whether it is frozen, and fail the transfer before any balance is touched if it is.

The contract uses a `#[contractclient]` trait to call the primitive via a cross-contract call, avoiding linking its implementation into the same wasm binary.

## Primitives composed

- `circuit-breaker` — emergency freeze switch

## Build

```sh
cargo build -p circuit-breaker-consumer --target wasm32v1-none --release
```
