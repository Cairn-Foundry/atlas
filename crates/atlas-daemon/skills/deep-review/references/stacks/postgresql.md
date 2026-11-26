# PostgreSQL (schema, queries, migrations)

SQL and migration traps that recur in changes touching PostgreSQL, whatever the ORM or migration
tool. The repository's own rules win. A migration is also a deploy-order question: see
`cross-service.md`.

## Migrations on a live table
- `CREATE INDEX` without `CONCURRENTLY` blocks writes. `CONCURRENTLY` cannot run inside a
  transaction: check the tool runs that migration outside one.
- Adding a foreign key or a `CHECK` on a large table without `NOT VALID` then a separate
  `VALIDATE CONSTRAINT` holds a lock while it scans.
- `ADD COLUMN ... DEFAULT <volatile>` or a column type change rewrites the table. A constant
  default is instant.
- `SET NOT NULL` scans the table; a `NOT NULL` column without a default breaks the inserts of the
  previous app version during deploy.
- Rename or drop of a column or table the running version still uses: expand and contract across
  two deploys.
- A backfill as one giant `UPDATE`: batch it. Rows deleted by a migration that cannot be restored:
  log them first.
- No `lock_timeout` on DDL: it queues behind a long transaction and blocks everything behind it.
- `down()` that is not the inverse of `up()`, or an edit to a migration already applied.
- `ddl-auto` or synchronize options enabled outside local development.

## Schema
- Foreign key columns without an index; duplicate or unused indexes; a partial index whose
  predicate does not match the query.
- `TIMESTAMP` without time zone; money in `float` or `money`; `serial` instead of identity;
  random UUIDv4 primary keys on a large table (PostgreSQL 18 has `uuidv7()`).
- Evolving value sets as `ENUM` (adding is easy, removing is not): `CHECK` or a lookup table.
- Uniqueness enforced only in application code: a unique index.

## Queries
- A function on an indexed column (`date_trunc`, `lower`) or a leading-wildcard `LIKE` defeats the
  index; a type mismatch in a predicate causes an implicit cast and a sequential scan.
- `NOT IN` with a nullable subquery returns nothing: `NOT EXISTS`.
- `OFFSET` pagination on a large table: keyset pagination. `SELECT *` in application code.
- `COUNT(*) > 0` for existence: `EXISTS`. An N+1 from the application: `= ANY($1)` or a join.
- `DELETE` or `UPDATE` without `WHERE`; `TRUNCATE` in application code.

## Transactions and concurrency
- Check-then-insert races under `READ COMMITTED`: a unique constraint plus `ON CONFLICT`. The
  `ON CONFLICT` target must match a unique index.
- `max(id) + 1` counters: a sequence.
- `SELECT ... FOR UPDATE` on several rows in inconsistent order deadlocks: lock in a fixed order.
  Queues use `FOR UPDATE SKIP LOCKED`.
- A transaction held open across an external call or user interaction.

## Security
- A table exposed through an API layer with Row Level Security missing or policies that are too
  broad. A policy that calls a function whose result does not depend on the row (such as
  `auth.uid()` or `current_setting()`) and is not wrapped in `(select ...)` can be called per row;
  the wrapped form runs once per statement (Supabase RLS guide, "Call functions with select").
- A policy that sub-SELECTs another table without `FOR SHARE` or a security definer function can
  leak through races (PostgreSQL docs, row security policies).
- `SECURITY DEFINER` without a fixed `search_path`; grants to `PUBLIC`.
- SQL built by string concatenation, including identifiers (allow-list them).

## Connections
- No `statement_timeout` or `idle_in_transaction_session_timeout`.
- Prepared statements with PgBouncer in transaction mode; a pool larger than the server can serve.
