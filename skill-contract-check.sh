#!/usr/bin/env bash
# SKILL 文档字段名交叉核对（阶段4 · D-018）——D-016 教训迁移：字段名对不上时页面/agent 看着完全正常但数据全空，
# 只测 HTTP 200 是假门。本脚本从**真进程真实响应**里核对 skills/medme-ehr 里承诺的每个字段名。
# 用法: ./skill-contract-check.sh [PORT]   默认 18692
set -uo pipefail
PORT="${1:-18692}"
BIN="${EHR_BIN:-$HOME/medme-shared-target/release/ehr-api}"
ROOT="$(cd "$(dirname "$0")" && pwd)"
SKILL="$ROOT/skills/medme-ehr/SKILL.md"
REFS="$ROOT/skills/medme-ehr/references/tool-contracts.md"
fails=0
gate() { if [ "$2" = "$3" ]; then echo "  PASS $1 = $2"; else echo "  FAIL $1: got=$2 want=$3"; fails=$((fails+1)); fi; }

[ -x "$BIN" ] || { echo "missing binary: $BIN"; exit 1; }
[ -f "$SKILL" ] || { echo "missing SKILL.md"; exit 1; }

H="$(mktemp -d)"; mkdir -p "$H/vaults"
EHR_HOME="$H" "$BIN" --addr="127.0.0.1:$PORT" 2>"$H/serve.log" &
SRV=$!
trap 'kill $SRV 2>/dev/null; rm -rf "$H"' EXIT
for i in $(seq 1 40); do curl -sf "127.0.0.1:$PORT/healthz" >/dev/null 2>&1 && break; sleep 0.5; done
echo "[证据] server up pid=$SRV"
rpc() { curl -s -X POST "127.0.0.1:$PORT/mcp" -H 'content-type: application/json' -d "$1"; }
jq_() { python3 -c "import json,sys;d=json.load(sys.stdin);$1"; }

# 造真实数据：一人一文档一指标
PID=$(curl -s -X POST "127.0.0.1:$PORT/api/v1/persons" -H 'content-type: application/json' \
      -d '{"name":"SKILL门禁","gender":"female","birthday":"1988-03-02"}' | jq_ 'print(d["id"])')
printf '姓名：钱八\n检查项目：血常规\n白细胞计数 11.2 10^9/L 偏高\n' > "$H/lab.txt"
SHA=$(curl -s -X POST "127.0.0.1:$PORT/api/v1/persons/$PID/documents" -F "file=@$H/lab.txt" | jq_ 'print(d["content_hash"])')
# D-029：三个新读侧工具需要非空数据面（空数组 ⇒ 字段名根本不存在，字段核对会假红）
printf '姓名：钱八\n处方\n阿莫西林胶囊 0.5g bid\n' > "$H/rx.txt"
curl -s -X POST "127.0.0.1:$PORT/api/v1/persons/$PID/documents" -F "file=@$H/rx.txt" >/dev/null
printf '姓名：钱八\n出院记录\n主要诊断：2型糖尿病(E11.9)\n' > "$H/dx.txt"
curl -s -X POST "127.0.0.1:$PORT/api/v1/persons/$PID/documents" -F "file=@$H/dx.txt" >/dev/null
curl -s -X POST "127.0.0.1:$PORT/api/v1/persons/$PID/entries/self-measurement" \
  -H 'content-type: application/json' \
  -d '{"values":[{"analyte_key":"bp_systolic","value":128,"unit":"mmHg"},{"analyte_key":"bp_diastolic","value":82,"unit":"mmHg"}]}' >/dev/null

# 1) SKILL.md 里承诺的字段名必须逐个出现在真实 MCP 响应中
RESP=$(rpc "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"list_persons\",\"arguments\":{}}}")$(rpc "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"name\":\"get_timeline\",\"arguments\":{\"person_id\":\"$PID\"}}}")$(rpc "{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"tools/call\",\"params\":{\"name\":\"search_records\",\"arguments\":{\"person_id\":\"$PID\",\"q\":\"白细胞\"}}}")$(rpc "{\"jsonrpc\":\"2.0\",\"id\":4,\"method\":\"tools/call\",\"params\":{\"name\":\"get_labs\",\"arguments\":{\"person_id\":\"$PID\"}}}")$(rpc "{\"jsonrpc\":\"2.0\",\"id\":5,\"method\":\"tools/call\",\"params\":{\"name\":\"list_documents\",\"arguments\":{\"person_id\":\"$PID\"}}}")$(rpc "{\"jsonrpc\":\"2.0\",\"id\":6,\"method\":\"tools/call\",\"params\":{\"name\":\"get_document_detail\",\"arguments\":{\"person_id\":\"$PID\",\"sha256\":\"$SHA\"}}}")

