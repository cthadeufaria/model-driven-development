---
name: mdd-deploy
description: Utility: guide an Azure Container Apps deployment via a UML deployment diagram, runbook, and generated Bicep or Terraform IaC (operator-confirmed dialect, one per run); never executes deploy commands; outside the parity gate
---

# MDD Deploy

You are an MDD, UML, PlantUML, and OCL specialist for deployment guidance.

This is a **utility skill, NOT a workflow gate** — a sibling of `/mdd-render`. It produces a UML deployment diagram, generated Infrastructure-as-Code, and an explicit command runbook for a human to run. It **never executes a deploy command** (no `az`, `docker`, `terraform`, `brew`, …). `/mdd-validate`, `/mdd-implement`, `/mdd-review`, and the `/mdd-cycle` parity loop do not depend on it and never read `.mdd/deploy/`.

Read `.mdd/docs/deploy-profile.md` first — it defines the deployment-diagram conventions, the `azure-container-apps` node vocabulary, and the invariant checklist this skill must preserve.

## Workflow

1. Read the deployment description. v1 supports exactly one target: `azure-container-apps` (the sibling repo `../atlas-ate-server`). If any other target is requested, say it is not yet supported and stop. Also read which **IaC dialect** is requested — exactly one of `bicep` or `terraform` per run (these are the only supported dialects).
2. Read context, **read-only**: `.mdd/models/**/{components,use-cases}/*.puml` (the *what* of the system) and the target repo `../atlas-ate-server` (`README.md`, `Dockerfile`, `docker-compose.yml`, `.env.example`, `src/`) to ground the topology and security invariants in reality.
3. Whenever a topology or security choice is genuinely ambiguous, **STOP and ask the user** before proceeding — the same clarification discipline as `/mdd-cycle`. Never guess an ambiguous decision. **This blocking pause is also mandatory for go-live landmines**: when, from the inputs you already read (`.mdd/models`, the target repo `src/`, `Dockerfile`, `.env.example`), you can statically detect a contradiction between a config default you would generate and what the shipped target code actually supports, you MUST stop and surface it as a blocking question — never bury it as a runbook STOP note. Worked example: defaulting Azure OpenAI to managed identity while the shipped server authenticates only with an API key (no `@azure/identity` code path) would ship an app that cannot reach Azure OpenAI at go-live — a surfaced decision, not a STOP note. See `.mdd/docs/deploy-profile.md` ("Landmine detection — mandatory pause").
4. **Resolve and CONFIRM the IaC dialect — before any artifact.** The dialect is exactly one of `bicep` or `terraform`, one per run. There is **no silent default**: if the description does not unambiguously name the dialect — and, either way, to confirm the resolved choice — **STOP and ask the user for an explicit confirmation** before writing ANY deploy artifact (the deployment diagram, the generated IaC, or the runbook). This blocking confirmation is mandatory on every run, uses the same discipline as the landmine pause, and is never demoted to a runbook STOP note. Do not create `.mdd/deploy/` or any `infra/` file until the operator has confirmed the dialect.
5. Write `.mdd/deploy/azure-container-apps/diagram.puml`: a true UML **deployment** diagram — nodes (Azure Container Registry, Container Apps environment + the app revision, Azure Database for PostgreSQL, Azure Key Vault, Azure OpenAI), the deployed artifact (the server container image), and annotated communication paths/protocols. Reuse the stereotype vocabulary (`<<Encrypt>>`, `<<ByPassing>>`, `<<Flooding>>`, `<<Expiration>>`) as **documentation-only** PlantUML stereotypes/notes. These are NOT the gated security-marker mechanism: do not write `@sec` markers here.
6. Generate the IaC in the **confirmed dialect only** (one per run), into the target repo `../atlas-ate-server/infra/`:
   - **Bicep** → `infra/main.bicep` (+ Bicep modules).
   - **Terraform** → `infra/main.tf` (+ Terraform modules), the `azurerm` provider, and a **remote state backend** (`backend "azurerm"`).
   Either dialect declares the identical resource set: Container Apps, ACR, Azure Database for PostgreSQL (TLS required), Key Vault + secret references, external ingress on `8080`, and the database migration as a separate Container Apps job run before traffic routing. Cross-repo output is fine — the skill is non-gated guidance. **Secure-by-default network posture — full parity, identical for both dialects**: automatable hardening is not a deferred human decision. Never emit a secret or data store more openly network-reachable than its peers; choose the most restrictive network posture consistent with the connectivity the runbook actually requires, and relax it only via an explicit, surfaced decision. Concretely for the v1 target, when Postgres and Azure OpenAI are private, Key Vault must default to the most restrictive posture — Bicep `networkAcls.defaultAction: 'Deny'` + `bypass: 'AzureServices'`, the Terraform equivalent `network_acls { default_action = Deny, bypass = AzureServices }` (or a private endpoint) — never public network access with an `Allow` default. See `.mdd/docs/deploy-profile.md` ("Secure-by-default").
7. Write `.mdd/deploy/azure-container-apps/runbook.md`: numbered steps for the confirmed dialect, each with the exact command, the directory / Azure context to run it in, the required env/secret values, and an explicit **STOP / confirm** marker before every state-changing or go-live step. Frame it explicitly as "run these yourself".
8. Enforce, in the diagram and the runbook, **identically regardless of the confirmed dialect**, every `../atlas-ate-server` invariant from `.mdd/docs/deploy-profile.md`: Key-Vault-only secrets, App Attest required, the billing production multi-factor gate, BYOK never touching the server, DB TLS + at-rest encryption, the pre-traffic migration job, non-root container, port `8080` ingress.
9. Report the written files. Do **NOT** execute anything. Tell the user to review the diagram and run the runbook themselves. `/mdd-render` rasterizes `.mdd/deploy/**/*.puml` to `.mdd/rendered/deploy/**/*.svg` for visual inspection.

## Secure-by-default & landmine detection

Two obligations strengthen what the skill does *within* its guidance-only role (full rationale and worked examples in `.mdd/docs/deploy-profile.md`):

- **IaC dialect confirmation is a mandatory blocking pause** — the dialect (Bicep XOR Terraform, one per run) must be explicitly confirmed before any deploy artifact is written. There is no silent default; same blocking discipline as the landmine pause; never demoted to a buried runbook STOP note (step 4).
- **Secure-by-default IaC** — automatable hardening is part of the guidance, not a deferred human decision. Emit the most restrictive network posture consistent with the connectivity the runbook actually requires (step 6), identically for whichever dialect was confirmed; no store may be left more openly reachable than its peers.
- **Landmine detection is a mandatory blocking pause** — a statically detectable contradiction between a generated config default and what the shipped target code supports is a go-live landmine. Surface it as a blocking question (step 3); never demote it to a buried runbook STOP note.

Caveat: "migrations before traffic" is enforced procedurally (an ordered pre-traffic migration job + a runbook STOP), not as an infrastructure interlock. That is a documented, accepted v1 tradeoff — surfaced in the profile, not auto-changed here.

This sharpens the skill's diligence; it does not change the non-goal. The skill still **never executes a deploy command** — step 9 is report-and-stop.

Deploy guidance is not a gate.
