# PR 12 — Reconciliation operational handoff builder

## Goal

`greentic-operala wizard --answers answers.json` must produce deterministic operational
handoff metadata for reconciliation. For the pilot, it may also write a local
`.gtpack` compatibility artifact that OperaX can run, but production assembly
remains a `gtc` responsibility.

## Generated handoff

```text
target/operala/tenancy-rent-reconciliation/
  operala.yaml
  operala-handoff.json
  operala.build.lock
  capability/reconciliation.json
  bindings/sorx-http.template.json
  schemas/
    bank-transaction.schema.json
    daily-bank-transactions.schema.json
    decision-envelope.schema.json
  flows/
    ingest-transaction.flow.yaml
    ingest-daily-transactions.flow.yaml
    reconcile-one.flow.yaml
  components/
    reconciliation-runtime.wasm
  ui/
    reconciliation-exception.card.json
  tests/
    one-transaction.json
    daily-transactions.json
    expected-decisions.json
```

Optional Greentic pack output:

```text
target/gtpacks/tenancy-rent-reconciliation.gtpack
  manifest.cbor
  sbom.json
  flows/<flow>/flow.ygtc
  assets/operala/...
```

## Metadata

`operala.yaml` should include:

```yaml
schema: greentic.operala.handoff.v1
capability: reconciliation
extension: greentic.operala.reconciliation.v1
tenant_required: true
team_optional: true
sorla:
  source_digest: sha256:<digest>
  parser: greentic-sorla-lib
  required_schema: greentic.sorla.v0.2
sorx:
  transport: http
  url: runtime-provided
```

## Acceptance criteria

- Handoff metadata is created deterministically.
- Optional `.gtpack` is built by Greentic pack tooling and validates with the
  Greentic pack reader.
- build lock records input SoRLa digest, answers digest, extension version.
- generated handoff supports single transaction and daily batch.
- generated artifacts contain no plaintext secrets, provider credentials, or
  concrete customer SORX URLs.
