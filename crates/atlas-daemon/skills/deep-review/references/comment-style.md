# Comment style

Modelled on the review comments that led to a fix on the first round. One-word comments ("remove?",
"why?", "not needed") needed a second round almost every time: do not imitate them.

## Contents

- Language and tone
- Shape of a finding
- Suggestion blocks (GitLab and GitHub)
- Severity
- Examples (fictional)
- Report template (terminal)
- Summary note on an MR or PR (when posting)

## Language and tone

- The report is written in the user's language. Comments posted on an MR or PR use the language
  of that repository's existing review threads; if there are none, the user's language.
- Direct, no filler praise, no emojis. Identifiers in backticks.
- Code in suggestions follows the repository's language rules for identifiers, comments and
  user-facing text.

## Shape of a finding

1. **Fact** - what the code does, with `file:line`, or the call chain when the problem crosses
   files or services.
2. **Mechanism** - why it breaks: which call, which state, which input.
3. **Consequence** - the effect for the user or the data, in domain terms.
4. **Fix** - a suggestion block when local (under about 15 lines), one or two options otherwise,
   or an explicit question when intent is unknown ("intentional removal or a leftover of the
   refactor?").
5. **Other sites** - every other place with the same problem, in the same finding.

A convention or a one-line bug stays short: one sentence with its reason, plus a suggestion. Use a
question when intent is unclear or depends on an unverified fact; an imperative with a reason when
a documented convention applies.

## Suggestion blocks (GitLab and GitHub)

````
```suggestion
<exact replacement of the commented line or range>
```
````

GitLab accepts `suggestion:-N+M` to cover N lines above and M below. The replacement must be exact
and complete for the range: a suggestion that duplicates lines breaks the build when applied.

## Severity

| Severity | Use when |
|---|---|
| CRITICAL | Exploitable access gap, data loss or corruption, irreversible state, production crash |
| HIGH | Wrong business behaviour, silent degradation, contract break between services, missing test on the headline behaviour |
| MEDIUM | Will cause problems over time: missing error code, layering, duplication that will drift, a test that cannot fail |
| LOW | Clarity, naming, a documented convention |
| NITPICK | Optional idea, labelled as optional |

Posted comments start with `**[SEVERITY]**` on their own line, so that fix tooling can parse them.

## Examples (fictional)

Business regression, mechanism and consequence:

> `src/checkout/CheckoutForm.vue:212`: the `if (!saved) return` guard was merged into the payment
> status check. `isSuccess` of a mutation goes back to `false` when the next mutation starts,
> which is exactly what the new `retryPayment` does. The watcher then shows "payment failed" while
> nothing failed. Keep the two checks separate.

Error handling with a scenario:

> This `catch` turns every error from the inventory service into `ProductNotFound`. When the
> inventory service is down, the user is told the product does not exist and nothing reaches the
> error tracker. Check `error.response?.status === 404` first and rethrow the rest.

Retry, as a question:

> Is this call idempotent? The job has `tries = 3`; if the second step fails, the first one
> (`createInvoice`) runs again and the customer gets two invoices.

Cross-service:

> `orders-api` publishes `order.refunded` with `amount` in major units
> (`src/refunds/refunds.service.ts:88`), and this handler reads `payload['amount_cents']`
> (`app/Listeners/IssueCreditNote.php:31`). Every credit note is issued for 0. Align on one field;
> the producer's name is the one the other consumers already use.

One-line bug:

> `||` falls back when the discount is 0.
> ```suggestion
> const discount = order.discount ?? null;
> ```

## Report template (terminal)

```markdown
# Deep review - <work name or ticket>

**Sources**
1. <repo> <kind> <ref> - <role in the work>

**Verdict:** <changes required | ok with remarks | ok>
<two or three lines: the main risks>

## Cross-service
- [SEVERITY] <source A file:line> <-> <source B file:line> - <one line>

## <source 1>
- [SEVERITY] `file:line` - <finding, per the shape above>

## <source 2>
- ...

## Verified
- <what was actually checked and how: callers grepped, other side read, tests read - one line each>

## Deploy order and notes
1. <step> - <reason>

## Out of scope (follow-up suggested)
- <pre-existing problem, where, why it matters>

## Declined to judge
- <what could not be assessed and why: no access to the other side, no runner, a fact left as a question> (optional, omit when empty)

## Dropped at verification
- <count> candidates, main reasons in one line each
```

Omit empty sections. "Verified" is mandatory: the strongest reviews say what was verified rather
than believed.

## Summary note on an MR or PR (when posting)

The same structure restricted to that MR or PR, plus its cross-service findings with a link to the
other MR or PR, and the deploy order of the whole work.
