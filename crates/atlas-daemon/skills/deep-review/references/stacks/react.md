# React (with React Hook Form, Zod, RTK Query or TanStack Query)

Framework traps and review checks that recur in React + TypeScript fronts. The repository's own
rules win.

## Components
- A component declared inside another component remounts on every render (state lost, effects
  rerun): move it to module scope.
- Constants, configuration arrays and pure helpers inside a component body are recreated on every
  render: module scope or `utils/`.
- A function named `renderX` returning JSX is a component.
- Business logic in UI files: hooks, utils, config.
- Reuse the repository's own kit before writing (toast hook, dialog, loading button, conditional
  render helpers, form field components). Grep for it.

## Keys, effects, memo
- `key={index}` on a list that can reorder, filter or load more: a stable id. Two lists on one page
  with index-based keys collide when rendered as siblings.
- A `useEffect` reacting to a user event belongs in the event handler.
- Async effects need cancellation (`AbortController`, cleanup, ignore flag); timers need
  `clearTimeout`.
- Several `exhaustive-deps` disables in one file hide stale closures.
- `useMemo` and `useCallback` need a reason (referential stability for a dependency, measured
  cost).
- `cond && <X/>` renders `0` when `cond` is the number 0; `value || fallback` drops a legitimate 0:
  use `??` and explicit checks.

## React Hook Form + Zod
- Validation lives in the schema, not in `rules`, when the repository uses a resolver.
- `defaultValues` is read once at init: later changes need `reset(values)`.
- Changing the resolver does not revalidate existing values: call `trigger()`.
- `watch()` on a parent object may not re-render on nested changes; `useWatch` on the field.
- `useFieldArray` indices shift when the displayed list is re-sorted: keep the original index.
- Hidden fields must not be required; conditional shapes with `z.discriminatedUnion`.
- `isDirty` and `dirtyFields` instead of custom modified tracking; `handleSubmit` for submission.
- Text fields default to `''`, not `null`.

## Data fetching
- RTK Query: mutations invalidate the right tags; `skipToken` instead of placeholder args;
  endpoints not on the cache layer need their own refresh.
- A query whose result is not rendered while a parent value is unset (a dependent select, a child
  list) is skipped until that value exists: otherwise it fetches an unfiltered list for nothing and
  can raise an error toast for data nobody sees.
- TanStack Query: invalidate on mutation; `enabled` for dependent queries.
- Stale async results (a slow response for a previous selection): guard with a request id.
- No empty `catch`; a failed count never displays 0; backend error codes mapped to translations
  with a fallback.
- Long numeric strings (phone numbers, card numbers, big ids): never `Number()`, precision is lost
  above 2^53.
- Filtering a paginated or limited backend result on the client hides real data.

## Typing
- `as` and `!` hide uncertainty: generics, inference or a guard.
- A conditional render helper component does not narrow types: a `!` right after it is a smell.
- Types mirror backend nullability exactly.

## UX and accessibility
- Every async action shows progress and failure; double submission prevented.
- Disabled actions explain why.
- Two buttons with the same accessible name, toggles without `aria-expanded`, loading skeletons
  without `role="status"`, tooltips on disabled buttons without a focusable wrapper.
- Technical error messages never shown to end users.

## End-to-end tests (Playwright)
- `toBeHidden()` passes when the element never rendered; `toHaveCount(0)` before the list loaded
  proves nothing; `getByRole('row')` counts the header row.
- Skip conditions derived from backend data, not from what the page renders; `test.skip` must not
  mask a failure.
- Tests must not mutate a shared real backend without restoring fixtures.
- Timeouts on setup requests and spawned processes.
