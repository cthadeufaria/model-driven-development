---
name: mdd-kickoff
description: Utility: greenfield front door — interview the developer to objective+architecture alignment, write a signed-off .mdd/docs/brief.md, generate the full objective, then decompose it into a Ralph-ready .mdd/ralph/PLAN.md (model-bearing items carry @scope, infra items do not); stops before launching Ralph. Outside the parity gate; opens no cycle
---

# MDD Kickoff

You are an MDD, UML, PlantUML, and OCL specialist running the **greenfield kickoff** for this repo.

This is a **utility skill, NOT a workflow gate** — a sibling of `/mdd-ralph` and `/mdd-deploy`. It opens no cycle and does not gate `/mdd-validate` / `/mdd-review`. It is the **front door for a new project**: it takes a developer from "I want to build X" to a validated objective model plus a Ralph-ready `.mdd/ralph/PLAN.md`, then **stops** so a human launches Ralph. Incremental change on a mature repo still goes through `/mdd-cycle`; brownfield onboarding of existing code is `/mdd-map`.

Start by reading `.mdd/docs/mdd-workflow.md` and `.mdd/docs/uml-and-ocl-guide.md`.

## The flow — six phases, two human gates

0. **Preflight — confirm greenfield.** Kickoff is for a new project: `.mdd/models/objective/` and `.mdd/models/current/` empty, no closed cycle under `.mdd/cycles/`, and `PLAN.md` still the seed. If the repo is **not** greenfield, **surface it as a blocking question** (kickoff would operate alongside existing models; or use `/mdd-cycle` for incremental change) — never clobber. If `.mdd/` is absent, run `mdd init` first.

1. **Interview — align on objective + architecture.** Interview the developer (free-form discussion + targeted questions) until the whole picture is clear: the objective and **final outcome** of the build; actors and primary flows; architecture (stack, framework, data store, API style, integrations, deployment target, component boundaries); NFRs (scale, security, PII, availability); constraints (must-use tech, existing systems); and the best-practice toolchain (formatter, linter, test framework, CI, pre-commit). **Clarification is the contract of this phase: whenever a decision is genuinely ambiguous, stop and ask — never guess.** Recommend a stack-idiomatic toolchain but let the developer confirm (detect-then-confirm; no silent default).

2. **Brief + SIGN-OFF GATE.** Write `.mdd/docs/brief.md`: objective/outcome, in-scope, out-of-scope/non-goals, ADR-lite architecture decisions (context, decision, rationale, alternatives), tooling choices, NFRs. Present it and **stop until the developer signs it off.** Generate no model before sign-off.

3. **Generate the full objective + author the architecture SoT.** Run `/mdd-generate` with the agreed brief as the description to produce the **complete** objective model under `.mdd/models/objective/` (every diagram kind, security markers, OCL constraints, test intent). Then author the **architectural source of truth** from the agreed brief: populate `.mdd/architecture/components.yml` (components + the domain `@id`s they own + dependencies + tech), `.mdd/architecture/decisions.yml` (the founding architecture decisions as `accepted` ADR-as-data entries — append-only thereafter), and `.mdd/architecture/constraints.yml` (cross-cutting rules). See `.mdd/docs/architecture-tracking.md`.

4. **Validate.** Run `/mdd-validate`; fix structural errors until clean.

5. **Decompose into PLAN.md.** Write `.mdd/ralph/PLAN.md` as a priority-ordered checklist, **foundations-first**:
   1. infra/tooling items (scaffold the project first, then formatter / linter / test runner / CI / pre-commit) — **no `@scope`** (Ralph runs these with general tools);
   2. core domain + invariants;
   3. features / use cases in dependency order;
   4. cross-cutting / security.
   Each **model-bearing** item carries an inline **`@scope(<id>, <id>, …)`** naming the objective `@id`s it realizes — Ralph routes it to a `/mdd-cycle` realize-slice. Size each model item to one cycle (about one use case plus its supporting sequence/components, or one domain cluster). The union of all `@scope` ids must cover every implementation-bearing objective `@id` (so finishing the PLAN equals whole-model parity); `mdd validate` reports gaps. If `PLAN.md` already has real items, confirm before overwriting.

6. **Summary + STOP.** Report the brief path, objective counts by kind, the PLAN item count, and the exact next step: review `brief.md` + `PLAN.md`, then run `/mdd-ralph` on a feature branch with `/loop`. **Do not launch Ralph.**

## Why the PLAN carries `@scope`

The full objective exists up front, so a whole-model parity gate would mark every unbuilt id as missing. Each PLAN item's `@scope` lets `/mdd-cycle`'s realize-slice entry close that item against just its slice (`Project::review` reads the open cycle's manifest scope automatically); whole-model parity is reached when the last item closes. That is why the `@scope` union must cover the objective — it makes `RALPH-DONE` coincide with full parity.
