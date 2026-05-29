# PR 09 — Reconciliation readiness analyser

## Goal

The reconciliation extension must inspect SoRLa and determine whether it can
build a pilot-ready operational handoff for reconciliation.

## Required SoRLa concepts

```yaml
source_event:
  examples: [BankTransaction, PaymentWebhook, BankingTransactionImported]

expected_record:
  examples: [RentObligation, Invoice, ExpectedPayment]

settlement_record:
  examples: [Payment, Receipt, Allocation]

exception_record:
  examples: [ReconciliationCase, ManualReviewCase, PaymentException]

actions_or_agent_endpoints:
  - create_payment
  - mark_expected_record_paid
  - mark_expected_record_partially_paid
  - create_reconciliation_case
```

Readiness should use parsed SoRLa v0.2 data: `records`, `events`, `actions`,
`agent_endpoints`, field references, record access, and endpoint authorization.
Localized labels and aliases may help discovery, but locked IDs and canonical
hashes are the machine contract.

## Output

```yaml
schema: greentic.operala.readiness.v1
capability: reconciliation
status: ready
found:
  source_event: BankTransaction
  expected_record: RentObligation
  settlement_record: Payment
  exception_record: ReconciliationCase
  actions:
    create_settlement: create_payment
    mark_paid: mark_obligation_paid
    mark_partially_paid: mark_obligation_partially_paid
    create_exception: create_reconciliation_case
missing: []
warnings: []
```

## Ambiguity handling

If multiple candidate SoRLa records, actions, or agent endpoints are found,
status should be:

```text
unsafe_or_ambiguous
```

unless answers explicitly select one.

## Acceptance criteria

- Ready SoRLa produces `status: ready`.
- Missing records produce `status: needs_sorla_changes`.
- Ambiguous records produce `status: unsafe_or_ambiguous`.
- Report is localized in human summary but machine fields remain stable.
- Missing agent endpoints may be non-blocking if matching `actions` exist for
  handoff, but the report should warn that runtime exposure will require SORX or
  `gtc` assembly.
