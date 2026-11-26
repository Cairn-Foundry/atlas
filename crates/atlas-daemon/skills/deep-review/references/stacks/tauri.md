# Tauri v2 (Rust core, web front)

Security and contract traps that recur in Tauri v2 apps. The repository's own rules win. A Tauri
change spans two languages: read the Rust command and the front call site together.

## Capabilities and permissions
- Plugin and core permissions are denied until a capability grants them. A plugin added in
  `Cargo.toml` and `lib.rs` without its permission in `capabilities/*.json` fails silently at runtime.
- Broad grants: an `fs` permission without a path scope, `shell:allow-execute` without a named
  command and validated args, `windows: ["*"]` where one window needs it.
- The app's own commands are callable from every window by default: with several windows or remote
  content, restrict them (`AppManifest::commands` in `build.rs`).
- Remote URLs given IPC access through a capability `remote` block (`urls`): review every pattern.
  On Linux and Android Tauri cannot tell an embedded `<iframe>` from the window itself.
- Tauri v1 leftovers: `allowlist`, `@tauri-apps/api/tauri` imports, `get_window`.

## Commands and the IPC contract
- A `#[tauri::command]` missing from `generate_handler!` is "command not found" at runtime.
- An async command with a borrowed argument (`&str`) does not compile: use an owned type, or
  return `Result` (the form that also works with `State<'_, T>`).
- Arguments are camelCase from JS and snake_case in Rust; an error returned as a plain `String`
  loses its kind. A typed, serialisable error enum keeps it.
- A command that takes a path or a command line from the front and uses it unchecked
  (path traversal, command injection): validate against the allowed scope in Rust.
- Blocking work in a command without `spawn_blocking`; a sync command freezes the UI thread.
- `invoke` called raw from components with an untyped result instead of a typed service layer.

## State and events
- `State<T>` must name exactly the type given to `manage()`: a mismatch rejects the `invoke` with a
  "state not managed" error; `app.state::<T>()` on an unmanaged type panics.
- A `std::sync::Mutex` guard held across `.await`; window handles stored in state instead of being
  resolved from the `AppHandle`.
- `emit` is delivered to all listeners: a sensitive payload goes through `emit_to` (one webview) or
  `emit_filter`. Events are untyped JSON and not meant for high throughput: streams use a
  `Channel`. A `listen` without `unlisten` on unmount leaks.

## Web content security
- `app.security.csp` null or with `unsafe-eval`; `dangerousDisableAssetCspModification`.
- `{@html}`, `dangerouslySetInnerHTML` or `eval` with data from outside the app.
- `shell.open` used for links: the opener plugin.

## Updater and distribution
- `plugins.updater.pubkey` holds the key content, not a path; endpoints over HTTPS; never
  `dangerousInsecureTransportProtocol`. `bundle.createUpdaterArtifacts: true`.
- Signing key or its password in the repository or in a config file; `.env` files are not read for
  `TAURI_SIGNING_PRIVATE_KEY`.
- A JS plugin package and its Rust crate on different versions: they must be the exact same version
  (for example `@tauri-apps/plugin-fs` 2.2.1 with `tauri-plugin-fs` 2.2.1); `@tauri-apps/api` and
  the `tauri` crate must share the same minor version.

## Layout and mobile
- Logic in `main.rs` instead of `lib.rs`: mobile builds need the entry in `lib.rs`
  (`#[cfg_attr(mobile, tauri::mobile_entry_point)]`). Desktop-only APIs (tray, multi-window) need
  `#[cfg(desktop)]`.
- Hardcoded absolute paths; writes to the resource directory (read-only at runtime): use the path
  API (`app.path().app_data_dir()`).
