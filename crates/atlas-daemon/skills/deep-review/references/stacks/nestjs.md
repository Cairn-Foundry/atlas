# NestJS (with TypeORM, class-validator, Swagger)

Framework traps and review checks that recur in NestJS services. The repository's own rules win.

## DTO and validation
- `@IsOptional()` lets `null` through as well as `undefined`: type the field `field?: T | null`,
  and keep TS type, `@ApiProperty({ nullable })` and interfaces aligned.
- Query params arrive as strings: `@Type(() => Number)` with `@IsInt()`; ids `@IsInt()` +
  `@Min(1)`, not `@IsNumber()`. `@IsNotEmpty()` next to `@IsNumber()` is redundant.
- Arrays of objects need a nested DTO, `@ValidateNested({ each: true })` and `@Type(() => X)`,
  otherwise the items are not validated. `@ArrayMinSize(1)` when empty is invalid.
- `@ApiProperty({ default })` only documents: the class field needs its own `= value`.
- A class-level constraint on an optional field is skipped when the field is absent;
  conditional rules need `@ValidateIf`.
- PATCH payloads: only `undefined` means "leave unchanged"; `null` is a value. A falsy check drops
  it.
- A payload accepted at runtime but absent from the Swagger `oneOf` is an undocumented contract.
- Transformations belong in the DTO (`@Transform`); near-duplicate DTOs use `PartialType`,
  `OmitType`, `PickType` or extension.

## Errors
- A `catch` converting every upstream error into `NotFoundException`: check
  `error.response?.status === 404` first and rethrow the rest (network errors have no
  `response`).
- Upstream failure: `BadGatewayException` or a domain exception with a registered code, not a bare
  `Error` that becomes 500.
- Log with the `Logger`, then rethrow the original error; do not wrap it into a context-free new
  one.
- An exception filter that fills messages from codes: passing the message again duplicates it.
- Report 5xx to error tracking, not 4xx the client already shows.

## Layering
- No business logic in controllers. A client service for an external system stays a specialist
  of that system.
- Configuration through `ConfigService`, not `process.env`; read it in the constructor, not in a
  field initialiser that runs before injection.
- A new module for a single function, or a shared module exporting a service used by one module.

## Security
- Every route by id checks the caller's scope (tenant, organisation, customer) in addition to the
  role guard. Role decorators default to own-scope; "any" needs a reason.
- Controllers without guards, including test or maintenance controllers.

## TypeORM
- `findOne({ where: { id: value } })` with `value` undefined drops the condition and returns an
  arbitrary row: guard the value or use `Equal(value)`.
- An index created in a hand-written migration but not declared with `@Index` on the entity is
  dropped by the next `migration:generate`. Indexes the ORM cannot express:
  `@Index(name, { synchronize: false })`.
- Generated migrations often contain unrelated statements (drift from other branches): remove
  them. Several migrations on the same table in one change: squash if not deployed.
- `down()` must be the inverse of `up()`, and safe with foreign keys.
- GIN `jsonb_path_ops` does not support the `?|` and `?&` operators.
- Loading rows to count them: `getCount()`. Filtering in memory after a paginated query breaks
  totals.
- Rewriting a whole jsonb column from a stale object loses concurrent changes: merge the
  sub-field.

## Concurrency and messaging
- In-memory flags or locks in a provider protect only one process; several replicas need a
  database advisory lock or an external lock.
- Two callbacks that can race (finished, cancelled): an advisory lock on the aggregate plus a
  durable completion marker, not a read-then-publish check.
- Consumers of retried messages are idempotent; the local child record is written before the
  external call.
- Async helpers returning a promise must be awaited (`void` only with an explicit catch).

## Search indexes (Elasticsearch, OpenSearch)
- An index not created explicitly gets a dynamic mapping on first write: register new indices in
  the bootstrap that creates them with their mapping.
- Write paths that change indexed data reindex, once.
- `size` above 10 000 or deep pagination needs `search_after` or scroll.
- Alias and index are different things: an alias lookup returns indices.
- Querying the index with the criteria beats loading every document and filtering in code.

## Tests
- Assertions that can fail: status and error code asserted, no bare `toBeDefined()`, no
  `expect.anything()` on the field that matters, non-sequential ids in fixtures.
- Fake timers restored in `finally` or `afterEach`; concurrency tests with `Promise.all`.
- Specs must compile (`tsc` on the test config).
