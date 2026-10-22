#!/usr/bin/env bash
# medme-ehr · T34「MCP resources 面」门禁（真进程、真实数据、无 mock）。
#
# 为什么需要它（PLAN 未销账②「MCP 侧仍无 resources/prompts」的正面处置）：
#   resources 面是 MCP 规范的**第二个协议面**（tools 是动作，resources 是「某个东西本身」）。
#   本轮 T34 把它落地 ⇒ 它必须有自己的门，否则它就是「有实现、无判据」的新鲜覆盖：
#   实现改了 URI 模板 / 内容形状 / 权限口径，agent 照旧读得到错的东西，而没人发现。
#
# 判据（缺一即算不通过）：
#   R1 **公布 == 实现**（双向、派生）：`resources/list` 与 `resources/templates/list` 的
#      URI 集合必须与**干跑清单** `--print-mcp-resources` **逐字相等**（不手抄 URI 表）。
#      双向：实现里多一个模板而没公布 ⇒ 红；公布了实现里没有的 URI ⇒ 红。
#   R2 **跨面不变量（本门最关键）**：同一个语义，resources/read 的内容必须与
#      tools/call 的 structuredContent **逐字相等** ⇒ 「一个语义只有一处实现」（D2）。
#      这一条若成立，则 resources 面**不可能**与 REST 面分叉（两者都走同一份 builder）。
#   R3 **只读**：ro token 能读 resources（资源是只读上下文）。
#   R4 **错误语义分得开**：未知 URI ⇒ -32005（改 URI）；未知 person ⇒ -32001（换人）。
#      两者混成一个码 ⇒ agent 只会瞎重试（D5）。
#   R5 **握手能力声明**：initialize 必须宣告 resources 能力。
#
# D-050 纪律：**派生的期望值为空时必须红**（不许空==空判 PASS）。
#
# 用法: ./mcp-resources-e2e.sh [PORT]   默认 18693
set -uo pipefail
PORT="${1:-18693}"
ROOT="$(cd "$(dirname "$0")" && pwd)"
. "$ROOT/ci/env.sh"
BIN="$EHR_BIN"
fails=0
note() { echo "[证据] $*"; }
gate() { # gate <name> <actual> <expected>
  if [ "$2" = "$3" ]; then echo "  PASS $1 = $2"; else echo "  FAIL $1: got=$2 want=$3"; fails=$((fails+1)); fi
}

[ -x "$BIN" ] || { echo "missing binary: $BIN"; exit 1; }

H="$(mktemp -d)"; mkdir -p "$H/vaults"
EHR_HOME="$H" "$BIN" --addr="127.0.0.1:$PORT" 2>"$H/serve.log" &
SRV=$!
trap 'kill $SRV 2>/dev/null; rm -rf "$H"' EXIT
for i in $(seq 1 40); do curl -sf "127.0.0.1:$PORT/healthz" >/dev/null 2>&1 && break; sleep 0.5; done
note "server up pid=$SRV home=$H"

rpc() { curl -s -X POST "127.0.0.1:$PORT/mcp" -H 'content-type: application/json' -d "$1"; }
jq_() { python3 -c "import json,sys;d=json.load(sys.stdin);$1"; }

# ---- 造数据：一人 + 一份化验单（labs/timeline/documents 面都要非空，否则字段面恒绿）----
PID=$(curl -s -X POST "127.0.0.1:$PORT/api/v1/persons" -H 'content-type: application/json' \
      -d '{"name":"资源门禁","gender":"female","birthday":"1988-03-02"}' | jq_ 'print(d["id"])')
