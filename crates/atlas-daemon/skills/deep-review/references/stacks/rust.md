# Rust (with Tokio, Serde, Cargo)

Language and ecosystem traps and review checks that recur in Rust services and libraries. The
repository's own rules win.

## Panics and conversions
- `.unwrap()` or `.expect()` on external input (network, files, user data) outside tests; `panic!`,
  `unreachable!` or slice indexing `v[i]` reachable from a request.
- `as` between integer widths or signs truncates silently: `try_from` with the error handled.
  The release profile defaults to `overflow-checks = false`, so release builds wrap on overflow
  instead of panicking (dev builds panic): `checked_*` or `saturating_*` where the value is external.
- Float equality with `==` in code or tests.

## Async and concurrency
- A `std::sync::Mutex` guard held across `.await`: the future is not `Send`, or it deadlocks.
  `tokio::sync::Mutex` only when the guard must cross an await.
- Blocking calls in an `async fn` (`std::thread::sleep`, `std::fs`, heavy CPU, a blocking client):
  `spawn_blocking` or the async equivalent.
- A detached `tokio::spawn` whose `JoinHandle` and error are dropped: failures vanish. Use a
  `JoinSet` or log and propagate.
- Futures inside `select!` that are not cancellation-safe lose data when the other branch wins
  (`read_exact`, `read_to_end`, `write_all` are unsafe; `mpsc::Receiver::recv` is safe; `Mutex::lock`
  loses its queue position).
- Unbounded channels or unbounded spawning from external input.
- `Arc<Mutex<T>>` where a channel or an owner task expresses the design.

## Ownership and API
- `.clone()` added to silence the borrow checker, `String` parameters where `&str` would do.
- Public fields or a new trait method on a public type: a semver break for a library.
  `#[non_exhaustive]` on public enums and error types; `#[must_use]` on results.
- `pub` wider than needed; a `pub use` that leaks an internal type.
- Leaking implementation details through type aliases; `Box<dyn Error>` in a library API.

## Errors
- `map_err(|_| ...)` drops the source: keep it with `#[from]` or `source`.
- `anyhow` in a library, `thiserror` enum with a catch-all `Other(String)`.
- `let _ = fallible();` silencing a `Result` that matters.
- Error messages that include secrets or full payloads.

## Unsafe
- `unsafe` without a `// SAFETY:` comment naming the invariant; `transmute`; `get_unchecked`
  without a proven bound. In edition 2024, unsafe operations inside an `unsafe fn` need their own
  block.

## Serde and wire contracts
- Renaming a field or variant, or changing `rename_all`, changes the wire format: a contract
  change for every consumer.
- Missing `#[serde(default)]` on a new field makes old payloads fail; `deny_unknown_fields` makes
  a producer that adds a field break its consumers.
- `#[serde(untagged)]` hides which variant failed and why.

## Cargo
- A new dependency without a reason, or with default features when a subset would do;
  `tokio = { features = ["full"] }` in a library (Cargo unifies features, so it forces them on every
  consumer).
- `rust-version` not pinned, edition older than the workspace's; `Cargo.lock` not committed for a
  binary.
- Lint levels set with `#[allow]` instead of `#[expect]` and a reason.

## Tests
- `#[should_panic]` without `expected`; `assert!(x.is_ok())` where the value matters.
- Async tests without `#[tokio::test]`, or with real sleeps instead of paused time.
- A test that reads the clock, the environment or the network without control.
