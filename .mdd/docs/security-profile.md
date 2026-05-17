# MDD Security Profile

Security-sensitive use cases, sequences, classes, and components carry an inline
UML stereotype on the diagram element plus an `@sec(...)` comment marker that
records its tagged values. The marker MUST live in the same file as the
`@id(...)` it references via `host=`, and refs resolve **within the same side**
(a `current/` marker never resolves to an `objective/` host or vice versa).

Reserved ID prefixes:

- `SEC-...` — security requirement / annotation host. Optional `id=SEC-<NAME>`;
  participates in per-side ID uniqueness like any other `@id(...)`.
- `THR-...` — misuse case / threat.

## Marker syntax

```
' @sec(stereotype=<Name>, host=<ID>, <tagged values...>, id=SEC-<NAME>)
```

- One marker per line, opened with the PlantUML comment quote `'`.
- Key/value pairs are comma-separated; list-valued tags use pipe `|` as the
  separator (e.g. `denied=Anonymous|Customer`).
- `stereotype=` and `host=` are always required. `id=SEC-...` is optional — add
  it only when the marker is a trace target or test-scaffold host.
- The same `<<Stereotype>>` must also appear inline on the hosted diagram
  element.

## Active stereotype catalog

A marker fails validation if `stereotype=` is not in this catalog, if `host=`
does not resolve to a same-side `@id(...)` of an accepted host kind, if a
required tagged value is missing, if an enumerated value is outside its set, or
if an integer tag is not a positive integer.

### `<<ByPassing>>` — access-control bypass

- **Host kinds:** actor or use case.
- **Required:** for a use-case host, `link=<route>` and `allowed=<Role>` (pipe
  list). For an actor host, `role=<Role>`.
- **Optional:** `denied=<Role>` (pipe list of explicitly rejected roles).

### `<<Encrypt>>` — field or channel encryption

- **Host kinds:** class or sequence participant.
- **Required:** `algorithm=<cipher>` (e.g. `AES-256-GCM`,
  `TLS1.3_AES_128_GCM`), `scope=in_transit|at_rest|both`.
- **Optional:** `field=<attribute>` when scoped to a single field.

### `<<BufferOverflow>>` — bounded-input length guard

- **Host kinds:** class.
- **Required:** `field=<attribute>`, `max_length=<positive int>`.

### `<<SqlInjection>>` — bound SQL parameter with sanitizer

- **Host kinds:** class.
- **Required:** `field=<attribute>`, `sink=<repository or table>`,
  `sanitizer=parameterized|prepared-statement|orm|escape|stored-procedure`.
- String concatenation that builds SQL must **not** be marked, so security
  parity review flags the gap.

### `<<Flooding>>` — rate or concurrency limit

- **Host kinds:** use case or component.
- **Required:** at least one of `max_rate=<positive int>` or
  `max_concurrent=<positive int>`, plus `window=<duration>`.
- **Optional:** `action=throttle|reject|queue`.

### `<<Expiration>>` — session / token TTL

- **Host kinds:** class.
- **Required:** `field=<attribute>`, `ttl=<duration>` (e.g. `15m`, `24h`).

## Enumerated value sets

- `scope`: `in_transit`, `at_rest`, `both`.
- `sanitizer`: `parameterized`, `prepared-statement`, `orm`, `escape`,
  `stored-procedure`.
- `action`: `throttle`, `reject`, `queue`.

## Examples

```
' @sec(stereotype=ByPassing, host=USE-CHANGE-BOOK-PRICE, link=/admin/books, allowed=Admin, denied=Anonymous|Customer, id=SEC-ADMIN-PRICE-GUARD)
usecase "Change book price" as ChangePrice <<ByPassing>>

' @sec(stereotype=BufferOverflow, host=DOM-USER-INPUT, field=email, max_length=254, id=SEC-EMAIL-LEN)
class UserInput <<BufferOverflow>>
```

## Brownfield (`/mdd-map`) guidance

When mapping existing code, annotate the current-side element with the marker
that reflects **what the code actually enforces today**, not what it should:

- `<<ByPassing>>` — route guards, auth middleware (`requires_auth`,
  `requires_role`, `@PreAuthorize`, FastAPI `Depends`, Express middleware, Axum
  extractors), in-handler role checks.
- `<<Encrypt>>` — TLS config, `https://`, `crypto.encrypt`, KMS calls,
  `ENCRYPTED BY` columns, `@Encrypted` annotations.
- `<<BufferOverflow>>` — explicit length checks, Pydantic `max_length`, Joi
  `.max(N)`, `VARCHAR(N)`, `#[serde(max_length=N)]`.
- `<<SqlInjection>>` — ORM usage or parameterized queries (record
  `sanitizer=`). String-concatenated SQL stays unmarked.
- `<<Flooding>>` — rate-limit middleware, worker-pool sizes, `Semaphore::new`.
- `<<Expiration>>` — JWT `exp`, session cookie `maxAge`, Redis `TTL`,
  `expires_at` columns.

Intentionally public, unauthenticated endpoints stay unmarked. When a concern
is plainly out of scope, leave the element unmarked rather than fabricate
values.

## Security parity (`/mdd-review`)

`/mdd-review` runs a security-parity pass as its second pass (after ID parity).
It extracts every `@sec(...)` marker from both sides, keys each one by
`(host, stereotype, sorted params)` **excluding** `id=SEC-...` (so two markers
with the same body but different SEC- IDs match), and diffs objective vs
current:

- **missing security marker** — a marker the objective requires that the
  current (code-derived) side does not carry: the design demands a guard the
  code does not enforce. Leaving a security-sensitive element unmarked on the
  current side during `/mdd-map` is the intended way to surface this gap.
- **extra** — a current-side marker absent from objective (informational).

Behavior is governed by `.mdd/config.yml`:

```yaml
security:
  parity_check: error   # default
```

- **`error` (default — security-by-default)**: a missing security marker
  blocks cycle closure exactly like a missing `@id`. `/mdd-review` does not
  pass until the gap is closed in `/mdd-implement` (and re-mapped) or the
  objective marker is intentionally removed.
- **`warn`**: missing markers are still reported and a diff is still written,
  but they do not by themselves block closure. Opt-down for projects not yet
  enforcing security parity by setting `security.parity_check: warn`.

On any missing security marker an annotated
`.mdd/rendered/review/<diagram>.security.diff.puml` is written; render it with
`/mdd-render` to inspect the gap visually.