if [ -z "$PID" ] || [ "$PID" = "None" ]; then echo "  FAIL 建档失败"; exit 1; fi
printf '姓名：资源门禁\n检查项目：血常规\n白细胞计数 11.2 10^9/L 偏高\n' > "$H/lab.txt"
SHA=$(curl -s -X POST "127.0.0.1:$PORT/api/v1/persons/$PID/documents" -F "file=@$H/lab.txt" | jq_ 'print(d["content_hash"])')
printf '姓名：资源门禁\n处方\n阿莫西林胶囊 0.5g bid\n' > "$H/rx.txt"
curl -s -X POST "127.0.0.1:$PORT/api/v1/persons/$PID/documents" -F "file=@$H/rx.txt" >/dev/null
printf '姓名：资源门禁\n出院记录\n主要诊断：2型糖尿病(E11.9)\n' > "$H/dx.txt"
curl -s -X POST "127.0.0.1:$PORT/api/v1/persons/$PID/documents" -F "file=@$H/dx.txt" >/dev/null
curl -s -X POST "127.0.0.1:$PORT/api/v1/persons/$PID/entries/self-measurement" \
  -H 'content-type: application/json' \
  -d '{"values":[{"analyte_key":"bp_systolic","value":128,"unit":"mmHg"},{"analyte_key":"bp_diastolic","value":82,"unit":"mmHg"}]}' >/dev/null
note "person=$PID sha=$SHA"

# ---- 期望值从**实现干跑清单**派生（不手抄 URI 表）----
MAN=$(EHR_HOME="$H" "$BIN" --print-mcp-resources 2>/dev/null)
EXP_URIS=$(echo "$MAN" | jq_ 'print(",".join(d["uris"]))')
EXP_TEMPLATES=$(echo "$MAN" | jq_ 'print(",".join(d["template_uris"]))')
EXP_COUNT=$(echo "$MAN" | jq_ 'print(d["count"])')
EXP_TCOUNT=$(echo "$MAN" | jq_ 'print(d["templates"])')
# ⚠️ D-050：派生的期望值为空 ⇒ 必须红，不许拿空值参与比较。
if [ -z "$EXP_URIS" ] || [ -z "$EXP_TEMPLATES" ] || [ "$EXP_COUNT" = "0" ] || [ "$EXP_TCOUNT" = "0" ]; then
  echo "  FAIL 干跑清单派生的期望值为空(uris='$EXP_URIS' templates='$EXP_TEMPLATES')⇒ 门无判别力"
  fails=$((fails+1))
fi

echo "== R1 公布 == 实现（双向，派生） =="
GOT_URIS=$(rpc '{"jsonrpc":"2.0","id":1,"method":"resources/list"}' | jq_ 'print(",".join(r["uri"] for r in d["result"]["resources"]))')
GOT_COUNT=$(rpc '{"jsonrpc":"2.0","id":2,"method":"resources/list"}' | jq_ 'print(len(d["result"]["resources"]))')
gate "resources_list.uris_match_impl" "$GOT_URIS" "$EXP_URIS"
gate "resources_list.count_match_impl" "$GOT_COUNT" "$EXP_COUNT"
GOT_TPL=$(rpc '{"jsonrpc":"2.0","id":3,"method":"resources/templates/list"}' | jq_ 'print(",".join(r["uriTemplate"] for r in d["result"]["resourceTemplates"]))')
GOT_TCOUNT=$(rpc '{"jsonrpc":"2.0","id":4,"method":"resources/templates/list"}' | jq_ 'print(len(d["result"]["resourceTemplates"]))')
gate "resources_templates.match_impl" "$GOT_TPL" "$EXP_TEMPLATES"
gate "resources_templates.count_match_impl" "$GOT_TCOUNT" "$EXP_TCOUNT"

echo "== R6 每个公布的模板都**真能读**（派生，不手抄 kind 表） =="
# ⚠️ 为什么要有 R6：R1 是「公布 == 干跑清单」，两者同源 ⇒ **同义反复**风险（D-031 家族：
#   两边都从 resource_templates() 派生 ⇒ 永远相等 ⇒ 永远绿）。
#   ⇒ 真正的判别力来自**跨面**：把实现公布出来的每个模板**逐个真读一遍**，
#      模板占位符用真值替换（{person_id}→真人、{sha256}→真原件哈希），
#      读得出（result.contents 非空且非 isError）才算这个模板真的存在。
#      若实现里「公布了模板但读不出来」⇒ 立刻红（这正是「挂名不实现」的资源面版本）。
TPL_JSON=$(rpc '{"jsonrpc":"2.0","id":20,"method":"resources/templates/list"}' | jq_ 'print(json.dumps([r["uriTemplate"] for r in d["result"]["resourceTemplates"]]))')
TPL_N=$(echo "$TPL_JSON" | python3 -c 'import json,sys;print(len(json.load(sys.stdin)))')
TPL_OK=0
for t in $(echo "$TPL_JSON" | python3 -c 'import json,sys;[print(x) for x in json.load(sys.stdin)]'); do
  uri=$(echo "$t" | sed -e "s/{person_id}/$PID/g" -e "s/{sha256}/$SHA/g")
  V=$(rpc "{\"jsonrpc\":\"2.0\",\"id\":21,\"method\":\"resources/read\",\"params\":{\"uri\":\"$uri\"}}" \
      | jq_ 'print("ok" if d.get("error") is None and not d["result"].get("isError") and len(d["result"].get("contents",[]))==1 else "err")')
  if [ "$V" = "ok" ]; then TPL_OK=$((TPL_OK+1)); else echo "  FAIL template 读不出: $t (uri=$uri)"; fails=$((fails+1)); fi
