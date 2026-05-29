# PR 01 — OperaLa minimal CLI surface

## Goal

Implement the small pilot-ready CLI:

```bash
greentic-operala prompt --sorla sorla.yaml "prompt goes here"
greentic-operala wizard --schema
greentic-operala wizard --answers answers.json
```

The CLI mirrors SoRLa's wizard-first shape but targets operational business
logic. Parsing, validation, and handoff generation must stay behind library
facades; the binary should only own argument parsing, localized rendering, and
exit codes.

## Required CLI flags

### `greentic-operala prompt`

```bash
greentic-operala prompt --sorla ./sorla.yaml --locale en-GB "..."
```

Optional:

```bash
--output ./answers.json
--tenant demo-tenant
--team property-ops
```

Rules:

- `--sorla` is required.
- prompt text is positional.
- `--locale` is optional and defaults to environment/user settings.
- output is only `answers.json`.
- no `--capability`.
- capability is inferred.
- the referenced SoRLa source is parsed through `greentic-sorla-lib` or a
  stable exported SoRLa artifact, not an OperaLa-local parser.
- prompt must not build handoff artifacts, call SORX, or apply SoRLa patches.

### `greentic-operala wizard`

```bash
greentic-operala wizard --schema --locale en-GB
greentic-operala wizard --answers ./answers.json --locale en-GB
```

Also support distributed answers references:

```bash
greentic-operala wizard --answers oci://ghcr.io/greenticai/customer/answers/tenant-recon:1.0.0
greentic-operala wizard --answers store://customer-store/demo-tenant/operala/answers/rent-recon
greentic-operala wizard --answers repo://customer-repo/operala/answers/rent-recon.json
```

## Implementation notes

- Use `clap`.
- Keep CLI parsing thin.
- Hand off all logic to `operala-core`.
- All human-facing text must go through `greentic-i18n-lib`.

## Acceptance criteria

- CLI compiles.
- Commands exist.
- Help output is localized.
- `greentic-operala prompt` writes a valid draft `answers.json`.
- `greentic-operala wizard --schema` emits a Greentic QA schema.
- `greentic-operala wizard --answers` accepts both local files and distributor refs.
- `greentic-operala wizard --answers` emits deterministic operational handoff metadata
  and, when requested by answers, a `greentic-pack` `.gtpack` artifact.
