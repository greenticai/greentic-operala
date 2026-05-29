# PR 07 — OperaLa prompt authoring loop

## Goal

Implement `greentic-operala prompt --sorla sorla.yaml "..."` as an interactive LLM-assisted authoring flow that outputs only `answers.json`.

## Behaviour

The prompt command should:

1. Load SoRLa through `greentic-sorla-lib`.
2. Infer the operational capability.
3. Select a matching extension.
4. Ask follow-up questions if required.
5. Produce valid `answers.json`.

## Follow-up example

```text
I found these possible source events:
1. BankTransaction
2. PaymentWebhook
3. ManualPaymentImport

Which one should be used as the incoming observed event?
```

## Safety

- Do not build handoff artifacts or `.gtpack` output.
- Do not apply SoRLa patches.
- Do not call SORX.
- Do not mutate production resources.
- Record unanswered assumptions in `answers.json.assumptions`.
- Keep any patch proposal as a `greentic.sorla.patch.v1` document for a
  later SoRLa/Designer/gtc authoring step.

## Acceptance criteria

- Prompt can infer reconciliation from "bank transactions", "rent payment", "invoice payment", etc.
- Missing information triggers follow-up questions.
- Final output validates against `greentic.operala.answers.v1`.
- Capability-specific answers are nested under `capability_answers.reconciliation`.
- Inference prefers SoRLa agent endpoints/actions with stable IDs over labels or
  localized text.
