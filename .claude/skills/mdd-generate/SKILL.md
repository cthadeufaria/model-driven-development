---
name: mdd-generate
description: Derive the objective view of the system from a description into .mdd/models/objective/
---

# MDD Generate

You are an MDD, UML, PlantUML, and OCL specialist for the **objective** view of the system.

Start by reading `.mdd/docs/mdd-workflow.md` and `.mdd/docs/uml-and-ocl-guide.md`. Treat `.mdd/models`, `.mdd/constraints`, and `.mdd/trace.yml` as authoritative MDD state.

Use this skill to derive the objective view from a description — a POC concept, a feature spec, a change request, or a bug-fix narrative. Diagrams go into `.mdd/models/objective/`.

## Preconditions

- The user must provide a description. If their message has no description, ask for one; do not run without it.
- Code may or may not exist. Generate works either way; it does not require an existing current-side baseline.

## Workflow

1. Read the description. If `.mdd/models/current/` is non-empty, also read those diagrams so the objective refines what exists rather than starting from scratch.
2. Extract actors, externally visible goals, key flows, domain concepts, component boundaries, and UI requirements from the description. Mark uncertainty with notes in the diagrams rather than inventing behavior.
3. Produce or refine PlantUML files under `.mdd/models/objective/`:
   - `.mdd/models/objective/use-cases/<name>.puml`
   - `.mdd/models/objective/sequences/<name>.puml`
   - `.mdd/models/objective/domain/<name>.puml`
   - `.mdd/models/objective/components/<name>.puml`
   - `.mdd/models/objective/states/<name>.puml` (when lifecycle behavior is non-trivial; same rule as map)
4. If the description involves UI, also write PlantUML Salt mockups under `.mdd/models/objective/mockups/<name>.puml` with `MCK-...` IDs and `@ref(...)` markers back to the use case or sequence supported, plus UI contract comments:
   - `@ui-route(/path)`
   - `@ui-viewport(desktop,1280,720)`
   - `@ui-element(UIC-..., role=button, name="Accessible name", required=true)`
   Generate Playwright spec scaffolds under `.mdd/tests/ui/` with `UIT-...` IDs and `framework: playwright`, and link them in `.mdd/trace.yml` under `generated_ui_tests`.
5. Add OCL constraints under `.mdd/constraints/` for domain invariants implied by the description, with `@id(OCL-...)` and `@ref(...)` to the relevant domain model ID.
6. **Security stereotypes** — when the description mentions any security concern, emit the matching `@sec(...)` marker on the affected objective element plus the inline `<<Stereotype>>` on the diagram. See `.mdd/docs/security-profile.md` for full syntax. The marker must live in the same file as its `host=` ID and uses pipe `|` as the list separator.

   - "authentication / login / sign-in / RBAC / admin-only" → `<<ByPassing>>` on the use case (or actor): `host=USE-<NAME>, link=<route>, allowed=<Role1>|<Role2>, denied=<Role>`. For an actor host, `host=ACTOR-<NAME>, role=<Role>`.
   - "encrypted / TLS / at-rest / in-transit / secrets / PII / GDPR sensitive" → `<<Encrypt>>` on the class or sequence participant: `algorithm=<cipher>, scope=at_rest|in_transit|both, field=<attribute>`.
   - "input length / max characters / size limit / bounded input" → `<<BufferOverflow>>` on the class: `field=<attribute>, max_length=<positive int>`.
   - "search / filter / SQL / query / user-supplied database parameter" → `<<SqlInjection>>` on the class: `field=<attribute>, sink=<repository or table>, sanitizer=parameterized|prepared-statement|orm|escape|stored-procedure`.
   - "rate limit / throttle / DoS / max concurrent / requests per second" → `<<Flooding>>` on the use case or component: `max_rate=<n>` and/or `max_concurrent=<n>`, plus `window=<duration>` and optionally `action=throttle|reject|queue`.
   - "session timeout / token TTL / expires after / one-time code" → `<<Expiration>>` on the class: `field=<attribute>, ttl=<duration>`.

   Add `id=SEC-<NAME>` only when the marker is a trace target or test-scaffold host. When the description is silent on a concern for a feature that is plainly out of scope, do not invent a marker — leave the element unmarked rather than emit fabricated values.
7. Add stable `@id(...)` markers using the prefixes from `.mdd/docs/uml-and-ocl-guide.md`. Use `@ref(...)` only between objective-side IDs.
8. Update `.mdd/trace.yml` with model-to-model trace links. Do not add `source_links` here — `/mdd-map` adds those after implementation.
9. **Author per-layer test intent + links** for the impl-bearing `@id`s, marking gap tests `expect: red-until-implemented` in the unified trace `tests`. Use-case (acceptance) tests are **executable Playwright e2e** specs keyed to the `USE-` id — reuse the UI/e2e runner; do **not** write Gherkin `.feature` files as the spec. The canonical documentation of a use case is its **diagram** (`@desc` + the sequence it realizes); generate any acceptance prose from the diagram, keeping the diagram the single source of truth (Gherkin-as-documentation is retired).
10. Hand off to `/mdd-validate`.

Keep the objective reviewable and specific. If a behavior is ambiguous, mark the ambiguity in the model and ask for review before treating it as implementation scope.
