# Svelte 5 and SvelteKit

Framework traps and review checks that recur in Svelte 5 components and SvelteKit apps. The
repository's own rules win. Check the project's SvelteKit major version before flagging imports:
SvelteKit 3 differs from 2 (below).

## Runes and components
- `$effect` that derives state or fetches data: `$derived`, or a `load` function. An effect is for
  synchronising with the outside world (DOM, timers, third-party libraries).
- `$effect` without a cleanup for the timer, listener or subscription it creates.
- `$effect` wrapped in `if (browser)`: effects never run on the server, the guard is noise.
- A value computed once from a prop (`const label = format(prop)`) does not update when the prop
  changes: `$derived`.
- Reassigning or mutating a prop that is not `$bindable`.
- `$state` proxies passed to a library or `structuredClone`: `$state.snapshot`.
- `{#each}` without a key, or keyed by index on a list that reorders or filters.
- `{#key}` unmounts and remounts everything below it: expensive, often a `$derived` in disguise.
- `{@html}` with untrusted content.
- Legacy syntax in new code: `export let`, `$:`, `on:click`, `<slot>`, `createEventDispatcher`,
  `use:action` where `{@attach}` fits.

## Shared state and SSR
- Module-level `$state` or a store holding per-user data: shared by every request on the server.
  Use context (`createContext`) set in the layout.
- Reading `window`, `document` or `localStorage` at module level, or in code that runs on the server.

## SvelteKit data and forms
- Secrets or private env vars read in a universal `+page.ts` / `+layout.ts` (it runs in the browser
  too): `+page.server.ts`, `$env/static/private`.
- Authorization checked in a layout `load` only: form actions and `+server.ts` endpoints run
  without it. Check `locals` in each.
- Sequential `await`s in `load` that could run in parallel; global `fetch` instead of the provided
  one (loses cookies and relative URLs on the server).
- A form action that trusts its input: validate, return `fail(400, ...)`, never throw raw errors.
- `redirect()` or `error()` inside a broad `try`/`catch` is swallowed.
- A form without `use:enhance` where a full reload is not intended; `invalidateAll()` used as a
  fix for a missing dependency.
- Remote functions are experimental: flag them in production code unless the flag is set on purpose.

## SvelteKit 3 specifics (check the project's SvelteKit major first)
Source: svelte.dev/docs/kit/migrating-to-sveltekit-3.
- `$app/stores` is removed: `$app/state` (`page`, `navigating`). `page.url` is readonly.
- `$lib` is not generated: `#lib` and `#lib/*` declared in the `package.json` `imports` field, with
  explicit file extensions. `browser` comes from `$app/env` (was `$app/environment`).
- Options live in the `sveltekit()` plugin call in `vite.config`, not in `svelte.config.js`; Svelte
  compiler options go under its `compilerOptions`.
- `error(status, message)` takes a string (extra properties as a third argument); an external
  `redirect` needs the `external` option; `fail(status)` now sets the response status with
  `use:enhance`.

## Tests
- Component tests asserting on implementation (internal state) instead of rendered output.
- End-to-end tests with fixed timeouts instead of waiting on a condition.
