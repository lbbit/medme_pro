#!/usr/bin/env bash
# medme-ehr · T40「写侧多轮」门禁（真进程 + 真 HTTP）。
#
# 为什么需要它（T40 的缺口定位）：
#   PLAN 从 T35 起挂着同一条未销账项：写侧「多轮/并发/大载荷」三维。
#   T38 关掉「并发」、T39 关掉「大载荷」⇒ 剩下「**多轮**」⇒ 本门关它。
#   此前写侧在真进程里**只被调用过 1 次建档 + 1 次上传** ⇒ 「连续多轮写入后派生读面
#   还准不准」「重复上传幂等吗」「重建后还原吗」「MCP 面与 REST 面同源吗」全都**没有门**。
#
# 判据（缺一即红）：
#   M1 **多轮累积**：同一人连传 N 份**内容不同**的原件 ⇒ timeline/documents 条数 == N、
#      labs 点数 == N（派生读面没把前面的吞掉）
#   M2 **重复上传幂等**：同字节传 N 次 ⇒ 箱里恰好 1 条，且**每次回执的 document_id /
#      text_indexed 都与真相层一致**（T40 真缺陷所在：旧实现重传回执 document_id=null）
#   M3 **重建后还原**：多轮写入后重建派生索引 ⇒ 读面计数**逐字相等**
#   M4 **MCP 面同源**：MCP 连写 N 次 ⇒ REST 读面立刻看见同样 N 条
#   M5 **MCP 面并发建档**：registry 仍逐行完整、读面仍可用（T38 只钉了 REST 面）
#   M6 **真相层未被打坏**：registry 每行都是完整 JSON（直接验落盘字节，不看 API 的说法）
#
# 纪律（D-050 家族）：**派生的期望值不得为空** —— N>0、条数>0 才允许参与比较。
# 纪律（D-038 家族）：**只能用裸 `wait`** ⇒ 裸 wait 会等**所有**后台作业 ⇒ 包括那个
#   一直活着的 ehr-api 服务 ⇒ 门**永远挂住**（T38 实撞）。⇒ 只收集 curl 的 PID 逐个 wait。
#   并发响应**一请求一文件** ⇒ 并发 `>>` 追加到同一文件会让响应互相交错（T38 实撞）。
#
# 用法: ./write-multiround-e2e.sh [PORT]   默认 18698
set -uo pipefail
PORT="${1:-18698}"
ROOT="$(cd "$(dirname "$0")" && pwd)"
. "$ROOT/ci/env.sh"
BIN="$EHR_BIN"
fails=0
note() { echo "[证据] $*"; }
ok()   { echo "  PASS $1"; }
bad()  { echo "  FAIL $1"; fails=$((fails+1)); }

[ -x "$BIN" ] || { echo "missing binary: $BIN"; exit 1; }

H="$(mktemp -d)"; mkdir -p "$H/vaults"
SRV=""
cleanup() {
  [ -n "$SRV" ] && kill "$SRV" 2>/dev/null
  [ -n "$SRV" ] && wait "$SRV" 2>/dev/null
  rm -rf "$H"
}
trap cleanup EXIT

EHR_HOME="$H" "$BIN" --addr="127.0.0.1:${PORT}" >"$H/serve.log" 2>&1 &
SRV=$!
BASE="http://127.0.0.1:${PORT}"
for i in $(seq 1 60); do curl -sf "$BASE/api/v1/healthz" >/dev/null 2>&1 && break; sleep 0.2; done
curl -sf "$BASE/api/v1/healthz" >/dev/null 2>&1 || { echo "FAIL 服务未起来"; cat "$H/serve.log"; echo "fails=$fails"; exit 1; }
note "服务已起 base=$BASE 产物=$BIN"

N="${EHR_MULTIROUND_N:-15}"
note "多轮份数 N=$N"

