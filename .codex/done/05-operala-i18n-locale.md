# PR 05 — Locale and multi-language support with greentic-i18n-lib

## Goal

Make all OperaLa CLI and wizard output locale-aware.

## Required support

- `--locale`
- locale field in `answers.json`
- localized wizard labels/help
- localized validation messages
- localized build summaries

## Example

```bash
greentic-operala wizard --schema --locale nl-NL
greentic-operala wizard --answers answers.json --locale ar-SA
```

## Implementation

Add:

```text
crates/operala-i18n
i18n/
  en-GB.yaml
  nl-NL.yaml
  fr-FR.yaml
  de-DE.yaml
  ar-SA.yaml
```

Prefer the same locale fallback shape used by SoRLa: English authoring strings
as canonical source, stable i18n keys in schemas/reports, and generated locale
catalogs as sidecars. Do not generate translated operational source contracts.

## Required keys

```yaml
operala.cli.prompt.about:
operala.cli.wizard.schema.about:
operala.validation.missing_field:
operala.extension.reconciliation.source_event.label:
operala.extension.reconciliation.partial_payment_policy.label:
```

## Acceptance criteria

- No hardcoded user-facing strings in core paths.
- Wizard schema labels are localized.
- RTL locale metadata is exposed for UI consumers.
- Unsupported locale falls back to `en-GB`.
- Machine fields, schema IDs, extension IDs, record/action IDs, and endpoint
  refs stay stable and untranslated.