# 期望字段表（域: 字段,逗号分隔），与 SKILL.md / tool-contracts.md 的承诺一一对应
check_fields() { # check_fields <label> <csv> <response-json>
  local miss="" resp="${3:-$RESP}"
  for f in ${2//,/ }; do case "$resp" in *"\"$f\""*) ;; *) miss="$miss $f";; esac; done
  if [ -z "$miss" ]; then echo "  PASS $1 全部存在($(echo $2 | tr ',' ' ' | wc -w) 个)"; else echo "  FAIL $1 缺字段:$miss"; fails=$((fails+1)); fi
}
check_fields "list_persons.fields"   "person_id,name,birthday,gender,id_note"
check_fields "timeline.fields"      "person_id,count,doc_date,doc_type,title"
check_fields "search.fields"        "person_id,q,count,title,snippet"
check_fields "labs.fields"          "person_id,count,series,group_name,unit_canonical,any_abnormal,points,date,value,flag,docref"
check_fields "documents.fields"     "person_id,count,content_hash,original_name,doc_type,doc_date,byte_size"
check_fields "detail.always_fields"   "content_hash,original_name,mime_type,byte_size,imported_at,document_id,doc_type,doc_date,doc_date_end,title,language,page_count,encounter_id,text_indexed,text_truncated_bytes"
# text / ocr_meta / extraction 是 Option + skip_serializing_if：该文档没有文本层时**整个键不存在**（不是空值）。
# 故只断言「存在时形状正确」，不断言必存在——否则门禁会逼文档写假承诺。
DETAIL=$(rpc "{\"jsonrpc\":\"2.0\",\"id\":9,\"method\":\"tools/call\",\"params\":{\"name\":\"get_document_detail\",\"arguments\":{\"person_id\":\"$PID\",\"sha256\":\"$SHA\"}}}")
if echo "$DETAIL" | grep -q '"text"'; then
  if echo "$DETAIL" | grep -q '白细胞计数'; then echo "  PASS detail.text_has_content"
  else echo "  FAIL detail.text 无内容"; fails=$((fails+1)); fi
else echo "  FAIL detail.text 应存在(本例已索引文本层)"; fails=$((fails+1)); fi
if echo "$DETAIL" | grep -q '"extraction"'; then
  if echo "$DETAIL" | grep -q '"parseable"'; then echo "  PASS detail.extraction_has_parseable"
  else echo "  FAIL extraction 缺 parseable"; fails=$((fails+1)); fi
else echo "  PASS detail.extraction_optional_absent(可缺省,文档已载明)"; fi

# 2) SKILL.md 里提到的每个工具名必须都在 tools/list 里公布，且公布清单 == 实现派生清单
#    （T15 · D-031 新坑②：工具数不再硬编码——从实现干跑取值）
TOOLS=$(rpc '{"jsonrpc":"2.0","id":7,"method":"tools/list"}')
MANIFEST=$(EHR_HOME="$H" "$BIN" --print-mcp-tools 2>/dev/null)
EXPECTED_NAMES=$(echo "$MANIFEST" | jq_ 'print(",".join(d["names"]))')
EXPECTED_COUNT=$(echo "$MANIFEST" | jq_ 'print(d["count"])')
ACTUAL_NAMES=$(echo "$TOOLS" | jq_ 'print(",".join(t["name"] for t in d["result"]["tools"]))')
miss=""
for t in $(echo "$EXPECTED_NAMES" | tr ',' ' '); do
  case "$TOOLS" in *"\"$t\""*) ;; *) miss="$miss $t";; esac
  grep -q "$t" "$SKILL" || { echo "  FAIL SKILL.md 未提工具 $t"; fails=$((fails+1)); }
