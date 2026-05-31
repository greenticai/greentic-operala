# Mock SORX

This mock server accepts the local SORX HTTP endpoints used by the installed
`greentic-operax` runner, including health checks, route/action metadata,
business action invoke calls, and the reconciliation-case generated route. It
records state plus the ordered action log for the tenancy demo.

```bash
tmp_dir="$(mktemp -d -t operala-demo.XXXXXX)"
cp extensions/reconciliation/examples/tenancy/sorx-fixtures/initial-state.json "$tmp_dir/sorx-state.json"
echo "[]" > "$tmp_dir/sorx-calls.json"
python3 extensions/reconciliation/examples/tenancy/mock-sorx/server.py \
  --port 18088 \
  --state "$tmp_dir/sorx-state.json" \
  --calls "$tmp_dir/sorx-calls.json"
```
