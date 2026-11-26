# Java (with Spring Boot, Spring Data JPA, Bean Validation)

Framework traps and review checks that recur in Spring Boot services. The repository's own rules
win.

## Transactions and persistence
- `@Transactional` is proxy-based: on a private method, or on a method called from the same class
  (self-invocation), it does nothing. Call through another bean or restructure.
- Only `RuntimeException` and `Error` roll back by default: a checked exception thrown out of a
  `@Transactional` method commits unless `rollbackFor` is set.
- `@Transactional` belongs on the service method, not on the controller or the repository;
  `readOnly = true` on queries.
- External calls (HTTP, messaging) inside the transaction: a rollback does not undo them, and the
  connection stays held while waiting. Publish after commit.
- Lazy relations walked in a loop are an N+1: fetch join, entity graph or a DTO projection.
  `LazyInitializationException` fixed by `spring.jpa.open-in-view=true` hides the problem.
- `equals`/`hashCode` on a generated id (null before persist) or Lombok `@Data` on an entity.
- `ddl-auto=update` outside a local profile; schema changes belong in Flyway or Liquibase.
- A counter read then incremented (`findMax() + 1`) is a race: a sequence or a unique constraint.

## Validation and DTOs
- Entities returned from controllers leak fields and trigger lazy loading: a DTO record.
- `@Valid` missing on the `@RequestBody`, or on the nested object field (validation does not
  cascade without `@Valid`). `@Valid` alone does not trigger method validation: a constraint
  annotation (`@NotNull`, `@Min`) does.
- Path and query parameter constraints: from Spring Framework 6.1 the built-in method validation
  handles them (`HandlerMethodValidationException`) and a class-level `@Validated` must be removed
  to use it; before 6.1 the class needed `@Validated`. Flag a class-level `@Validated` on a 6.1+
  controller only when the project depends on the AOP behavior.
- A required field typed as a primitive `int` defaults to 0 when absent: `Integer` plus `@NotNull`.
- PATCH payloads: absent and `null` are different; a plain DTO cannot tell them apart.

## Errors
- `@ExceptionHandler` returning `e.getMessage()` of an infrastructure exception, or a stack trace.
  One error shape (`ProblemDetail`), one meaning per status.
- Empty `catch`, `catch (Exception e)` that continues, or log-and-rethrow (the stack trace appears
  twice). Wrap with the cause kept.
- `Optional.get()` without a check; `Optional` as a field or parameter; `null` returned from a
  public method.

## Security
- A route by id that checks the role but not the caller's scope (tenant, owner).
- `permitAll()` on a wide matcher, `csrf.disable()` on a browser-session app, CORS `*` with
  credentials.
- Method security (`@PreAuthorize`) on a controller reached through another path that has none.
- Secrets, tokens or PII in `application.yml`, in logs or in exception messages.
- Passwords hashed with a fast hash (MD5, SHA-*); a bcrypt work factor below OWASP's minimum of 10
  (Spring's default strength is 10; tune it so a hash takes about one second); a hand-picked
  encoder instead of `DelegatingPasswordEncoder`, which allows upgrading later. bcrypt reads at most
  72 bytes of input. The repository's own minimum wins.

## Concurrency and time
- State in fields of a singleton bean (`@Service`, `@Component`) is shared by every request.
- `@Async` called from the same class does not run async; its exceptions vanish without a handler.
  `@Scheduled` runs on every replica: a lock or a single scheduler.
- `ThreadLocal` carrying request data under virtual threads: `ScopedValue` or explicit parameters.
  `parallelStream()` with blocking I/O starves the common pool.
- `LocalDateTime` for an instant: `Instant` or `OffsetDateTime`. `new Date()` and `Calendar`.
- Money as `double`; `new BigDecimal(0.1)`; `BigDecimal.equals` where `compareTo` is meant.

## Tests
- `@SpringBootTest` for what a slice (`@WebMvcTest`, `@DataJpaTest`) covers: slow and coupled.
- An in-memory H2 standing in for PostgreSQL hides dialect and locking behaviour: Testcontainers.
- `Thread.sleep` waiting for async work: Awaitility.
- `@DirtiesContext` used to hide shared state between tests.

## Boot 4 migration residue
- `@MockBean` and `@SpyBean` are removed: `@MockitoBean` and `@MockitoSpyBean`.
- Jackson 3 moves its packages and group IDs from `com.fasterxml.jackson` to `tools.jackson`: a
  leftover `com.fasterxml.jackson.databind` import on a Boot 4 project is a missed migration. The
  annotations are the exception: `jackson-annotations` keeps `com.fasterxml.jackson.annotation`, so
  those imports are correct. Boot 4 ships a deprecated `spring-boot-jackson2` module as a stop-gap.
  Check the project's Boot version before flagging.
