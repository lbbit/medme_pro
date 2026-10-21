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

# 2) tools/list 公布的工具清单必须**等于**实现派生的清单（T15 · D-031 新坑②）
#    工具数不再硬编码：这里断言「公布 == 实现」，实现侧由 `ehr-api --print-mcp-tools` 干跑取值。
#    （硬编码数字是快照不是判据：加一个工具要改三处，漏一处就红且看不出原因。）
MANIFEST=$(EHR_HOME="$H" "$BIN" --print-mcp-tools 2>/dev/null)
EXPECTED_NAMES=$(echo "$MANIFEST" | jq_ 'print(",".join(d["names"]))')
EXPECTED_COUNT=$(echo "$MANIFEST" | jq_ 'print(d["count"])')
TOOLS=$(rpc '{"jsonrpc":"2.0","id":2,"method":"tools/list"}')
ACTUAL_NAMES=$(echo "$TOOLS" | jq_ 'print(",".join(t["name"] for t in d["result"]["tools"]))')
ACTUAL_COUNT=$(echo "$TOOLS" | jq_ 'print(len(d["result"]["tools"]))')
gate "tools_count" "$ACTUAL_COUNT" "$EXPECTED_COUNT"
gate "tools_names_match_impl" "$ACTUAL_NAMES" "$EXPECTED_NAMES"
note "tools_count=$ACTUAL_COUNT (derived from --print-mcp-tools, not hardcoded)"

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

# 4b) D-029：三个新读侧工具（与 REST 同源：同一 meds_aggregate/conditions_aggregate/measurements_projection）
#     造一份处方（用药面）+ 一条出院记录（诊断面）+ 一条自测值（measurements 面）
printf '姓名：钱七\n处方\n阿莫西林胶囊 0.5g bid\n' > "$H/rx.txt"
curl -s -X POST "127.0.0.1:$PORT/api/v1/persons/$PID/documents" -F "file=@$H/rx.txt" >/dev/null
printf '姓名：钱七\n出院记录\n主要诊断：2型糖尿病(E11.9)\n' > "$H/dx.txt"
curl -s -X POST "127.0.0.1:$PORT/api/v1/persons/$PID/documents" -F "file=@$H/dx.txt" >/dev/null
curl -s -X POST "127.0.0.1:$PORT/api/v1/persons/$PID/entries/self-measurement" \
  -H 'content-type: application/json' \
  -d '{"values":[{"analyte_key":"bp_systolic","value":128,"unit":"mmHg"},{"analyte_key":"bp_diastolic","value":82,"unit":"mmHg"}]}' >/dev/null
V=$(rpc "{\"jsonrpc\":\"2.0\",\"id\":21,\"method\":\"tools/call\",\"params\":{\"name\":\"get_medications\",\"arguments\":{\"person_id\":\"$PID\"}}}" | jq_ 'd=d["result"]["structuredContent"];print(d["med_count"]>0, d["medications"][0]["docrefs"][0]!="")')
gate "medications.nonempty+docref" "$(echo "$V" | tr ' ' '/')" "True/True"
V=$(rpc "{\"jsonrpc\":\"2.0\",\"id\":22,\"method\":\"tools/call\",\"params\":{\"name\":\"get_conditions\",\"arguments\":{\"person_id\":\"$PID\"}}}" | jq_ 'd=d["result"]["structuredContent"];print(d["condition_count"]>0, d["conditions"][0]["docrefs"][0]!="")')
gate "conditions.nonempty+docref" "$(echo "$V" | tr ' ' '/')" "True/True"
V=$(rpc "{\"jsonrpc\":\"2.0\",\"id\":23,\"method\":\"tools/call\",\"params\":{\"name\":\"get_measurements\",\"arguments\":{\"person_id\":\"$PID\"}}}" | jq_ 'd=d["result"]["structuredContent"];print(d["count"], d["measurements"][0]["kind"], d["measurements"][0]["docref"]!="")')
gate "measurements.count+kind+docref" "$(echo "$V" | tr ' ' '/')" "1/self_measurement/True"
# 过滤器真过滤（不是摆设）
V=$(rpc "{\"jsonrpc\":\"2.0\",\"id\":24,\"method\":\"tools/call\",\"params\":{\"name\":\"get_measurements\",\"arguments\":{\"person_id\":\"$PID\",\"kind\":\"note\"}}}" | jq_ 'print(d["result"]["structuredContent"]["count"])')
gate "measurements.kind_filter" "$V" "0"
# 非法 kind ⇒ -32602（参数非法，agent 该改参数）
V=$(rpc "{\"jsonrpc\":\"2.0\",\"id\":25,\"method\":\"tools/call\",\"params\":{\"name\":\"get_measurements\",\"arguments\":{\"person_id\":\"$PID\",\"kind\":\"nonsense\"}}}" | jq_ 'print(d["result"]["_code"])')
gate "measurements.bad_kind=-32602" "$V" "-32602"
# 未知人 ⇒ -32001
V=$(rpc "{\"jsonrpc\":\"2.0\",\"id\":26,\"method\":\"tools/call\",\"params\":{\"name\":\"get_conditions\",\"arguments\":{\"person_id\":\"nope\"}}}" | jq_ 'print(d["result"]["_code"])')
gate "conditions.unknown_person=-32001" "$V" "-32001"

