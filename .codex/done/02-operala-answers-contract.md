# PR 02 — OperaLa answers contract

## Goal

Define the canonical `answers.json` format used by both prompt and wizard.
The answers contract should be operational and extension-oriented; it references
SoRLa as an upstream source contract and must not embed a competing SoRLa model.

## Schema name

```text
greentic.operala.answers.v1
```

## Required fields

```json
{
  "schema": "greentic.operala.answers.v1",
  "intent": "Set up rent payment reconciliation from bank transactions",
  "sorla": {
    "source": {
      "kind": "file",
      "uri": "./examples/tenancy/sorla.yaml"
    },
    "expected_schema": "greentic.sorla.v0.2"
  },
  "detected_capability": "reconciliation",
  "extension": "greentic.operala.reconciliation.v1",
  "locale": "en-GB",
  "tenant": "demo-tenant",
  "team": "property-ops",
  "outputs": {
    "handoff_path": "./target/operala/tenancy-rent-reconciliation/operala-handoff.json",
    "gtpack_path": "./target/gtpacks/tenancy-rent-reconciliation.gtpack",
    "work_dir": "./target/operala/tenancy-rent-reconciliation"
  },
  "approval": {
    "allow_sorla_patch_proposal": true,
    "apply_sorla_patch": false
  },
  "capability_answers": {}
}
```

## Source reference model

`answers.json` must support:

```json
{
  "kind": "file",
  "uri": "./sorla.yaml"
}
```

```json
{
  "kind": "oci",
  "uri": "oci://ghcr.io/greenticai/customer/sorla/tenancy:1.0.0"
}
```

```json
{
  "kind": "store",
  "uri": "store://customer-store/demo-tenant/sorla/tenancy"
}
```

```json
{
  "kind": "repo",
  "uri": "repo://customer-repo/sorla/tenancy.yaml"
}
```

## Acceptance criteria

- Rust structs exist.
- JSON schema exists.
- Validation rejects missing `schema`, `intent`, `sorla.source`, `extension`, or `outputs`.
- Tenant is required for pilot runs.
- Team is optional.
- Locale is optional but persisted when provided.
- Capability-specific answers remain under `capability_answers.<capability>`.
- Output paths distinguish primary OperaLa handoff metadata from optional
  `greentic-pack` `.gtpack` output.