rpc() { curl -s -X POST "$BASE/mcp" -H 'content-type: application/json' -d "$1"; }
# ⚠️ `rpc` 不能吞掉额外参数（如 `-o file`）⇒ 另开一个可带 `-o` 的变体（T40 实撞：
#   并发响应必须**一请求一文件**，否则响应互相交错 ⇒ 判据变假弱，T38 同族坑）。
rpc_to() { local out="$1"; shift; curl -s -o "$out" -X POST "$BASE/mcp" -H 'content-type: application/json' -d "$1"; }

# ---- 建一个人 ----
PID=$(curl -s -X POST "$BASE/api/v1/persons" -H 'content-type: application/json' \
      -d '{"name":"T40多轮门禁"}' | python3 -c 'import json,sys;print(json.load(sys.stdin)["id"])')
[ -n "$PID" ] && [ "$PID" != "None" ] || { echo "FAIL 建档失败"; echo "fails=$fails"; exit 1; }
note "person=$PID"

# ---- M1：连传 N 份内容不同的原件 ----
for i in $(seq 1 "$N"); do
  printf '姓名：多轮门禁\n检查项目：血常规\n白细胞计数 %d.0 10^9/L\n' "$i" > "$H/f$i.txt"
  code=$(curl -s -o "$H/up$i.json" -w '%{http_code}' -X POST "$BASE/api/v1/persons/$PID/documents" -F "file=@$H/f$i.txt")
  [ "$code" = "200" ] || bad "M1 第 $i 份上传 http=$code"
done
TL=$(curl -s "$BASE/api/v1/persons/$PID/timeline" | python3 -c 'import json,sys;print(json.load(sys.stdin)["count"])')
DC=$(curl -s "$BASE/api/v1/persons/$PID/documents" | python3 -c 'import json,sys;print(json.load(sys.stdin)["count"])')
LB=$(curl -s "$BASE/api/v1/persons/$PID/labs" | python3 -c 'import json,sys;d=json.load(sys.stdin);print(d["total_points"])')
if [ "$TL" = "$N" ] && [ "$DC" = "$N" ] && [ "$LB" = "$N" ]; then
  ok "M1 多轮累积 timeline=$TL documents=$DC labs_points=$LB (期望 $N/$N/$N)"
else
  bad "M1 多轮累积 timeline=$TL documents=$DC labs_points=$LB（期望 $N/$N/$N）"
fi

# ---- M2：同字节传 3 次 ⇒ 恰好 1 条，且每次回执与真相层一致 ----
# ⚠️ 这份 `same.txt` 的内容**必须与 M1 里某一份完全相同**，否则就不是「重复上传」
#   而是「又一份新原件」（CAS 按内容去重 ⇒ 那是另一条文档）。
#   ⇒ 直接复用 M1 的第 1 份 `f1.txt`（同字节 ⇒ 真·重传）。
SAME_SRC="$H/f1.txt"
[ -f "$SAME_SRC" ] || { bad "M2 缺少重传样本 f1.txt"; SAME_SRC=""; }
if [ -n "$SAME_SRC" ]; then
FIRST=""
for i in 1 2 3; do
  curl -s -o "$H/same$i.json" -X POST "$BASE/api/v1/persons/$PID/documents" -F "file=@$SAME_SRC"
done
python3 - "$H" "$BASE" "$PID" "$N" <<'PY' && ok "M2 重传同字节幂等且回执与真相层一致" || bad "M2 幂等/回执一致性"
import json,sys,os,urllib.request
h,base,pid,n=sys.argv[1],sys.argv[2],sys.argv[3],int(sys.argv[4])
# 基准 = M1 里第 1 份的首次上传回执（那份是「首传」⇒ deduped=false）
first=json.load(open(os.path.join(h,"up1.json")))
assert first.get("deduped") is False, "M1 的首传不应判重: %s"%first
assert first.get("document_id") is not None, "M1 的首传应有 document_id: %s"%first
# 三次重传：全部必须判重、且回执与首传**逐字一致**（document_id / text_indexed）
for i in (1,2,3):
    d=json.load(open(os.path.join(h,"same%d.json"%i)))
    if d.get("deduped") is not True:
        print("FAIL 第 %d 次重传未判重: %s"%(i,d)); sys.exit(1)
    if d.get("document_id")!=first.get("document_id"):
        print("FAIL 第 %d 次重传回执 document_id=%s ≠ 首传 %s（回执与真相层不一致）"%(i,d.get("document_id"),first.get("document_id"))); sys.exit(1)
    if d.get("text_indexed") is not True:
        print("FAIL 第 %d 次重传回执 text_indexed=false（真相层里已索引）: %s"%(i,d)); sys.exit(1)
