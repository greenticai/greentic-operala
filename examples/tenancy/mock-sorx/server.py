#!/usr/bin/env python3
import argparse
import json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path


class MockSorxHandler(BaseHTTPRequestHandler):
    state_path = None
    log_path = None

    def do_GET(self):
        if self.path == "/healthz":
            self._send_json({"ok": True})
        elif self.path == "/readyz":
            self._send_json({"ready": True})
        elif self.path == "/v1/sorx/routes":
            self._send_json({
                "routes": [
                    {
                        "endpoint_id": "reconciliation_case.create",
                        "method": "POST",
                        "path": "/v1/agent/reconciliation-cases/create"
                    }
                ]
            })
        elif self.path == "/v1/sorx/business-actions":
            self._send_json({
                "actions": [
                    {
                        "id": "record_rent_payment",
                        "version": "0.1.0",
                        "contract_hash": "sha256:0000000000000000000000000000000000000000000000000000000000000000"
                    }
                ]
            })
        else:
            self.send_error(404, "unknown mock SORX endpoint")

    def do_POST(self):
        length = int(self.headers.get("content-length", "0"))
        payload = json.loads(self.rfile.read(length) or b"{}")
        action, values = self._action_and_values(payload)
        if action is None:
            self.send_error(404, "expected a SORX business-action or generated-route endpoint")
            return
        state = json.loads(self.state_path.read_text())
        calls = json.loads(self.log_path.read_text()) if self.log_path.exists() else []
        calls.append({"action": action, "payload": values})

        if action in ("create_payment", "record_rent_payment"):
            state.setdefault("Payment", []).append(values)
            matched_record_id = values.get("matched_record_id") or values.get("obligation_id")
            if matched_record_id:
                for obligation in state.get("RentObligation", []):
                    if obligation.get("obligation_id") == matched_record_id:
                        obligation["status"] = "paid"
        elif action == "mark_obligation_paid":
            for obligation in state.get("RentObligation", []):
                if obligation.get("obligation_id") == values.get("obligation_id"):
                    obligation["status"] = "paid"
        elif action == "mark_obligation_partially_paid":
            for obligation in state.get("RentObligation", []):
                if obligation.get("obligation_id") == values.get("obligation_id"):
                    obligation["status"] = "partially_paid"
        elif action in ("create_reconciliation_case", "reconciliation_case.create"):
            state.setdefault("ReconciliationCase", []).append(values)
        else:
            self.send_error(404, f"unknown action {action}")
            return

        self.state_path.write_text(json.dumps(state, indent=2, sort_keys=True))
        self.log_path.write_text(json.dumps(calls, indent=2, sort_keys=True))
        self._send_json({"status": "ok", "action": action})

    def _action_and_values(self, payload):
        if self.path.startswith("/actions/"):
            return self.path.rsplit("/", 1)[-1], payload
        prefix = "/v1/sorx/business-actions/"
        suffix = "/invoke"
        if self.path.startswith(prefix) and self.path.endswith(suffix):
            rest = self.path[len(prefix):-len(suffix)]
            action = rest.split("/versions/", 1)[0]
            return action, payload.get("values", {})
        if self.path == "/v1/agent/reconciliation-cases/create":
            return "reconciliation_case.create", payload
        return None, None

    def _send_json(self, value):
        body = json.dumps(value).encode()
        self.send_response(200)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, fmt, *args):
        return


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--port", type=int, default=8088)
    parser.add_argument("--state", required=True)
    parser.add_argument("--calls", required=True)
    args = parser.parse_args()
    MockSorxHandler.state_path = Path(args.state)
    MockSorxHandler.log_path = Path(args.calls)
    server = ThreadingHTTPServer(("127.0.0.1", args.port), MockSorxHandler)
    print(f"mock SORX listening on 127.0.0.1:{args.port}", flush=True)
    server.serve_forever()


if __name__ == "__main__":
    main()
