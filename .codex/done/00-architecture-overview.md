# PR 00 — Architecture alignment for OperaLa, SoRLa, built-in extensions, and OperaX

## Goal

Align the architecture before implementation:

- SoRLa describes system-of-record source contracts, canonical IR, agent
  endpoint contracts, and SORX handoff metadata.
- OperaLa describes operational business logic that consumes SoRLa contracts and
  emits deterministic operational handoff artifacts.
- OperaLa contains an `extensions/` subgroup for example/default extensions.
- Reconciliation is the first built-in extension at `greentic-operala/extensions/reconciliation`.
- OperaX is a pilot/local runner for operational handoff artifacts.
- SORX is accessed via HTTP at runtime.
- `gtc` remains the owner of production extension orchestration and final
  Greentic bundle/pack assembly.
- All prompt output becomes `answers.json`; deterministic operations run through `greentic-operala wizard --answers`.

## Target product model

```text
SoRLa = describes the system of record, canonical IR, and agent endpoints
SORX  = runs the system of record over HTTP

OperaLa = prompts + deterministic wizard + operational handoff generation
  extensions/
    reconciliation/

OperaX = local/pilot runner that loads OperaLa handoff artifacts and calls SORX HTTP
gtc    = production extension orchestration and final assembly
```

## Required repo layout

```text
greentic-operala/
  crates/
    operala-cli/
    operala-core/
    operala-answers/
    operala-extension-api/
    operala-qa/
    operala-i18n/
    operala-distribution/
    operala-handoff/
  extensions/
    reconciliation/
      crates/
        operala-reconciliation/
      templates/
      schemas/
      examples/

greentic-operax/
  crates/
    operax-cli/
    operax-core/
    operax-pack-loader/
    operax-sorx-http/
    operax-dispatch/
    operax-policy/
    operax-audit/
```

The actual first PR may keep a smaller workspace and grow toward this layout.
Do not introduce a separate SoRLa parser or schema crate in OperaLa; depend on
`greentic-sorla-lib` or stable SoRLa artifact contracts.

## Non-goals

- Do not create separate `reconcila` or `reconcilax` CLIs.
- Do not make runtime LLM-dependent.
- Do not directly mutate SoRLa during `greentic-operala prompt` or `greentic-operala wizard`.
- Do not create a separate reconciliation repo unless later needed for marketplace distribution.
- Do not make OperaLa own final `.gtbundle` assembly, SORX runtime policy,
  provider credentials, OAuth, or database execution.

## Acceptance criteria

- Architecture decision recorded in `docs/architecture/operala-operax.md`.
- Reconciliation extension lives under `greentic-operala/extensions/reconciliation`.
- All PRs reference this architecture.
- The decision explicitly names SoRLa as the SoR contract source and `gtc` as
  the production assembly owner.
