# Tenancy Reconciliation Demo

```bash
greentic-operala prompt --locale en-GB --tenant demo-tenant --team property-ops \
  --sorla examples/tenancy/sorla.yaml \
  --output target/operala-demo/answers.json \
  "$(cat examples/tenancy/prompt.txt)"

greentic-operala wizard --answers target/operala-demo/answers.json --locale en-GB

greentic-operax run \
  target/gtpacks/tenancy-rent-reconciliation.gtpack \
  --tenant demo-tenant \
  --team property-ops \
  --sorx-url http://localhost:8088 \
  --input examples/tenancy/banking/one-transaction.json \
  --dry-run \
  --json

tmp_dir="$(mktemp -d -t operala-demo.XXXXXX)"
cp examples/tenancy/sorx-fixtures/initial-state.json "$tmp_dir/sorx-state.json"
echo "[]" > "$tmp_dir/sorx-calls.json"
python3 examples/tenancy/mock-sorx/server.py \
  --port 18088 \
  --state "$tmp_dir/sorx-state.json" \
  --calls "$tmp_dir/sorx-calls.json" &
mock_pid="$!"

greentic-operax run \
  target/gtpacks/tenancy-rent-reconciliation.gtpack \
  --tenant demo-tenant \
  --team property-ops \
  --sorx-url http://127.0.0.1:18088 \
  --input examples/tenancy/banking/daily-transactions.json \
  --json

kill "$mock_pid"
```
