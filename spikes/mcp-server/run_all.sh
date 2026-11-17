#!/usr/bin/env bash
# Builds both servers and runs every caller against each. Linux and macOS shells.
# Results land in results/. See README.md for how the client libraries are installed.
set -u
cd "$(dirname "$0")"
export CARGO_TARGET_DIR="$PWD/target"
(cd on-sdk && cargo build --release) || exit 1
(cd by-hand && cargo build --release) || exit 1
PY=clients/py/venv/bin/python
BOTH="2025-11-25,2026-07-28"
rm -f results/*.json results/*.jsonl

for s in by-hand on-sdk; do
  bin="target/release/spike-mcp-$s"
  python3 callers/raw_conformance.py "$bin" tools.json "results/raw-$s.json" > "results/raw-$s.txt" 2>&1
  echo "raw conformance against $s: $(tail -1 "results/raw-$s.txt")"
  for set in both:$BOTH handshake-only:2025-11-25 modern-only:2026-07-28; do
    label="${set%%:*}"; revisions="${set#*:}"
    run() { # run <name> <caller command...>; the server command is appended
      local name="$1"; shift
      "$@" "results/$name-$label-$s.json" python3 callers/tee.py "results/wire-$name-$label-$s.jsonl" "$bin" tools.json "$revisions" > /dev/null 2>&1
    }
    run py-auto      "$PY" callers/py_caller.py auto
    run py-handshake "$PY" callers/py_caller.py legacy
    run py-modern    "$PY" callers/py_caller.py 2026-07-28
    run ts1          node callers/ts_caller.mjs v1 legacy clients/ts
    run ts2-default  node callers/ts_caller.mjs v2 legacy clients/ts2
    run ts2-auto     node callers/ts_caller.mjs v2 auto clients/ts2
    run ts2-pin      node callers/ts_caller.mjs v2 pin clients/ts2
  done
done
# The SDK variant's stateless Streamable HTTP mode: raw requests, then the Python client.
(cd on-sdk && cargo build --release --features http --target-dir "$CARGO_TARGET_DIR/with-http") || exit 1
http_bin="target/with-http/release/spike-mcp-on-sdk"
callers/http_probe.sh "$http_bin" tools.json > results/http-on-sdk.txt 2>&1
port_file=$(mktemp)
"$http_bin" tools.json "$BOTH" --http > "$port_file" 2> /dev/null &
http_pid=$!
for _ in $(seq 50); do [ -s "$port_file" ] && break; sleep 0.1; done
for mode in auto legacy; do
  name="py-auto"; [ "$mode" = legacy ] && name="py-handshake"
  "$PY" callers/py_caller.py "$mode" "results/$name-both-on-sdk-over-http.json" "http://127.0.0.1:$(head -1 "$port_file")/mcp" > /dev/null 2>&1
done
kill "$http_pid"; rm -f "$port_file"

python3 callers/summarize.py results > results/summary.md
cat results/summary.md
