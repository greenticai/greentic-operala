# OperaLa Architecture Rules

For this repository, future work should follow these rules:

- Treat SoRLa as the system-of-record source contract. Use
  `greentic-sorla-lib` or stable SoRLa handoff artifacts instead of duplicating
  SoRLa parsing, AST, patch, or pack logic locally.
- Treat OperaLa as the authoring layer for operational business logic and
  deterministic operational handoff metadata.
- Treat `gtc` as the production owner of extension orchestration. Local
  `.gtpack` output must be built with `greentic-pack` and scoped to OperaX
  pilot/demo execution unless a later architecture decision explicitly moves
  ownership.
- Treat OperaX as a local/pilot runner that calls SORX over HTTP. It must not
  run SORX in process, resolve provider credentials, or mutate `sorla.yaml`.
- Do not introduce separate `reconcila` or `reconcilax` products for the built
  in reconciliation capability.
- Keep generated artifacts free of plaintext secrets, provider credentials, and
  concrete customer SORX URLs.
