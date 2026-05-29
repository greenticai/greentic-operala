# GLOBAL RULES FOR GREENTIC OPERALA

For this repository, you must always:

1. Maintain `.codex/repo_overview.md` before starting PR-style implementation
   work and after finishing it.
2. Run `bash ci/local_check.sh` at the end of implementation work and ensure it
   passes, or explain precisely why it cannot pass.
3. Prefer existing Greentic repos/crates before adding local core types,
   interfaces, or cross-cutting behavior.
4. Follow `.codex/architecture_rules.md` for the SoRLa, OperaLa, OperaX, SORX,
   and `gtc` ownership boundaries.

## Repo Overview Maintenance

Use `.codex/repo_overview_task.md` when refreshing the overview. Keep the file
short, current, and grounded in the code and planning artifacts that exist.

## Local Checks

The canonical local gate is:

```bash
bash ci/local_check.sh
```

It currently validates Rust formatting/lint/tests/docs, `.codex` schemas and
fixtures, the customer pilot planning fixture, and coverage policy hooks.
