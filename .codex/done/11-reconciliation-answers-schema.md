# PR 11 — Reconciliation capability answers schema

## Goal

Define the reconciliation-specific `capability_answers.reconciliation` schema.

## Shape

```json
{
  "capability_answers": {
    "reconciliation": {
      "name": "tenancy_rent_reconciliation",
      "source_event": "BankTransaction",
      "expected_record": "RentObligation",
      "settlement_record": "Payment",
      "exception_record": "ReconciliationCase",
      "input_modes": ["single", "batch"],
      "source_fields": {
        "external_id": "transaction_id",
        "amount": "amount",
        "date": "booked_at",
        "reference": "reference",
        "currency": "currency"
      },
      "expected_fields": {
        "id": "obligation_id",
        "amount": "expected_amount",
        "due_date": "due_date",
        "reference": "tenancy_reference",
        "status": "status"
      },
      "matching": {
        "amount_tolerance": 2.0,
        "date_window_days": 7,
        "auto_match_threshold": 85,
        "review_threshold": 50
      },
      "exception_policy": {
        "partial_payment": "create_case",
        "ambiguous_match": "manual_allocation",
        "unmatched": "unallocated_cash_case",
        "duplicate_possible": "manual_review"
      },
      "actions": {
        "create_settlement": "create_payment",
        "mark_paid": "mark_obligation_paid",
        "mark_partially_paid": "mark_obligation_partially_paid",
        "create_exception": "create_reconciliation_case"
      },
      "agent_endpoints": {
        "create_settlement": "create_payment",
        "mark_paid": "mark_obligation_paid",
        "mark_partially_paid": "mark_obligation_partially_paid",
        "create_exception": "create_reconciliation_case"
      }
    }
  }
}
```

The `actions` map binds to SoRLa `actions[].name`. The optional
`agent_endpoints` map binds to SoRLa `agent_endpoints[].id` when runtime
exposure is already declared in SoRLa. OperaLa should prefer locked endpoint
references when they exist and fall back to action handoff metadata with a
readiness warning when they do not.

## Acceptance criteria

- Schema validates complete answers.
- Schema rejects missing mapping fields.
- Schema supports currency.
- Schema supports exact single event and batch input modes.
- Schema validates thresholds as percentages from 0 to 100 and requires
  `auto_match_threshold >= review_threshold`.