# 4c) D-031：审计工具（与 REST 同一 `audit_view()`，不得另写一份判据）
AV=$(rpc '{"jsonrpc":"2.0","id":27,"method":"tools/call","params":{"name":"get_audit","arguments":{}}}' | jq_ 'd=d["result"]["structuredContent"];src=sorted({e["source"] for e in d["entries"]});print(d["count"]>=2, src==["registry","vault"])' | tr ' ' '/')
gate "audit.two_layers_present" "$AV" "True/True"
# 同源硬证据：MCP 与 REST 的 count 逐字相等
RC=$(curl -s "127.0.0.1:$PORT/api/v1/audit" | jq_ 'print(d["count"])')
MC=$(rpc '{"jsonrpc":"2.0","id":28,"method":"tools/call","params":{"name":"get_audit","arguments":{}}}' | jq_ 'print(d["result"]["structuredContent"]["count"])')
gate "audit.mcp_equals_rest_count" "$MC" "$RC"
# person 过滤真进判据
V=$(rpc "{\"jsonrpc\":\"2.0\",\"id\":29,\"method\":\"tools/call\",\"params\":{\"name\":\"get_audit\",\"arguments\":{\"person\":\"$PID\"}}}" | jq_ 'd=d["result"]["structuredContent"];print(d["count"]>0, all(e["person_id"]=="'$PID'" for e in d["entries"]))' | tr ' ' '/')
gate "audit.person_filter" "$V" "True/True"
# op 过滤真进判据（大小写不敏感）
V=$(rpc '{"jsonrpc":"2.0","id":30,"method":"tools/call","params":{"name":"get_audit","arguments":{"op":"CREATE"}}}' | jq_ 'd=d["result"]["structuredContent"];print(d["count"], all(e["op"]=="create" for e in d["entries"]))' | tr ' ' '/')
gate "audit.op_filter_case_insensitive" "$V" "1/True"
# 未知人 ⇒ 0 条（**不是**错误：审计是全局视图）
V=$(rpc '{"jsonrpc":"2.0","id":31,"method":"tools/call","params":{"name":"get_audit","arguments":{"person":"00000000-0000-0000-0000-000000000000"}}}' | jq_ 'print(d["result"]["structuredContent"]["count"])')
gate "audit.unknown_person_zero" "$V" "0"

# 5) 错误语义（agent 要能据此换人/改参，而不是重试）
V=$(rpc '{"jsonrpc":"2.0","id":10,"method":"tools/call","params":{"name":"get_timeline","arguments":{"person_id":"nope"}}}' | jq_ 'd=d["result"];print(d["isError"],d["_code"])' | tr ' ' '/')
gate "unknown_person=err/-32001" "$V" "True/-32001"
V=$(rpc '{"jsonrpc":"2.0","id":11,"method":"tools/call","params":{"name":"get_timeline","arguments":{}}}' | jq_ 'print(d["result"]["_code"])')
gate "missing_arg=-32602" "$V" "-32602"
# ⚠️ D-029：此前的探针用 `get_medications`（当时它「恰好」不在 tools/list ⇒ 断言 -32601）；
#    D-029 把它实装成真工具后，同一探针会返回 -32602（缺 person_id）⇒ **探针过期**，不是代码坏了。
V=$(rpc '{"jsonrpc":"2.0","id":12,"method":"tools/call","params":{"name":"get_no_such_tool","arguments":{}}}' | jq_ 'print(d["result"]["_code"])')
gate "unknown_tool=-32601" "$V" "-32601"
V=$(rpc '{"jsonrpc":"2.0","id":13,"method":"no/such"}' | jq_ 'print(d["error"]["code"])')
gate "unknown_method=-32601" "$V" "-32601"

