# Review method - per-source checks

Distilled from a large corpus of real human review threads across backend, frontend and
orchestration codebases, and from the triage of thousands of automated review remarks. Sections are
ordered by the weight reviewers gave them: the first ones led to fixes and incidents, the last
ones are cheap to check.

## Contents

1. Business correctness
2. Silent degradation
3. Retries, idempotency, concurrency, partial writes
4. Access scope, permissions, sensitive data
5. Tests
6. Reuse before writing
7. Dead code, leftovers, scope
8. Blast radius
9. Layering and contracts inside the service
10. Typing and data shape
11. Performance on hot paths
12. Naming and language
13. Hygiene

For each check: the rule, then how to verify it. Stack references add framework specifics; the
repository's own rules (CLAUDE.md, lint config, contributing guide) override both.

---

## 1. Business correctness

The highest-weight theme. Check the rule, not only the code.

- The implementation matches the title, description and ticket. A headline behaviour announced
  and absent from the diff is a finding.
- A refactor removed a call or side effect that another live state still depends on: a flag still
  computed, a button still enabled, an upload no longer called. Check every removed call.
- A data source switched to a new field that existing rows do not have yet: needs a fallback or a
  backfill that runs before the code. Columns added later as nullable are NULL for every older row.
- Status, role or type lists that enumerate some cases but miss the error ones or the recent ones.
  A `default` branch that silently absorbs an unknown value instead of failing.
- A filter forced for every profile when a privileged role must keep full visibility. Check list
  and export paths and sibling configurations.
- A new path for one product variant that does not replicate what the sibling variant does
  (status check, event, reservation, notification) and does not say why.
- Truthiness on business values: `0`, `'0'` and `''` are legitimate. `||`, `&&` and ternaries on
  prices, counts and references drop them.

## 2. Silent degradation

The second most costly class of bugs. Users cannot see that a filter was ignored, an export
truncated or a sync marker advanced.

- Invalid input, a malformed upstream response or a failed call that ends in a success response
  with wrong, unfiltered or empty data, with no error and no log. Typical shapes: `?? []` on an
  upstream response that then drives a write or a delete; a `catch` returning `null` or `[]` on a
  write path; a lenient parser that skips a filter on failure; a monotone cursor advanced after a
  response nobody validated.
- A `catch` that converts every error into one specific error (not found, forbidden) must check
  the status first and rethrow the rest.
- A tolerant fallback is acceptable only on a read-only display path, and it logs.
- Errors swallowed on side channels (reporting, indexing, notifications) need at least error
  tracking, and a way to replay.
- A transient upstream failure (timeout, 503) mapped to a terminal state.
- Errors returned to clients carry a stable machine-readable code; the client never parses a
  message.
- A `catch` that files programming errors (`TypeError`, a failed transform) under a transport or
  network category hides the bug behind "the server is unreachable". Map only the errors of the
  transport itself; let the rest surface as what they are. LOW when no consumer reads the
  category, MEDIUM when one does.

## 3. Retries, idempotency, concurrency, partial writes

- An external write inside a retried unit (job with retries, workflow step, webhook handler,
  message consumer) is idempotent on replay. Ask what happens when step 2 of 3 fails and the unit
  runs again.
- Validate all items before the first external write; on failure compensate only what this
  operation created.
- Persist the local trace (child row, job row, event) before the external call, so the call is
  visible and its id reaches the error path.
- A terminal "failed" status written in the per-attempt `catch` before retries are exhausted.
- Read-then-write guards that two concurrent callbacks can both pass; `max() + 1` counters;
  in-memory flags or locks in a service that may run several instances. Require the interleaving.
- A bulk update and its cursor or version insert outside one transaction; a success log or an
  event emitted inside the transaction (emit after commit).
- An invariant guarded only in application code where a unique constraint belongs.

## 4. Access scope, permissions, sensitive data

- Every endpoint reading or writing by id checks the caller's scope (tenant, organisation,
  customer, restricted subsets), not only authentication. A parent-id check without the child-id
  check is incomplete.
