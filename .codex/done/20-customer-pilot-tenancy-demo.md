# PR 20 — Customer pilot tenancy rent reconciliation demo

## Goal

Create a full demo showing:

1. OperaLa prompt creates `answers.json`.
2. OperaLa wizard creates reconciliation handoff metadata and an optional `greentic-pack` `.gtpack`.
3. OperaX runs the artifact against SORX HTTP endpoint.
4. Single and daily batch banking JSON are both supported.
5. SORX is updated through HTTP calls to SoRLa-declared actions/agent endpoints.

## Demo structure

```text
examples/tenancy/
  sorla.yaml
  prompt.txt
  answers.json
  banking/
    one-transaction.json
    daily-transactions.json
  sorx-fixtures/
    initial-state.json
    expected-after-single.json
    expected-after-batch.json
  mock-sorx/
    README.md
  expected/
    readiness.report.yaml
    decisions.batch.json
```

The sample `sorla.yaml` must use current SoRLa v0.2 YAML, not the old
`sorla: v0.1` / `entities` / `functions` shape.

## Demo command sequence

```bash
greentic-operala prompt --locale en-GB --sorla ./examples/tenancy/sorla.yaml \
  "$(cat ./examples/tenancy/prompt.txt)"

greentic-operala wizard --answers ./answers.json --locale en-GB

greentic-operax run ./target/gtpacks/tenancy-rent-reconciliation.gtpack \
  --tenant demo-tenant \
  --team property-ops \
  --sorx-url http://localhost:8088 \
  --input ./examples/tenancy/banking/one-transaction.json

greentic-operax run ./target/gtpacks/tenancy-rent-reconciliation.gtpack \
  --tenant demo-tenant \
  --team property-ops \
  --sorx-url http://localhost:8088 \
  --input ./examples/tenancy/banking/daily-transactions.json
```

## Expected results

```text
bank_tx_001 → matched → Payment created → RentObligation marked paid
bank_tx_002 → partial_payment → Payment created → ReconciliationCase created
bank_tx_003 → unmatched → ReconciliationCase created reason=unallocated_cash
```

## Acceptance criteria

- Demo can be run locally.
- Mock SORX HTTP server verifies expected calls.
- README includes exact commands.
- CI runs demo in dry-run and mock-write modes.
- Demo also validates the generated `operala-handoff.json` and build lock.
