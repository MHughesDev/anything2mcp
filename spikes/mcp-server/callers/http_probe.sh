#!/usr/bin/env bash
# Raw HTTP probe of the SDK variant's stateless Streamable HTTP mode (feature `http`).
# Usage: http_probe.sh <binary built with --features http> <tools.json>
# It starts the server on a port the system picks, sends a handful of POSTs with curl, and stops it.
set -u
out=$(mktemp)
"$1" "$2" "2025-11-25,2026-07-28" --http > "$out" &
pid=$!
trap 'kill $pid 2>/dev/null; rm -f "$out"' EXIT
for _ in $(seq 50); do [ -s "$out" ] && break; sleep 0.1; done
url="http://127.0.0.1:$(head -1 "$out")/mcp"
post() { # post <label> <body> [header...]
  local label="$1" body="$2"; shift 2
  local args=()
  for h in "$@"; do args+=(-H "$h"); done
  echo "== $label"
  curl -sS -m 5 -i -X POST "$url" -H 'Content-Type: application/json' -H 'Accept: application/json, text/event-stream' "${args[@]}" -d "$body" \
    | tr -d '\r' | grep -i -E '^HTTP/|^content-type|^mcp-session-id|^\{|^data:' | cut -c1-420
}
META='"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}'
post "handshake era: initialize (expect 200 and no Mcp-Session-Id)" \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"curl","version":"0"}}}'
post "handshake era: tools/call with no session, version header only" \
  '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"example_orders_order_get","arguments":{"order_id":"A-1001"}}}' \
  'MCP-Protocol-Version: 2025-11-25'
post "modern era: server/discover" \
  "{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"server/discover\",\"params\":{$META}}" \
  'MCP-Protocol-Version: 2026-07-28' 'Mcp-Method: server/discover'
post "modern era: tools/call with the standard headers" \
  "{\"jsonrpc\":\"2.0\",\"id\":4,\"method\":\"tools/call\",\"params\":{\"name\":\"example_orders_order_get\",\"arguments\":{\"order_id\":\"A-1001\"},$META}}" \
  'MCP-Protocol-Version: 2026-07-28' 'Mcp-Method: tools/call' 'Mcp-Name: example_orders_order_get'
post "modern era: header and body disagree (expect 400 and -32020)" \
  "{\"jsonrpc\":\"2.0\",\"id\":5,\"method\":\"tools/call\",\"params\":{\"name\":\"example_orders_order_get\",\"arguments\":{\"order_id\":\"A-1001\"},$META}}" \
  'MCP-Protocol-Version: 2026-07-28' 'Mcp-Method: tools/call' 'Mcp-Name: example_other_tool'
post "modern era: unserved version (expect 400 and -32022)" \
  '{"jsonrpc":"2.0","id":6,"method":"tools/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"1900-01-01","io.modelcontextprotocol/clientCapabilities":{}}}}' \
  'MCP-Protocol-Version: 1900-01-01' 'Mcp-Method: tools/list'
post "a foreign Host header (expect a refusal: DNS rebinding guard)" \
  '{"jsonrpc":"2.0","id":7,"method":"ping"}' 'Host: example.invalid'