done
note "templates_readable=$TPL_OK/$TPL_N"
if [ "$TPL_OK" != "$TPL_N" ]; then echo "  FAIL 模板数与可读数不等($TPL_OK/$TPL_N)"; fails=$((fails+1)); fi

echo "== R5 握手能力声明 =="
V=$(rpc '{"jsonrpc":"2.0","id":5,"method":"initialize","params":{}}' | jq_ 'print("ok" if d["result"]["capabilities"].get("resources") is not None else "missing")')
gate "initialize.declares_resources" "$V" "ok"

echo "== R2 跨面不变量：resources/read == tools/call（逐字） =="
# 逐个读侧域各比一次：只要有一个分叉就红（D2：一个语义只有一处实现）
cmp_face() { # cmp_face <label> <uri> <tool> <args-json>
  local label="$1" uri="$2" tool="$3" args="$4"
  local R T
  R=$(rpc "{\"jsonrpc\":\"2.0\",\"id\":10,\"method\":\"resources/read\",\"params\":{\"uri\":\"$uri\"}}" \
      | jq_ 'print(json.dumps(json.loads(d["result"]["contents"][0]["text"]),sort_keys=True))')
  T=$(rpc "{\"jsonrpc\":\"2.0\",\"id\":11,\"method\":\"tools/call\",\"params\":{\"name\":\"$tool\",\"arguments\":$args}}" \
      | jq_ 'print(json.dumps(d["result"]["structuredContent"],sort_keys=True))')
  gate "$label" "$R" "$T"
}
cmp_face "face.persons" "ehr://persons" "list_persons" "{}"
cmp_face "face.timeline" "ehr://persons/$PID/timeline" "get_timeline" "{\"person_id\":\"$PID\"}"
cmp_face "face.labs" "ehr://persons/$PID/labs" "get_labs" "{\"person_id\":\"$PID\"}"
cmp_face "face.medications" "ehr://persons/$PID/medications" "get_medications" "{\"person_id\":\"$PID\"}"
cmp_face "face.conditions" "ehr://persons/$PID/conditions" "get_conditions" "{\"person_id\":\"$PID\"}"
cmp_face "face.measurements" "ehr://persons/$PID/measurements" "get_measurements" "{\"person_id\":\"$PID\"}"
cmp_face "face.documents" "ehr://persons/$PID/documents" "list_documents" "{\"person_id\":\"$PID\"}"
cmp_face "face.document_detail" "ehr://persons/$PID/documents/$SHA" "get_document_detail" "{\"person_id\":\"$PID\",\"sha256\":\"$SHA\"}"
cmp_face "face.audit" "ehr://audit" "get_audit" "{}"
cmp_face "face.capabilities" "ehr://capabilities" "get_capabilities" "{}"
# 人员档案（registry 主数据）没有对应工具 ⇒ 单独断言它**非空且含 id**（不做跨面比较）
V=$(rpc "{\"jsonrpc\":\"2.0\",\"id\":12,\"method\":\"resources/read\",\"params\":{\"uri\":\"ehr://persons/$PID\"}}" \
    | jq_ 'd=json.loads(d["result"]["contents"][0]["text"]);print(d["id"]+"/"+d["name"])')
