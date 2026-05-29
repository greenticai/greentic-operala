# Repo Overview Maintenance

When refreshing `.codex/repo_overview.md`:

1. Inspect the current repository layout, `Cargo.toml`, `ci/`, `scripts/`,
   `.github/workflows/`, and active `.codex` planning files.
2. Update the overview so it reflects what exists now, not planned future code.
3. Preserve the OperaLa architecture boundaries:
   - SoRLa owns system-of-record contracts and canonical IR.
   - OperaLa owns operational authoring and handoff metadata.
   - OperaX is a local/pilot runner over SORX HTTP.
   - `gtc` owns production assembly.
4. Mention any validation scripts or workflows that new code must keep passing.
5. Do not mark planned PR notes as implemented until code exists.
