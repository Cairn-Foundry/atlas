# Calibration - verification protocol and what not to flag

Built from the human triage of automated review remarks: roughly half were fixed, the rest were
withdrawn, deferred or dismissed. The dismissed categories below are the ones rejected again and
again, with the reasons given. A review that repeats them loses trust faster than one that misses
a nit.

## Contents

- Verification protocol (every candidate, no exception)
- Do not flag: formatting, typography and language, speculative robustness, refactors and scope, tooling and tests that do not exist, house decisions, comments
- Flag confidently (accepted at a high rate)

---

## Verification protocol (every candidate, no exception)

A candidate becomes a finding only if it passes all of these.

1. **Concrete trigger.** Name the input, state or interleaving and the wrong outcome. For a
   concurrency claim, the exact interleaving. For a data claim, the rows that break.
2. **Current head.** The code exists at the head, at that line. Findings on a previous revision
   are a classic automated-review failure.
3. **Not handled elsewhere.** Check where humans looked before dismissing: global middleware
   (trimming, empty-to-null), the request validator or DTO that already validates, the exception
   filter that fills the message, auto-import configuration, lint configuration, the upstream
   guarantee (unique name, conflict status), the caller that already guards.
4. **Introduced or worsened by the work.** A pre-existing problem is reported as out of scope with
   a follow-up suggestion, never as blocking. Exception: a defect inside code the work rewrites (a
   migration, a refactor, a moved block) is in scope at LOW or NITPICK, since fixing it costs
   nothing in that diff. Parity with the base is not a verdict: "same as before" only says the
   work did not make it worse.
5. **The rule exists.** A cited convention is in the repository's rules (CLAUDE.md, lint config,
   review bot config, contributing guide) or is the measured majority of the surrounding code.
   Reviewers grep and count; an invented rule is dismissed.
6. **Local consistency.** When the surrounding code uses another style consistently, asking one
   line to change style is refused. Count occurrences before asking.
7. **Honest severity.** Style is never above LOW (see `comment-style.md`).
8. **Not already raised.** No existing thread, open or resolved, carries the same point; an
   author's reasoned answer is respected unless the head proves it wrong.

When a check depends on something you cannot see (production data, a system you cannot read),
the finding becomes a question.

**Findings from elsewhere** (another review bot, a colleague, a pasted list) pass the same
protocol before they are reported or fixed: run the claimed trigger. A claim about language or
library behaviour is checked by running it (`node -e`, a REPL, the library source), never assumed.
A finding that fails the trigger is answered with the evidence, not applied.

---

## Do not flag

### Formatting the tooling accepts
- Blank line before a `return` that opens its block or is a one-line guard. By far the most
  rejected automated remark. If the linter does not report it, do not report it.
- Layout produced by the formatter.
- `parseInt` radix in modern JavaScript and TypeScript runtimes.
- Import alias style on imports inside one module or feature, where relative imports are the norm.

### Typography and language
- Accented letters in user-facing strings, translation files, mails, templates, test titles, or
  data values in migrations. An ASCII rule, when a repository has one, targets typographic
  punctuation (long dashes, curly quotes, the ellipsis character) and emojis, not letters.
- Switching user-facing messages to another language than their neighbours in the same file.

### Speculative robustness
- Fallbacks for fields the producer always sets, or that a validator already guarantees.
- Guards against states that the data proves do not exist (check before asking).
- Low-probability races that need changes in systems the team does not own, or that an upstream
  guarantee already covers.
- Memoising or syncing a value that is constant per instance.

### Refactors and scope
- DRY or extraction of working code on a release branch, with no bug.
- Moving a single-caller private helper to a shared module.
- A shared base for two types used by independent workflows.
- Refactoring code scheduled for removal.
- Migrating an untouched module to a new pattern inside a change that only extends a condition.

### Tooling and tests that do not exist
- Findings sourced from a static analyser the repository does not install.
- Asking for a kind of test the repository does not run (unit tests with only an e2e runner,
  migration tests where there are none).
- Switching the mocking style when the module consistently uses another one.
- Fully typed fixtures where the file consistently uses partial casts.
- Moving test files against the repository's layout.

### House decisions
Anything the repository decided on purpose and documents or applies consistently: injection
style, logger idiom, database enum policy, logging of full payloads on internal failure paths,
comment policy. Check the rules and the majority before flagging.

### Comments
- Asking for comments that paraphrase code.

---

## Flag confidently (accepted at a high rate)

- Access-control gaps with the missing check named.
- Runtime bugs with a trace: invalid date crashing a list, undefined dereference, DST arithmetic,
  a cache storing the result instead of the promise, precision loss on long numeric strings.
- Data integrity: partial failure not propagated, truncated reads, an empty result overwriting
  data after an upstream error, a migration `down()` not symmetric with `up()`, a unique violation
  surfacing as 500.
- Validation that runs after a streamed response has started.
- Idempotency and compensation in retried units.
- Contract gaps: undocumented response or status, a payload accepted at runtime but missing from
  the schema, nullability that differs between type and schema, and every cross-service mismatch.
- Tests that cannot fail, with the regression that would pass them named.
- Technical or sensitive data shown to end users.
