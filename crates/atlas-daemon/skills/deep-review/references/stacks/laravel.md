# Laravel

Framework traps and review checks that recur in Laravel APIs. The repository's own rules win.

## Errors and API responses
- `abort()` scattered in services: prefer HTTP exceptions (`ConflictHttpException`,
  `HttpException`) or domain exceptions rendered by the handler, so status and code stay
  consistent.
- 4xx and 5xx JSON responses carry a stable `code` besides `message`; one meaning per status per
  endpoint; one error key (`error` or `errors`), not both.
- HTTP client calls (Saloon, Guzzle, `Http::`): on `failed()` throw, never return the error body as
  if it were data. `$response->json('data') ?? []` on a response that drives a write is silent
  degradation.
- Domain exception messages may be meant for users; infrastructure exception messages are not:
  mask them and keep the original as `previous`.

## Validation
- `FormRequest` messages for rules that are not declared; create and update requests that
  validate the same field differently; hierarchical consistency checked on create only.
- Free strings that feed comparisons or columns without `regex` or `max`.
- `filter_var(..., FILTER_NULL_ON_FAILURE)` and `Carbon::parse` on user input are permissive
  (`Carbon::parse('now')` succeeds): validate the format in the request.
- Validation inside a `StreamedResponse` callback runs after headers are sent: the user gets a
  truncated file instead of a 422. Validate before streaming.
- `TrimStrings` and `ConvertEmptyStringsToNull` middleware already normalise input: trimming again
  in `prepareForValidation` is noise.

## Queues and jobs
- A terminal FAILED status written in the per-attempt `catch` while `$tries > 1`: move it to
  `failed()`, which runs once after the last attempt.
- An async job behind a 202 needs a user-visible outcome on failure (`failed()` notifies).
- `WithoutOverlapping`: if `tries * releaseAfter` is shorter than `expireAfter`, runs are dropped.
- Long jobs on a worker shared with latency-sensitive queues block them: dedicated queue.
- `handle()` must be tested directly; a controller test with `Bus::fake()` stops at dispatch.

## Database
- A mass update and the cursor or version row it depends on in one `DB::transaction()`; logs and
  events after commit (`afterCommit`), not inside the closure.
- `max() + 1` counters and `lockForUpdate()` on an empty result are races; invariants need a
  `unique()` index.
- JSON in `jsonb` on PostgreSQL; an index on every new filter column.
- Columns added as `nullable()` are NULL on old rows: any matching on them needs a backfill command
  and a deployment note.
- Mixing `->change()` and new columns may need separate `Schema::table` closures (DBAL): not a
  smell.
- Deleting rows in a migration: log them before deleting when they cannot be restored.

## Authorization
- Routes under `auth:sanctum` that trigger writes or jobs need an ability (`tokenCan`, ability
  middleware) or a policy. Service tokens with a named ability, never `['*']`.
- Webhooks: `throttle`, signature or token check with `hash_equals`.
- Unauthenticated routes may bypass request logging middleware that relies on the user.

## Dates
- `Carbon::parse` of an ISO string with `Z` stays in UTC: `->setTimezone(config('app.timezone'))`
  before `startOfDay()` / `endOfDay()`, or local dates shift by a day.

## Performance
- A synchronous upstream call on every listing page or frequent write: `Cache::remember` with an
  invalidation path.
- Hundreds of ids in a GET query string (414 risk): POST or batching.
- A helper querying on every call inside a loop: memoise (`??=`).
- N+1 on relations in resources: eager load.

## Tests
- `assertJsonFragment(['id' => 5])` matches any nested `"id":5`: assert on
  `collect($response->json('data'))->pluck('id')`.
- Unit tests that touch `config()`, facades or the timezone need the application test case.
- Cover the values the client really sends (`1` / `0`, absent, invalid -> 422), `Mail::fake()`
  for mails.

## PHP
- Implicit nullable parameters (`int $x = null`) are deprecated: `?int $x = null`.
- Array access on external payloads without `?? null`.
