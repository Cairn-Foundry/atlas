# AGENTS.md

What an agent (or a human) must know before changing Atlas. The [README](README.md) describes the
product and its use; this file describes how to work in it.

## The project in short

Atlas is a desktop application (Tauri) that supervises Claude Code agents. Three parts:

- the **daemon** (`crates/atlas-daemon`): HTTP and WebSocket API on `127.0.0.1`, SQLite registry,
  PTY sessions, supervisor, orchestrator (`claude -p`), MCP servers, `hook` command;
- the **shell** (`crates/atlas-desktop`): the Tauri window, the menu bar icon, the start of the
  daemon as a sidecar;
- the **front** (`front/`): React 19, Vite, Tailwind 4, shadcn/ui, shown by the shell.

All the Rust lives in `crates/` (one Cargo workspace), all the TypeScript in `front/` (the only
`package.json`). Run `cargo` from `crates/` and `pnpm` from `front/`.

## Where things live

| Path | Content |
| --- | --- |
| `crates/atlas-daemon/src/service/` | Agent lifecycle: launch, steps, sessions, validations, worktrees, supervision |
| `crates/atlas-daemon/src/workflow.rs` | Rules of a workflow: order of the bricks, review passes, session sharing |
| `crates/atlas-daemon/src/orchestrator.rs` | The runs of the orchestrator and their queue |
| `crates/atlas-daemon/src/server.rs`, `mcp.rs` | HTTP and WebSocket routes, MCP servers of the agents and of the orchestrator |
| `crates/atlas-daemon/src/store/` | SQLite registry: schema, migrations, rows |
| `crates/atlas-daemon/src/contract.rs` | The contract with the front, only source of the shared types |
| `crates/atlas-daemon/src/catalogue.rs`, `brick.rs` | The catalogue of bricks and the `Brick` type |
| `crates/atlas-daemon/bricks/*.yml` | The shipped bricks, embedded in the binary by `build.rs` |
| `crates/atlas-daemon/skills/`, `subagents/` | The shipped skills and subagents the bricks hand their sessions, embedded the same way |
| `crates/atlas-daemon/assets/` | MCP tools (JSON) and texts of the orchestrator and the agents |
| `crates/atlas-daemon/tests/fixtures/` | The expected prompts, compared word for word by the tests |
| `crates/atlas-link` | Finding and reaching the daemon: folder, port, token, local HTTP |
| `crates/atlas-i18n` | The language Atlas speaks (French or English) and `tr!`, shared by the daemon and the shell |
| `crates/foundry-git`, `crates/foundry-pty` | Git worktrees and pseudo-terminals, shared with the Cairn Foundry suite |
| `front/src/components/` | Components of the application; `ui/` is generated shadcn |
| `front/src/lib/` | Logic without React, tested next to it (`*.test.ts`) |
| `front/src/lib/messages/` | The texts of the interface, one dictionary per language (`fr.ts`, `en.ts`) |
| `front/src/hooks/` | React hooks |

## Commands

```sh
# Front
cd front && pnpm install
pnpm lint && pnpm typecheck && pnpm test
pnpm web:build                      # the front alone, as the CI does

# Daemon and shared crates
cd crates
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test

# Desktop shell, outside the default members
cargo clippy -p atlas-desktop --all-targets -- -D warnings
cargo test -p atlas-desktop
```

Before handing work over, make the commands of the part you touched pass: the CI
(`.github/workflows/ci.yml`) replays them with `--locked`, on macOS, Linux and Windows for the
daemon, plus a `cargo check` on Rust 1.89 (the MSRV of the daemon; the shell asks for 1.90).

A single test: `cargo test -p atlas-daemon <name>` or `pnpm test <file>`.

## Releases

A tag `v<version>` builds the release (`.github/workflows/release.yml`): the version is checked
against `front/package.json` and `crates/Cargo.toml`, then six targets are bundled (AppImage and
deb for Linux x86_64 and aarch64, a dmg per macOS architecture, an NSIS installer for Windows
x86_64 and aarch64) and published as a GitHub release whose notes start with the message of the
tag. `.github/workflows/warm-cache.yml` runs the same compile on `main` so a tag build finds a
Rust cache: its matrix must stay identical to the one of the release.

## Never touch the real factory

`pnpm dev`, `pnpm build`, `cargo run -p atlas-daemon` and the application work on the real
`~/.atlas` (registry, bricks, token, port 4317) and start real Claude Code sessions.

- For any manual run, point `ATLAS_HOME` at a throwaway folder whose `config.yml` sets another
  `port` and a fake `claudeCommand` (a script that does not start Claude Code).
- `cargo test` and `pnpm test` are safe: temporary folders, in-memory SQLite, fake terminals
  (`crates/atlas-daemon/src/testing.rs`).