- A route that triggers writes or jobs under plain authentication, with no ability or permission.
  Service tokens scoped to what they do, never a wildcard. Webhooks rate-limited and authenticated.
- A permission or resource rename that makes an existing role lose a right.
- Sensitive data (personal data, secrets, identifiers that allow an account transfer, internal
  hosts, raw upstream bodies) in mails, user-visible logs or error messages.
- Seeds or scripts that are destructive outside development, or that version a ciphertext tied to
  an environment secret.

## 5. Tests

- The headline behaviour has a test. A new branch that blocks a user flow has a test. The sibling
  test in the same file is the model to follow.
- A test must be able to fail: no bare "is defined" assertions on the field that matters, no
  substring JSON assertions that match nested ids, no fixture edited until the rule no longer
  applies, no timezone case that lands on the same day in both zones, no mock resolving
  immediately when the point is to prove an await.
- Error paths assert the error type and code and the side effect (event, status, log row).
- Fallback branches the code adds (`?? null`, absent, invalid, boundary) are covered.
- Only ask for a kind of test the repository actually runs. Check the test config and CI first.

## 6. Reuse before writing

- New code re-implements what the repository already has: helper, validator, hook, shared
  component, enum, DTO, service method. Grep before flagging and name the existing thing and its
  path.
- Two near-identical blocks in the diff, or a block copied from a sibling module: one finding
  naming both sites. Extraction is asked for at the second real occurrence, never the first.
- A pure refactor with no bug is optional, and never on a release branch.

## 7. Dead code, leftovers, scope

- Exports, functions, computed values, DTOs, translation keys and attributes with no remaining
  user (grep the whole repository at the head).
- Debug logs, commented-out code, stray planning or context files, test-only endpoints.
- Actions shipped that do nothing yet.
- Unrelated changes: files touched by a bad rebase, formatting-only hunks, lockfile churn with no
  dependency change, configuration for another ticket. A diff far larger than the ticket usually
  means a wrong base branch.
- Configuration still injected but no longer read.

## 8. Blast radius

When the diff changes a base class, a shared client or connector, a mixin, a normaliser, a shared
hook or component, a global configuration value or an enum: list every subclass and caller and
check the behaviour change on those the ticket did not target. A timeout added on a shared client
applies to every call; a limit removed on a base exporter applies to every exporter.

## 9. Layering and contracts inside the service

- Business logic in controllers or in thin API-client layers.
- A client for an external system that gains logic from another domain.
- Configuration read ad hoc (`process.env`, `env()`) instead of through the configuration layer.
- A new endpoint that duplicates an existing one with a different filter or permission set.
- Response shape, status codes and error codes documented (OpenAPI or equivalent) when the
  service documents its API.

## 10. Typing and data shape

- Casts and non-null assertions that hide a real uncertainty. Ask what guarantees the value.
- Optional fields whose type ignores `null` when the source can send null.
- Dates from external JSON typed as date objects (they are strings).
- Magic strings where an enum exists.
- Schema changes: an index created in SQL but unknown to the ORM (the next generated migration
  drops it), unrelated statements in a generated migration, a column that needs a backfill with no
  command and no deployment note.

## 11. Performance on hot paths

- An upstream call added to every page of a listing or to a frequent write path, with no cache.
- Round-trips counted per user action; reads done only to log something.
- Loading rows to count them; filtering in memory what the database or search engine can filter.
- Unbounded fan-out to an upstream (concurrency limit).
- Work whose result is discarded: a request sent while the value it depends on is unset and the
  render drops its result, a read refetched on every mount when the data is constant per session.
  Mandatory pass on a change whose purpose is performance.

## 12. Naming and language

- Booleans read as booleans (`is`, `has`, `can`).
- Names say what the value is and match the name used for the same data in the other services.
- Language rules per repository (identifiers, comments, logs, user-facing text) come from its own
  rules and its majority usage. User-facing text in a language with accents keeps its accents.

## 13. Hygiene

- Title and description in the repository's format; the deploy order written when it matters.
- Remarks from review bots or humans left without an answer or a fix.
- Pre-existing problems found while reviewing are reported as out of scope with a follow-up
  suggestion, never as blocking.