with urllib.request.urlopen(base+"/api/v1/persons/%s/documents"%pid,timeout=30) as r:
    docs=json.load(r)
# M1 已经写了 N 份**不同**内容 ⇒ 加上这些重复的，箱里应当仍是 N 条（重复不新增）
if docs["count"]!=n:
    print("FAIL 同内容重传 3 次后箱里应仍是 %d 条，实为 %d"%(n,docs["count"])); sys.exit(1)
item=[d for d in docs["items"] if d["content_hash"]==first.get("content_hash")]
if not item:
    print("FAIL 重传的那份不在 documents 列表里: %s"%docs); sys.exit(1)
if item[0]["document_id"]!=first.get("document_id"):
    print("FAIL 读面 document_id=%s ≠ 回执 %s"%(item[0]["document_id"],first.get("document_id"))); sys.exit(1)
if item[0].get("text_indexed") is not True:
    print("FAIL 读面 text_indexed=false: %s"%item[0]); sys.exit(1)
print("documents=%d document_id=%s text_indexed=True"%(docs["count"],first.get("document_id")))
PY
fi

# ---- M3：重建派生索引 ⇒ 读面计数逐字相等 ----
BEFORE_TL=$(curl -s "$BASE/api/v1/persons/$PID/timeline" | python3 -c 'import json,sys;print(json.load(sys.stdin)["count"])')
BEFORE_LB=$(curl -s "$BASE/api/v1/persons/$PID/labs" | python3 -c 'import json,sys;print(json.load(sys.stdin)["total_points"])')
EHR_HOME="$H" "$BIN" --rebuild-index >"$H/rebuild.log" 2>&1
RC=$?
AFTER_TL=$(curl -s "$BASE/api/v1/persons/$PID/timeline" | python3 -c 'import json,sys;print(json.load(sys.stdin)["count"])')
AFTER_LB=$(curl -s "$BASE/api/v1/persons/$PID/labs" | python3 -c 'import json,sys;print(json.load(sys.stdin)["total_points"])')
if [ "$RC" = "0" ] && [ "$AFTER_TL" = "$BEFORE_TL" ] && [ "$AFTER_LB" = "$BEFORE_LB" ]; then
  ok "M3 重建后还原 timeline=$AFTER_TL labs_points=$AFTER_LB（与重建前逐字相等）"
else
  bad "M3 重建后还原 timeline=$AFTER_TL(前 $BEFORE_TL) labs_points=$AFTER_LB(前 $BEFORE_LB) rc=$RC"
fi

# ---- M4：MCP 面连写 N 次 ⇒ REST 读面立刻看见 ----
MPID=$(rpc '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"create_person","arguments":{"name":"T40MCP多轮"}}}' \
       | python3 -c 'import json,sys;print(json.load(sys.stdin)["result"]["structuredContent"]["id"])')
[ -n "$MPID" ] && [ "$MPID" != "None" ] || { bad "M4 MCP 建档失败"; }
for i in $(seq 1 "$N"); do
  printf '姓名：mcp多轮\n检查项目：血常规\n白细胞计数 %d.0 10^9/L\n' "$i" > "$H/m$i.txt"
  B64=$(base64 -w0 "$H/m$i.txt")
  rpc_to "$H/mup$i.json" "{\"jsonrpc\":\"2.0\",\"id\":$((100+i)),\"method\":\"tools/call\",\"params\":{\"name\":\"upload_document\",\"arguments\":{\"person_id\":\"$MPID\",\"filename\":\"m$i.txt\",\"content_base64\":\"$B64\"}}}"