# 6) REST 契约未被污染
V=$(curl -s "127.0.0.1:$PORT/openapi.json" | jq_ 'print(len(d["paths"]))')
if [ "$V" -ge 16 ] 2>/dev/null; then echo "  PASS openapi_paths>=16 = $V"; else echo "  FAIL openapi_paths=$V"; fails=$((fails+1)); fi
V=$(curl -s "127.0.0.1:$PORT/openapi.json" | jq_ 'print("/mcp" in d["paths"])')
gate "mcp_not_in_openapi" "$V" "False"

# 7) 写侧闭环（真进程 · 全程只走 MCP · agent 不必绕 REST）
#    create_person → upload_document(base64) → 读侧立刻可见 → 重传幂等
B64=$(printf '姓名：孙九\n检查项目：血常规\n白细胞计数 12.4 10^9/L 偏高\n' | base64 -w0)
# ⚠️ 期望 sha 必须对**解码后的字节**求值（不是对 base64 串）——先量后改(D-011 纪律)。
WANT_SHA=$(printf '姓名：孙九\n检查项目：血常规\n白细胞计数 12.4 10^9/L 偏高\n' | sha256sum | cut -d' ' -f1)
WPID=$(rpc '{"jsonrpc":"2.0","id":14,"method":"tools/call","params":{"name":"create_person","arguments":{"name":"写侧门禁","gender":"female"}}}' \
       | jq_ 'print(d["result"]["structuredContent"]["id"])')
if [ -n "$WPID" ] && [ "$WPID" != "None" ]; then echo "  PASS create_person person_id=$WPID"; else echo "  FAIL create_person 无 person_id"; fails=$((fails+1)); fi
WSHA=$(rpc "{\"jsonrpc\":\"2.0\",\"id\":15,\"method\":\"tools/call\",\"params\":{\"name\":\"upload_document\",\"arguments\":{\"person_id\":\"$WPID\",\"filename\":\"血常规.txt\",\"content_base64\":\"$B64\"}}}" \
       | jq_ 'd=d["result"]["structuredContent"];print(d["content_hash"],d["deduped"],d["text_indexed"])' | tr ' ' '/')
gate "upload_document(hash/deduped/indexed)" "$WSHA" "$WANT_SHA/False/True"
V=$(rpc "{\"jsonrpc\":\"2.0\",\"id\":16,\"method\":\"tools/call\",\"params\":{\"name\":\"search_records\",\"arguments\":{\"person_id\":\"$WPID\",\"q\":\"白细胞\"}}}" | jq_ 'print(d["result"]["structuredContent"]["count"])')
gate "write_then_read.search_count" "$V" "1"
V=$(rpc "{\"jsonrpc\":\"2.0\",\"id\":17,\"method\":\"tools/call\",\"params\":{\"name\":\"get_labs\",\"arguments\":{\"person_id\":\"$WPID\"}}}" | jq_ 'print(len(d["result"]["structuredContent"]["series"])>0)')
gate "write_then_read.labs_nonempty" "$V" "True"
V=$(rpc "{\"jsonrpc\":\"2.0\",\"id\":18,\"method\":\"tools/call\",\"params\":{\"name\":\"upload_document\",\"arguments\":{\"person_id\":\"$WPID\",\"filename\":\"血常规.txt\",\"content_base64\":\"$B64\"}}}" | jq_ 'print(d["result"]["structuredContent"]["deduped"])')
gate "reupload_idempotent" "$V" "True"
# 坏 base64 ⇒ -32602（参数校验先于任何 IO）
V=$(rpc "{\"jsonrpc\":\"2.0\",\"id\":19,\"method\":\"tools/call\",\"params\":{\"name\":\"upload_document\",\"arguments\":{\"person_id\":\"$WPID\",\"filename\":\"a.txt\",\"content_base64\":\"!!!!\"}}}" | jq_ 'print(d["result"]["_code"])')
gate "bad_base64=-32602" "$V" "-32602"
# 不支持的类型 ⇒ -32003（与「服务坏了」区分）
V=$(rpc "{\"jsonrpc\":\"2.0\",\"id\":20,\"method\":\"tools/call\",\"params\":{\"name\":\"upload_document\",\"arguments\":{\"person_id\":\"$WPID\",\"filename\":\"s.dcm\",\"content_base64\":\"$B64\"}}}" | jq_ 'print(d["result"]["_code"])')
gate "unsupported_type=-32003" "$V" "-32003"

echo "[证据] mcp-e2e.sh fails=$fails"
exit $([ "$fails" -eq 0 ] && echo 0 || echo 1)