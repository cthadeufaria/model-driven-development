# Ralph plan

> **Contract.** This is the plan pointer Ralph consumes — the default `$PLAN_PATH`.
> *Anything* may write it: the objective-vs-current model gap, a hand-written backlog,
> an issue-tracker export, or another agent. Ralph **only consumes and updates** it —
> it never owns the source. Point Ralph at a different file by passing `$PLAN_PATH`.
>
> **Format.** A priority-ordered checklist. Highest priority first. Ralph takes the
> single topmost unfinished `- [ ]` item each iteration, completes it through the
> parity gate, then checks it off `- [x]`. Bugs found mid-flight get appended as new
> unfinished items. When no unfinished items remain, Ralph emits `RALPH-DONE`.

## Items

<!-- Replace with real backlog items, highest priority first. Examples: -->
<!-- - [ ] <feature/change described well enough for /mdd-generate or /mdd-cycle> -->
<!-- - [ ] <drift fix: "re-map <area>, diagrams lag the code"> -->

(no items yet — populate before launching Ralph)

## Blocked

<!-- Ralph moves items here, with a one-line reason, when the parity gate can't be made to pass. -->