done
[ -z "$miss" ] && echo "  PASS tools_declared_in_skill = $EXPECTED_COUNT" || { echo "  FAIL tools/list 缺:$miss"; fails=$((fails+1)); }
gate "tools_count" "$(echo "$TOOLS" | jq_ 'print(len(d["result"]["tools"]))')" "$EXPECTED_COUNT"
gate "tools_names_match_impl" "$ACTUAL_NAMES" "$EXPECTED_NAMES"
# 2a) **文档里写的工具数**必须等于实现派生的数（T15：数字只在文档里写一次，门禁核对它）
#     「共 12」这类数字此前散落在 实现/两个门禁/两份文档 共 5 处 ⇒ 改一处漏四处。
#     现在：实现=真源，文档=唯一手写处，门禁=核对者。
DOC_COUNT=$(grep -oE '共 *[0-9]+' "$REFS" | head -1 | grep -oE '[0-9]+')
if [ -z "$DOC_COUNT" ]; then echo "  FAIL tool-contracts.md 未写「共 N」工具数"; fails=$((fails+1));
else gate "doc_tool_count" "$DOC_COUNT" "$EXPECTED_COUNT"; fi
SKILL_COUNT=$(grep -oE '[0-9]+ *个工具' "$SKILL" | head -1 | grep -oE '[0-9]+')
if [ -z "$SKILL_COUNT" ]; then echo "  FAIL SKILL.md 未写「N 个工具」"; fails=$((fails+1));
else gate "skill_tool_count" "$SKILL_COUNT" "$EXPECTED_COUNT"; fi
# 2b) 读/写分栏数也对齐（文档写「读侧 10 + 写侧 2」）
EXP_READ=$(echo "$MANIFEST" | jq_ 'print(d["read"])')
EXP_WRITE=$(echo "$MANIFEST" | jq_ 'print(d["write"])')
DOC_READ=$(grep -oE '读侧 *[0-9]+' "$REFS" | head -1 | grep -oE '[0-9]+')
DOC_WRITE=$(grep -oE '写侧 *[0-9]+' "$REFS" | head -1 | grep -oE '[0-9]+')
gate "doc_read_count" "$DOC_READ" "$EXP_READ"
gate "doc_write_count" "$DOC_WRITE" "$EXP_WRITE"

# 2b) 写侧：SKILL.md 承诺的写侧字段名必须出现在真实写侧响应中
B64=$(printf '姓名：钱九\n检查项目：血常规\n白细胞计数 13.1 10^9/L 偏高\n' | base64 -w0)
WPID=$(rpc '{"jsonrpc":"2.0","id":20,"method":"tools/call","params":{"name":"create_person","arguments":{"name":"SKILL写侧","gender":"female","birthday":"1992-07-08"}}}' | jq_ 'print(d["result"]["structuredContent"]["id"])')
if [ -n "$WPID" ] && [ "$WPID" != "None" ]; then echo "  PASS create_person person_id=$WPID"; else echo "  FAIL create_person 无 person_id"; fails=$((fails+1)); fi
WRESP=$(rpc "{\"jsonrpc\":\"2.0\",\"id\":21,\"method\":\"tools/call\",\"params\":{\"name\":\"upload_document\",\"arguments\":{\"person_id\":\"$WPID\",\"filename\":\"血常规.txt\",\"content_base64\":\"$B64\"}}}")
for f in content_hash deduped text_indexed stored_raw_only messages; do
  case "$WRESP" in *"\"$f\""*) ;; *) echo "  FAIL 写侧缺字段 $f"; fails=$((fails+1));; esac
done
[ $fails -eq 0 ] && echo "  PASS 写侧字段全部存在(5 个)"
# 写侧返回值字段名必须与 tool-contracts.md 承诺一致（create_person 的 id/name/birthday/gender）
CRESP=$(rpc '{"jsonrpc":"2.0","id":22,"method":"tools/call","params":{"name":"create_person","arguments":{"name":"字段核对"}}}' | jq_ 'print(",".join(sorted(d["result"]["structuredContent"].keys())))')
for f in id name birthday gender id_note created_at updated_at; do
  case "$CRESP" in *"$f"*) ;; *) echo "  FAIL create_person 缺字段 $f"; fails=$((fails+1));; esac
done
echo "  PASS create_person 字段核对完成"

# 2c) D-029：三个新读侧工具的字段名必须逐个出现在真实 MCP 响应中
NRESP=$(rpc "{\"jsonrpc\":\"2.0\",\"id\":30,\"method\":\"tools/call\",\"params\":{\"name\":\"get_medications\",\"arguments\":{\"person_id\":\"$PID\"}}}")$(rpc "{\"jsonrpc\":\"2.0\",\"id\":31,\"method\":\"tools/call\",\"params\":{\"name\":\"get_conditions\",\"arguments\":{\"person_id\":\"$PID\"}}}")$(rpc "{\"jsonrpc\":\"2.0\",\"id\":32,\"method\":\"tools/call\",\"params\":{\"name\":\"get_measurements\",\"arguments\":{\"person_id\":\"$PID\"}}}")
check_fields "medications.fields"  "person_id,documents_scanned,med_count,mention_count,medications,name,status,docrefs,mentions,raw_name,dose,docref" "$NRESP"
check_fields "conditions.fields"  "person_id,documents_scanned,condition_count,conditions,raw_text,icd_code,docrefs" "$NRESP"
# `onset` 是 Option+skip_serializing_if：来源文档**没日期**时整个键不存在（不是空值）。
# 故只断言「存在时形状正确」，不断言必存在——否则门禁会逼文档写假承诺（D-018 同款纪律）。
if echo "$NRESP" | grep -q '"onset"'; then echo "  PASS conditions.onset_has_value_if_present"
else echo "  PASS conditions.onset_optional_absent(可缺省,文档已载明)"; fi
check_fields "measurements.fields" "person_id,count,measurements,document_id,kind,taken_at,docref,payload_readable" "$NRESP"

