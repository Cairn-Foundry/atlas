# Cross-service review

The review of the sources together. Most of the expensive bugs of a multi-service change live
here, and a single-MR review cannot see them: each side is correct in isolation, the pair is
wrong.

## Contents

1. Contract surface of a source
2. Matching
3. Completeness of the work
4. Deploy order
5. Worked example (fictional)

## 1. Contract surface of a source

Record for every source, from its diff and the code it touches:

**Produces**
- HTTP endpoints added, removed or changed: path, method, request fields and validation, response
  fields and nullability, status codes, error codes.
- Messages and events published: name or routing key, payload fields, ordering assumptions.
- Webhooks or callbacks emitted, files exported, search-index fields written.
- Shared enums, types, packages or generated clients it publishes.

**Consumes**
- Endpoints, events, callbacks, index fields, shared types it reads, with the fields it relies on.

**Deploy requires**
- Migrations, backfills, reindexes, seeds, one-off commands.
- New environment variables, secrets, service accounts, queues, workers, crons, feature flags.
- Another service's change being live first.

## 2. Matching

Build a table: each produced item against its consumers, each consumed item against its producer.

| Item | Producer (source, file:line) | Consumer (source, file:line) | Status |
|---|---|---|---|

For every row, check:

- **Shape.** Field names, casing, nesting, nullability, types (number vs numeric string, ISO
  string vs timestamp), enum values. A value added to a producer enum and absent from a consumer
  `switch` falls into its default branch.
- **Semantics.** Same meaning on both sides: a filter "contains" on one side and "equals" on the
  other; a status code reused with two meanings; a success reported by a relay that did not check
  the downstream result.
- **Validation parity.** Consumer-side validation identical to producer-side validation (same
  pattern, same bounds, same required fields). A stricter producer rejects what the UI accepts; a
  stricter UI hides what the producer allows.
- **Errors.** Every error code the producer can return is handled by the consumer, with a
  fallback for unknown ones.
- **Decisions owned once.** A decision computed on both sides (which exports are async, which role
  may act, how a value is normalised) drifts. One side owns it; the other reads the result.
- **Orphans.** An item produced and never consumed (dead contract), or consumed and produced by
  nobody in the sources (read the missing side: it may be in a repository not given, or not written
  yet).
- **Tests.** Tests on each side use the values the other side really sends (the producer tests
  `"true"`, the consumer sends `1`).

When the other side of a contract is not among the sources, look for it (local clone, remote
default branch) and read it. If it cannot be read, turn the check into a question in the report.

## 3. Completeness of the work

- The ticket or description asks for behaviour that no source implements, or implements on one
  side only (an API field with no UI, a UI action with no endpoint, a producer event nobody
  subscribes to).
- A rename or removal done in one service and not in its consumers.
- A merge or conflict resolution in one source that silently dropped a field another source relies
  on: diff the resolved file against both parents when a merge commit touches a contract.

## 4. Deploy order

Derive the order from the dependencies, then compare it with what the MRs say:

- Producer before consumer for an additive change; consumer before producer for a removal
  (consumers stop reading first).
- A validation relaxed: the side that enforces it ships first, the UI last. A validation tightened:
  the UI first.
- A new index field exposed: backend deployed and reindex done before the UI reads it.
- A new callback or webhook: the receiving route deployed before the emitter is switched on.
- Backfills and migrations before the code that assumes their data.

Report the order as a numbered list with the reason for each step, and flag any source that states
a different order or none when one is needed.

## 5. Worked example (fictional)

Three sources are given for the ticket SHOP-812 "let support agents refund part of an order":

1. `orders-api` MR (service in NestJS): adds `POST /orders/:id/refunds` with
   `amount` validated as `@Min(0.01)`, and publishes `order.refunded` with `{ orderId, amount }`.
2. `billing-worker` branch `feat/partial-refund` (Laravel): consumes `order.refunded` and issues a
   credit note.
3. `web-console` MR (React): a refund dialog calling the new endpoint.

Findings a single-MR review misses:

- `web-console` validates `amount > 0` with a float input and sends `"12.50"` as a string;
  `orders-api` declares `amount: number` with `@IsNumber()` and no `@Type(() => Number)`: every
  refund returns 400. Contract shape, HIGH, on both MRs.
- `billing-worker` reads `payload['amount_cents']`, the name used by the older `order.cancelled`
  event; the new event sends `amount` in major units. Credit notes are issued for 0.
  Orphan and shape, CRITICAL.
- `orders-api` returns `409 REFUND_EXCEEDS_BALANCE`; `web-console` only handles 400 and shows a
  generic error. Error handling, MEDIUM.
- The consumer handler is not idempotent: the message broker redelivers after a worker restart and
  a second credit note is issued. Retry, HIGH, on `billing-worker`.
- Deploy order: `billing-worker` first (a published event with no consumer is lost),
  then `orders-api`, then `web-console`. None of the three MRs states it.
