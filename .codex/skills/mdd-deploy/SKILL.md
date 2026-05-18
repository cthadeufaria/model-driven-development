---
name: mdd-deploy
description: Utility: guide an Azure Container Apps deployment via a UML deployment diagram, runbook, and generated Bicep IaC; never executes deploy commands; outside the parity gate
---

# MDD Deploy

You are an MDD, UML, PlantUML, and OCL specialist for deployment guidance.

This is a **utility skill, NOT a workflow gate** — a sibling of `/mdd-render`. It produces a UML deployment diagram, generated Infrastructure-as-Code, and an explicit command runbook for a human to run. It **never executes a deploy command** (no `az`, `docker`, `brew`, …). `/mdd-validate`, `/mdd-implement`, `/mdd-review`, and the `/mdd-cycle` parity loop do not depend on it and never read `.mdd/deploy/`.

Read `.mdd/docs/deploy-profile.md` first — it defines the deployment-diagram conventions, the `azure-container-apps` node vocabulary, and the invariant checklist this skill must preserve.

## Workflow

1. Read the deployment description. v1 supports exactly one target: `azure-container-apps` (the sibling repo `../atlas-ate-server`). If any other target is requested, say it is not yet supported and stop.
2. Read context, **read-only**: `.mdd/models/**/{components,use-cases}/*.puml` (the *what* of the system) and the target repo `../atlas-ate-server` (`README.md`, `Dockerfile`, `docker-compose.yml`, `.env.example`, `src/`) to ground the topology and security invariants in reality.
3. Whenever a topology or security choice is genuinely ambiguous, **STOP and ask the user** before proceeding — the same clarification discipline as `/mdd-cycle`. Never guess an ambiguous decision.
4. Write `.mdd/deploy/azure-container-apps/diagram.puml`: a true UML **deployment** diagram — nodes (Azure Container Registry, Container Apps environment + the app revision, Azure Database for PostgreSQL, Azure Key Vault, Azure OpenAI), the deployed artifact (the server container image), and annotated communication paths/protocols. Reuse the stereotype vocabulary (`<<Encrypt>>`, `<<ByPassing>>`, `<<Flooding>>`, `<<Expiration>>`) as **documentation-only** PlantUML stereotypes/notes. These are NOT the gated security-marker mechanism: do not write `@sec` markers here.
5. Generate `../atlas-ate-server/infra/main.bicep` (+ modules): Container Apps, ACR, Azure Database for PostgreSQL (TLS required), Key Vault + secret references, external ingress on `8080`, and the database migration as a separate Container Apps job run before traffic routing. Cross-repo output is fine — the skill is non-gated guidance.
6. Write `.mdd/deploy/azure-container-apps/runbook.md`: numbered steps, each with the exact command, the directory / Azure context to run it in, the required env/secret values, and an explicit **STOP / confirm** marker before every state-changing or go-live step. Frame it explicitly as "run these yourself".
7. Enforce, in both the diagram and the runbook, every `../atlas-ate-server` invariant from `.mdd/docs/deploy-profile.md`: Key-Vault-only secrets, App Attest required, the billing production multi-factor gate, BYOK never touching the server, DB TLS + at-rest encryption, the pre-traffic migration job, non-root container, port `8080` ingress.
8. Report the written files. Do **NOT** execute anything. Tell the user to review the diagram and run the runbook themselves. `/mdd-render` rasterizes `.mdd/deploy/**/*.puml` to `.mdd/rendered/deploy/**/*.svg` for visual inspection.

Deploy guidance is not a gate.