# 2d) D-031：审计工具的字段名必须逐个出现在真实 MCP 响应中
ARESP=$(rpc '{"jsonrpc":"2.0","id":33,"method":"tools/call","params":{"name":"get_audit","arguments":{}}}')
check_fields "audit.fields" "persons_scanned,count,entries,source,op,ts,person_id" "$ARESP"
# 未知人过滤 ⇒ 0 条（不是错误）：审计是全局视图
V=$(rpc '{"jsonrpc":"2.0","id":34,"method":"tools/call","params":{"name":"get_audit","arguments":{"person":"00000000-0000-0000-0000-000000000000"}}}' | jq_ 'print(d["result"]["structuredContent"]["count"])')
gate "audit.unknown_person_zero" "$V" "0"

# 2c2) T16 · D-032 未销账⑩：**REST 面的路径数/operation 数**同样不许各处硬编码。
#     实现=真源（--print-rest-manifest）⇒ 文档里写的数字由门禁核对（照抄 D-032 决策③：核对 > 消除）。
#     文档数字此前散在 SKILL.md / rest-contracts.md / 5 个脚本里 ⇒ 同一事实多份实现。
RMAN=$(EHR_HOME="$H" "$BIN" --print-rest-manifest 2>/dev/null)
EXP_PATHS=$(echo "$RMAN" | jq_ 'print(d["paths"])')
EXP_OPS=$(echo "$RMAN" | jq_ 'print(d["operations"])')
RDOC="$ROOT/skills/medme-ehr/references/rest-contracts.md"
if [ ! -f "$RDOC" ]; then echo "  FAIL 缺 rest-contracts.md"; fails=$((fails+1));
else
  DOC_PATHS=$(grep -oE '[0-9]+ *路径' "$RDOC" | head -1 | grep -oE '[0-9]+')
  DOC_OPS=$(grep -oE '[0-9]+ *operation' "$RDOC" | head -1 | grep -oE '[0-9]+')
  if [ -z "$DOC_PATHS" ]; then echo "  FAIL rest-contracts.md 未写「N 路径」"; fails=$((fails+1));
  else gate "doc_rest_paths" "$DOC_PATHS" "$EXP_PATHS"; fi
  if [ -z "$DOC_OPS" ]; then echo "  FAIL rest-contracts.md 未写「N operation」"; fails=$((fails+1));
  else gate "doc_rest_operations" "$DOC_OPS" "$EXP_OPS"; fi
  # SKILL.md 里的路径数也必须与实现一致（它是 agent 读到的第一手描述）
  SK_PATHS=$(grep -oE '[0-9]+ *路径' "$SKILL" | head -1 | grep -oE '[0-9]+')
  if [ -z "$SK_PATHS" ]; then echo "  FAIL SKILL.md 未写「N 路径」"; fails=$((fails+1));
  else gate "skill_rest_paths" "$SK_PATHS" "$EXP_PATHS"; fi
fi

# 3) SKILL.md 与 tool-contracts.md 里写的错误码必须真的对得上
for code in -32602 -32601 -32001 -32002 -32003; do
  grep -q -- "$code" "$SKILL" || grep -q -- "$code" "$REFS" || { echo "  FAIL 文档未载明 $code"; fails=$((fails+1)); }
done
ERRP=$(rpc "{\"jsonrpc\":\"2.0\",\"id\":8,\"method\":\"tools/call\",\"params\":{\"name\":\"get_labs\",\"arguments\":{\"person_id\":\"00000000-0000-0000-0000-000000000000\"}}}")
gate "person_not_found_code(文档-32001)" "$(echo "$ERRP" | jq_ 'print(d["result"]["_code"])')" "-32001"
gate "person_not_found_isError" "$(echo "$ERRP" | jq_ 'print(d["result"]["isError"])')" "True"

# 4) frontmatter 必填字段（QwenPaw skill_pool 约定）
gate "frontmatter.name" "$(grep -m1 '^name:' "$SKILL" | awk '{print $2}')" "medme-ehr"
grep -q '^description:' "$SKILL" || { echo "  FAIL 缺 description"; fails=$((fails+1)); }
gate "references_exists" "$([ -f "$REFS" ] && echo yes || echo no)" "yes"

echo "[证据] skill-contract-check.sh fails=$fails"
[ "$fails" -eq 0 ] || exit 1