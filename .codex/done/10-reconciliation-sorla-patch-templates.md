# PR 10 — Reconciliation SoRLa patch proposal templates

## Goal

Generate minimal `sorla.patch.json` when SoRLa lacks records, actions, or agent
endpoints required for reconciliation.

## Patch rules

- Patch must be additive where possible.
- Do not rename existing records, actions, or agent endpoints.
- Do not delete fields.
- Do not modify existing actions or agent endpoints unless explicitly requested.
- Patch application requires `approval.apply_sorla_patch = true`.
- Patch proposals use the existing SoRLa semantic patch contract
  `greentic.sorla.patch.v1`; do not invent an OperaLa-specific patch format.

## Example patch

```json
{
  "schema": "greentic.sorla.patch.v1",
  "reason": "required_for_operala_reconciliation",
  "operations": [
    {
      "op": "add-record",
      "record": {
        "name": "Payment",
        "source": "native",
        "fields": [
          {"name": "payment_id", "type": "uuid", "rules": {"unique": true}},
          {"name": "obligation_id", "type": "uuid", "references": {"record": "RentObligation", "field": "obligation_id"}},
          {"name": "amount", "type": "decimal", "rules": {"min": 0, "precision": 12, "scale": 2}},
          {"name": "received_at", "type": "datetime"},
          {"name": "source_event_id", "type": "string"}
        ]
      }
    },
    {
      "op": "add-record",
      "record": {
        "name": "ReconciliationCase",
        "source": "native",
        "fields": [
          {"name": "case_id", "type": "uuid", "rules": {"unique": true}},
          {"name": "source_event_id", "type": "string"},
          {"name": "expected_record_id", "type": "uuid"},
          {"name": "reason", "type": "string"},
          {"name": "status", "type": "string"}
        ]
      }
    },
    {
      "op": "add-action",
      "action": {"name": "create_payment"}
    },
    {
      "op": "add-action",
      "action": {"name": "create_reconciliation_case"}
    }
  ]
}
```

The exact operation names should match the SoRLa semantic patch API available
from `greentic-sorla-lib`. The example above is illustrative, not permission to
invent a second patch engine.

## Legacy anti-example

Do not generate patches in the old shape:

```yaml
schema: greentic.sorla.patch.v1
reason: required_for_operala_reconciliation

add_entities:
  Payment:
    fields:
      payment_id: string
      obligation_id: ref RentObligation
      amount: money
      received_at: datetime
      source_event_id: string

  ReconciliationCase:
    fields:
      case_id: string
      source_event_id: string
      expected_record_id: string
      reason: string
      status:
        type: enum
        values: [open, resolved, rejected]

add_functions:
  create_payment:
    input:
      obligation_id: string
      amount: money
      received_at: datetime
      source_event_id: string

  create_reconciliation_case:
    input:
      source_event_id: string
      expected_record_id: string
      reason: string
```

SoRLa now models `records`, `actions`, and `agent_endpoints`, not
`entities`/`functions`.

## Acceptance criteria

- Patch generated when missing concepts.
- Patch is written to build work dir.
- Build summary clearly says patch was proposed, not applied.
- Patch validates against the real SoRLa patch schema/API before being written.
