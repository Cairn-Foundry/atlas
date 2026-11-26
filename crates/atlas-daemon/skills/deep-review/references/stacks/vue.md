# Vue 3 (with TanStack Query, component libraries, i18n)

Framework traps and review checks that recur in Vue 3 `<script setup>` fronts. The repository's
own rules win.

## Reactivity and components
- `const props = defineProps(...)` when `props` is never used in `<script>`: drop the variable.
- Two watchers mirroring each other (local copy and prop) risk loops: a computed with `get` and
  `set` emitting `update:modelValue`.
- A `watch` that only derives a value: a `computed`.
- Constants dressed as reactive state (`computed(() => true)`).
- `v-for` without a stable `:key`.
- Numbered wizard steps compared by index: name them.
- `disabled` added to hide a symptom instead of fixing the cause.
- A large template block rendering a self-contained concept: its own component. State created in
  a parent only to feed one child: the child owns it. Generic components do no domain checks.
- Script before template, types in dedicated type files rather than inside `.vue` files, when the
  repository does so.

## TanStack Query
- Every new API call behind a query or mutation composable, not called raw from components.
- Mutations invalidate the queries they affect; a manual `refetch()` after a mutation is a patch.
- `isSuccess` of a mutation resets to `false` when the next mutation starts: never merge a success
  guard with other checks, and beware retries.
- `staleTime: 0` needs a reason.
- Option lists that depend on environment data (ids) are fetched, never hardcoded.
- Data fetched for users who cannot see it: gate with `enabled`.

## Back-end ownership
- Decisions the backend owns (which exports are async, rights, normalisation) are read from its
  response, not duplicated. A 202 means queued; a blob means download.
- Search semantics shown in the UI match the backend filter (contains vs equals).
- Filters with a default value apply to every profile and to exports: check privileged roles.

## Errors
- Removing a rethrow in a `catch` loses the error-tracker capture: capture explicitly.
- A `catch` that swallows a failed upload must stop the next step.

## Routing and permissions
- Navigate by route name and params, not by string path.
- Permission checks through the repository's directive or helper, not inline role comparisons.
- Rights enforced only in the front are not rights.

## i18n and UI
- User-visible text in the translation files, not in utils, configs or types.
- Renamed domain terms renamed everywhere (toasts, filters, titles); filter labels match column
  labels.
- Components registered globally by an auto-import plugin do not need an import.
- Utility-class frameworks scan literal classes in source files: a safelist is only for classes
  built dynamically.

## Bundle
- Per-function imports for large utility libraries (`lodash/debounce`), dynamic import for heavy
  SDKs used in one path.

## Tests
- Check which runners exist before asking for tests. With e2e only, ask for an e2e scenario on the
  headline behaviour, with selectors that match the displayed labels.