- Do not kill an Atlas already running on the machine: its agents are at work.
- The shell is single-instance: a second launch hands over to the first. To check a build next to
  an open Atlas, build it under another `identifier`
  (`pnpm tauri build --bundles app --config '{"identifier":"...","productName":"..."}'`).
- Do not read or edit `~/.atlas` to make a test pass.

## The contract between Rust and TypeScript

The Rust is the only description of what the daemon sends.

- `front/src/lib/contract.ts` and `front/src/lib/contract-sample.ts` are **generated** by the
  daemon's tests: do not edit them by hand.
- After a change in `contract.rs`:

  ```sh
  cd crates && ATLAS_UPDATE_CONTRACT=1 ATLAS_UPDATE_CONTRACT_SAMPLE=1 cargo test contract
  cd ../front && pnpm typecheck
  ```

- `brick.schema.json` is derived from the `Brick` type and written by the daemon in
  `~/.atlas/bricks/`.

## Invariants to keep

- **A single thread** runs the daemon (`new_current_thread`): between two `.await`, nothing else
  touches the registry, and the service relies on it. Blocking work goes through the `blocking`
  helper of `service.rs`; the PTYs read on threads of their own and hand their output over through
  a channel.
- **Only what the binary uses is `pub`** in `atlas-daemon`: the rest stays private so the compiler
  reports dead code. Do not widen a visibility for a test.
- **The hook never fails an agent**: `atlas-daemon hook <event>` always exits 0.
- **The prompts are pinned by fixtures** (`tests/fixtures/`): a change to a text handed to an agent
  or to the orchestrator shows in a fixture diff, and is made on purpose
  (`ATLAS_UPDATE_FIXTURES=1 cargo test -p atlas-daemon` rewrites them).
- **A shipped brick** follows the updates as long as the master has not edited his copy; an edited
  brick is never rewritten. Adding a brick: one file in `crates/atlas-daemon/bricks/`, valid for
  the `Brick` type.
- **The registry migrates**: a database created by an earlier version must open. Any change to
  `store/schema.rs` comes with its upgrade and its test.
- **Security of the daemon**: listens on `127.0.0.1` only, local host required, origin of the page
  checked, token required everywhere but `/api/health`. A new route keeps these checks.
- **Three systems**: macOS, Linux and Windows (ConPTY, `claude.cmd` through `cmd.exe`). No path or
  command specific to one without a branch for the others.

## Conventions

- Comments, names and documentation in English.
- Atlas speaks French or English, the language the master picks (the system's until then). It is
  kept in the registry, carried by the snapshot, and changed while the daemon runs.
  - **What the master reads is written in both languages**, accents included in French: in Rust
    with `tr!("français", "english", arguments)` (refusals with `refuse!`, cards, notifications,
    errors, logs, the menu of the shell), in the front with an entry in both `messages/fr.ts` and
    `messages/en.ts`, read through `useMessages()`. The front never translates a text the daemon sends.
  - **What only a model reads is written once, in English**: the bricks, the skills, the
    subagents, the prompts and briefs, the descriptions and results of the MCP tools. The
    orchestrator and the agents are told which language to speak to the master (`language_rule`
    in `orchestrator_prompt.rs` and `prompts.rs`): never ship a second version of a prompt.
- Every source file (Rust, TypeScript, CSS, scripts) starts with the license header, before the
  `//!` of a Rust module; the generated contract files get it from their generator, and
  `components/ui/` is left as shadcn writes it:

  ```rust
  // Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
  // SPDX-License-Identifier: AGPL-3.0-or-later
  ```

- A comment says why, not what. Each Rust module opens with a one-sentence `//!`.
- Rust: edition 2024, `rustfmt` at 120 columns, `clippy` without a warning, `unused_must_use`
  denied. Tests live in a `mod tests` or a `tests.rs` next to the module.
- TypeScript: `import type` for types, `@/` alias for `front/src`, modules in kebab-case,
  components in PascalCase, booleans prefixed with `is` / `has` / `can`.
- `front/src/components/ui/` comes from shadcn: a component is added there with its tool, not
  retouched by hand.
- No new dependency without a real need; `Cargo.lock` and `pnpm-lock.yaml` are committed and the
  CI requires them up to date.

## Git

- Conventional commits, ticket at the end of the subject: `type(scope): description, ATLAS-4`.
- Subject of 80 characters at most, in English, saying what the change brings
  (`fix(daemon): never leave an agent at work without a session, ATLAS-4`).
- Usual scopes: `daemon`, `web`, `desktop`, `workflow`, `orchestrator`, `bricks`, `contract`,
  `repo`.
- One logical change per commit. Branches: `{type}/{ticket}/{name}`.
- Every commit is signed off (`git commit -s`): see [CONTRIBUTING.md](CONTRIBUTING.md).
- Do not push or open a pull request unless asked to.