done
MTL=$(curl -s "$BASE/api/v1/persons/$MPID/timeline" | python3 -c 'import json,sys;print(json.load(sys.stdin)["count"])')
MDC=$(curl -s "$BASE/api/v1/persons/$MPID/documents" | python3 -c 'import json,sys;print(json.load(sys.stdin)["count"])')
if [ "$MTL" = "$N" ] && [ "$MDC" = "$N" ]; then
  ok "M4 MCP 面多轮写 ⇒ REST 读面同源 timeline=$MTL documents=$MDC（期望 $N）"
else
  bad "M4 MCP 面多轮写 ⇒ REST 读面 timeline=$MTL documents=$MDC（期望 $N）"
fi

# ---- M5：MCP 面并发建档（真并发，一请求一文件）----
CN="${EHR_MCP_CONC:-20}"
CONC_DIR="$H/conc"; mkdir -p "$CONC_DIR"
CURL_PIDS=()
for i in $(seq 1 "$CN"); do
  ( rpc_to "$CONC_DIR/resp-$i.json" "{\"jsonrpc\":\"2.0\",\"id\":$((1000+i)),\"method\":\"tools/call\",\"params\":{\"name\":\"create_person\",\"arguments\":{\"name\":\"MCP并发-$i\"}}}" ) &
  CURL_PIDS+=($!)
done
for p in "${CURL_PIDS[@]}"; do wait "$p" 2>/dev/null; done
python3 - "$H" "$CONC_DIR" "$CN" "$BASE" <<'PY' && ok "M5 MCP 并发建档：registry 逐行完整 + 读面可用" || bad "M5 MCP 并发建档"
import json,sys,os,glob,urllib.request
h,outdir,n,base=sys.argv[1],sys.argv[2],int(sys.argv[3]),sys.argv[4]
ids=set();unparsed=0
for f in glob.glob(os.path.join(outdir,"resp-*.json")):
    try: d=json.load(open(f,encoding='utf-8'))
    except Exception: unparsed+=1; continue
    r=d.get("result",{})
    if r.get("isError"): print("FAIL 并发建档 isError: %s"%r); sys.exit(1)
    i=r.get("structuredContent",{}).get("id")
    if i: ids.add(i)
if unparsed: print("FAIL %d 个响应不是合法 JSON"%unparsed); sys.exit(1)
if len(ids)!=n: print("FAIL 并发建档 id 数 %d != %d"%(len(ids),n)); sys.exit(1)
raw=open(os.path.join(h,"registry","persons.jsonl"),encoding='utf-8').read()
lines=[l for l in raw.split("\n") if l.strip()]
if not lines: print("FAIL registry 为空（D-050：派生的期望值不得为空）"); sys.exit(1)
broken=0
for l in lines:
    try: json.loads(l)
    except Exception: broken+=1
if broken: print("FAIL %d/%d 行是交错/损坏的垃圾"%(broken,len(lines))); sys.exit(1)
try:
    with urllib.request.urlopen(base+"/api/v1/persons",timeout=30) as r: listed=json.load(r)
except Exception as e:
    print("FAIL 读面不可用: %r"%e); sys.exit(1)
if not listed: print("FAIL 读面为空"); sys.exit(1)
print("created_ids=%d registry_lines=%d broken=0 listed=%d"%(len(ids),len(lines),len(listed)))
PY

# ---- M6：真相层逐行完整（含上面所有人）----
python3 - "$H" <<'PY' && ok "M6 真相层每行都是完整 JSON" || bad "M6 真相层完整性"
import json,sys,os
h=sys.argv[1]
path=os.path.join(h,"registry","persons.jsonl")
raw=open(path,encoding='utf-8').read()
lines=[l for l in raw.split("\n") if l.strip()]
if not lines: print("FAIL registry 为空"); sys.exit(1)
bad=0
for i,l in enumerate(lines):
    try: json.loads(l)
    except Exception as e:
        bad+=1
        if bad<4: print("  line %d: %s"%(i+1,e))
if bad: print("FAIL %d/%d 行损坏"%(bad,len(lines))); sys.exit(1)
print("lines=%d intact=ok"%len(lines))
PY

note "真相层: $H/registry/persons.jsonl"
echo "fails=$fails"
[ "$fails" -eq 0 ]