gate "resource.person_profile" "$V" "$PID/资源门禁"

echo "== R7 文档面：tool-contracts.md 的 resources URI 表 == 实现（不手抄） =="
# ⚠️ 为什么要有 R7：文档面若只写不核 ⇒ 就会变成「手抄实现细节」的第二份
#   （D-041 家族）⇒ 实现改了资源面，文档照旧说旧话，agent 照旧取错 URI。
#   判据：文档表格里 `ehr://…` 那一列的 URI 集合，必须与**干跑清单**逐字相等。
RDOC="$ROOT/skills/medme-ehr/references/tool-contracts.md"
if [ ! -f "$RDOC" ]; then echo "  FAIL 缺文档 $RDOC"; fails=$((fails+1)); else
DOC_URIS=$(grep -oE '`ehr://[^`]*`' "$RDOC" | tr -d '`' | grep -v '…' | sort -u | paste -sd, -)
IMPL_URIS=$(echo "$MAN" | jq_ 'print(",".join(sorted(set(d["uris"]+d["template_uris"]))))')
DOC_URIS_SORTED=$(echo "$DOC_URIS" | tr ',' '\n' | sort -u | paste -sd, -)
gate "doc.resources_uris_match_impl" "$DOC_URIS_SORTED" "$IMPL_URIS"
[ -n "$DOC_URIS_SORTED" ] || { echo "  FAIL 文档里派生出的 URI 为空 ⇒ 门无判别力"; fails=$((fails+1)); }
fi

echo "== R4 错误语义分得开 =="
V=$(rpc '{"jsonrpc":"2.0","id":13,"method":"resources/read","params":{"uri":"ehr://nope"}}' | jq_ 'print(d["result"]["_code"])')
gate "unknown_uri=-32005" "$V" "-32005"
V=$(rpc '{"jsonrpc":"2.0","id":14,"method":"resources/read","params":{"uri":"ehr://persons/00000000-0000-0000-0000-000000000000/labs"}}' | jq_ 'print(d["result"]["_code"])')
gate "unknown_person=-32001" "$V" "-32001"
V=$(rpc '{"jsonrpc":"2.0","id":15,"method":"resources/read","params":{}}' | jq_ 'print(d["result"]["_code"])')
gate "missing_uri=-32602" "$V" "-32602"

echo "== R3 只读：ro token 也能读资源 =="
PORT2=$((PORT+1)); HOME2="$(mktemp -d)"; mkdir -p "$HOME2/vaults"
EHR_HOME="$HOME2" EHR_API_TOKENS="rwsecret:rw,rosecret:ro" "$BIN" --addr="127.0.0.1:$PORT2" 2>"$HOME2/serve.log" &
SRV2=$!
for i in $(seq 1 40); do curl -sf "127.0.0.1:$PORT2/healthz" >/dev/null 2>&1 && break; sleep 0.5; done
V=$(curl -s -X POST "127.0.0.1:$PORT2/mcp" -H 'content-type: application/json' -H 'Authorization: Bearer rosecret' \
     -d '{"jsonrpc":"2.0","id":16,"method":"resources/list"}' | jq_ 'print(len(d["result"]["resources"]))')
gate "ro_token.resources_list" "$V" "$EXP_COUNT"
# ⚠️ resources/read 的**成功**响应是 JSON-RPC result（无 isError 字段——那是 tools/call 的形状）；
#    失败时走 tool_err ⇒ result.isError=true + result._code。故判据按「有 error 或 isError ⇒ 失败」。
V=$(curl -s -X POST "127.0.0.1:$PORT2/mcp" -H 'content-type: application/json' -H 'Authorization: Bearer rosecret' \
     -d '{"jsonrpc":"2.0","id":17,"method":"resources/read","params":{"uri":"ehr://persons"}}' \
     | jq_ 'print("ok" if d.get("error") is None and not d["result"].get("isError") and len(d["result"]["contents"])==1 else "err")')
gate "ro_token.resources_read" "$V" "ok"
kill $SRV2 2>/dev/null; rm -rf "$HOME2"

echo "[证据] mcp-resources-e2e.sh fails=$fails"
exit $([ "$fails" -eq 0 ] && echo 0 || echo 1)
