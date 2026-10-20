#!/usr/bin/env bash
# MCP server e2e 门禁（阶段4 · D-017）—— 真进程、真实数据、无 mock。
# 用法: ./mcp-e2e.sh [PORT]   默认 18691
# 门禁口径: 任何一步 fails>0 即 exit=1。D4：参数生效要有证据，fails 计数必须打印出来。
set -uo pipefail
PORT="${1:-18691}"
BIN="${EHR_BIN:-$HOME/medme-shared-target/release/ehr-api}"
fails=0
note() { echo "[证据] $*"; }
gate() { # gate <name> <actual> <expected>
  if [ "$2" = "$3" ]; then echo "  PASS $1 = $2"; else echo "  FAIL $1: got=$2 want=$3"; fails=$((fails+1)); fi
}

[ -x "$BIN" ] || { echo "missing binary: $BIN (cargo build --release first)"; exit 1; }

H="$(mktemp -d)"; mkdir -p "$H/vaults"
EHR_HOME="$H" "$BIN" --addr="127.0.0.1:$PORT" 2>"$H/serve.log" &
SRV=$!
trap 'kill $SRV 2>/dev/null; rm -rf "$H"' EXIT

for i in $(seq 1 40); do
  curl -sf "127.0.0.1:$PORT/healthz" >/dev/null 2>&1 && break
  sleep 0.5
done
note "server up pid=$SRV home=$H"

rpc() { curl -s -X POST "127.0.0.1:$PORT/mcp" -H 'content-type: application/json' -d "$1"; }
jq_() { python3 -c "import json,sys;d=json.load(sys.stdin);$1"; }

# 1) initialize
V=$(rpc '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' | jq_ 'print(d["result"]["protocolVersion"])')
gate "protocolVersion" "$V" "2025-06-18"

# 2) tools/list = 6
N=$(rpc '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' | jq_ 'print(len(d["result"]["tools"]))')
gate "tools_count" "$N" "6"

# 3) 建档 + 上传（REST 侧写入 ⇒ MCP 侧必须读到同一真相层）
PID=$(curl -s -X POST "127.0.0.1:$PORT/api/v1/persons" -H 'content-type: application/json' \
      -d '{"name":"MCP门禁"}' | jq_ 'print(d["id"])')
printf '姓名：钱七\n检查项目：血常规\n白细胞计数 11.2 10^9/L 偏高\n' > "$H/lab.txt"
SHA=$(curl -s -X POST "127.0.0.1:$PORT/api/v1/persons/$PID/documents" -F "file=@$H/lab.txt" | jq_ 'print(d["content_hash"])')
note "person=$PID sha=$SHA"

# 4) 逐工具断言
V=$(rpc '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"list_persons","arguments":{}}}' | jq_ 'print(d["result"]["structuredContent"]["count"])')
gate "list_persons.count" "$V" "1"
V=$(rpc "{\"jsonrpc\":\"2.0\",\"id\":4,\"method\":\"tools/call\",\"params\":{\"name\":\"get_timeline\",\"arguments\":{\"person_id\":\"$PID\"}}}" | jq_ 'print(d["result"]["structuredContent"]["count"])')
gate "timeline.count" "$V" "1"
V=$(rpc "{\"jsonrpc\":\"2.0\",\"id\":5,\"method\":\"tools/call\",\"params\":{\"name\":\"search_records\",\"arguments\":{\"person_id\":\"$PID\",\"q\":\"白细胞\"}}}" | jq_ 'd=d["result"]["structuredContent"];print(d["count"],"[" in d["items"][0]["snippet"])' | tr ' ' '/')
gate "search.count/highlight" "$V" "1/True"
V=$(rpc "{\"jsonrpc\":\"2.0\",\"id\":6,\"method\":\"tools/call\",\"params\":{\"name\":\"search_records\",\"arguments\":{\"person_id\":\"$PID\",\"q\":\"zzqqww\"}}}" | jq_ 'print(d["result"]["structuredContent"]["count"])')
gate "search.zero_false_positive" "$V" "0"
V=$(rpc "{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"tools/call\",\"params\":{\"name\":\"get_labs\",\"arguments\":{\"person_id\":\"$PID\"}}}" | jq_ 'd=d["result"]["structuredContent"];print(len(d["series"])>0)')
gate "labs.nonempty" "$V" "True"
V=$(rpc "{\"jsonrpc\":\"2.0\",\"id\":8,\"method\":\"tools/call\",\"params\":{\"name\":\"list_documents\",\"arguments\":{\"person_id\":\"$PID\"}}}" | jq_ 'print(d["result"]["structuredContent"]["items"][0]["content_hash"])')
gate "documents.sha_matches_rest" "$V" "$SHA"
V=$(rpc "{\"jsonrpc\":\"2.0\",\"id\":9,\"method\":\"tools/call\",\"params\":{\"name\":\"get_document_detail\",\"arguments\":{\"person_id\":\"$PID\",\"sha256\":\"$SHA\"}}}" | jq_ 'print("白细胞计数" in (d["result"]["structuredContent"].get("text") or ""))')
gate "detail.text_has_content" "$V" "True"

# 5) 错误语义（agent 要能据此换人/改参，而不是重试）
V=$(rpc '{"jsonrpc":"2.0","id":10,"method":"tools/call","params":{"name":"get_timeline","arguments":{"person_id":"nope"}}}' | jq_ 'd=d["result"];print(d["isError"],d["_code"])' | tr ' ' '/')
gate "unknown_person=err/-32001" "$V" "True/-32001"
V=$(rpc '{"jsonrpc":"2.0","id":11,"method":"tools/call","params":{"name":"get_timeline","arguments":{}}}' | jq_ 'print(d["result"]["_code"])')
gate "missing_arg=-32602" "$V" "-32602"
V=$(rpc '{"jsonrpc":"2.0","id":12,"method":"tools/call","params":{"name":"get_medications","arguments":{}}}' | jq_ 'print(d["result"]["_code"])')
gate "unknown_tool=-32601" "$V" "-32601"
V=$(rpc '{"jsonrpc":"2.0","id":13,"method":"no/such"}' | jq_ 'print(d["error"]["code"])')
gate "unknown_method=-32601" "$V" "-32601"

# 6) REST 契约未被污染
V=$(curl -s "127.0.0.1:$PORT/openapi.json" | jq_ 'print(len(d["paths"]))')
if [ "$V" -ge 16 ] 2>/dev/null; then echo "  PASS openapi_paths>=16 = $V"; else echo "  FAIL openapi_paths=$V"; fails=$((fails+1)); fi
V=$(curl -s "127.0.0.1:$PORT/openapi.json" | jq_ 'print("/mcp" in d["paths"])')
gate "mcp_not_in_openapi" "$V" "False"

echo "[证据] mcp-e2e.sh fails=$fails"
exit $([ "$fails" -eq 0 ] && echo 0 || echo 1)