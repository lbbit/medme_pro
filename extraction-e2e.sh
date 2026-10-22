#!/usr/bin/env bash
# medme-ehr · T31「体检报告列布局 labs=0」端到端门禁（真进程 HTTP 面 · **零外部依赖**）
#
# 为什么要有这道门（PLAN 未销账项「体检报告 PDF 列布局 labs=0」）：
#   真实体检报告是**列布局**（项目 | 结果 | 单位 | 参考区间 分列在不同行），
#   上游 `parser::extract_labs` 的正则面向**化验单**（一行一个项目）⇒ 抽出来 0 条。
#   ⇒ 缺口不是「正则不够好」，而是**缺一条「结构化抽取（LLM extraction）能救回来」的路径**。
#   而「救回来了」这件事**必须机器可判** ⇒ 本门。
#
# 判据（缺一即算不通过）：
#   E1 **缺口是真的**：列布局文档进档案后，正则路径 labs **series=0**（复现缺口，不粉饰）。
#   E2 **抽取路径救得回来**：PUT 一条 schema v1 extraction（字段取自该文档的**原文逐字**）后，
#      labs **series≥3** 且点落在我们点名的指标上。
#   E3 **真相层不被污染**：故意塞一条**原文里根本没有**的伪造指标 ⇒ 它**不得**出现在 labs 里
#      （上游 deid::verify 逐字闸 + 落库的是**校验后**的 extraction，D-023 语义）。
#   E4 **持久化**：换一个新进程读同一 EHR_HOME ⇒ series 数不变（抽取结果进的是事件日志+派生索引，
#      不是只在内存里）。
#   E5 **契约一致**：extraction 端点在 OpenAPI 里是**真 operation**（非 stub→501）。
#
# 依赖：cargo（构建）+ curl + python3。**不需要 LLM 网关、不需要 key、不需要网络**。
#   （真实 PDF → LLM 的那一趟由 `tools/pdf_llm_extract.py` 负责取证；本门只锁「抽取路径能救列布局」
#     这条**可离线复现**的性质。）
set -uo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT"

export PATH="$HOME/.cargo/bin:$PATH"
. "$ROOT/ci/env.sh"
PORT="${PORT:-18672}"
BASE="http://127.0.0.1:${PORT}"
API="$BASE/api/v1"
BIN="$EHR_DBG_BIN"
HOME_DIR="$(mktemp -d /tmp/ehr-extraction-e2e-XXXXXX)"
LOG="$HOME_DIR/server.log"
fails=0
pass() { echo "PASS $*"; }
fail() { echo "FAIL $*"; fails=$((fails+1)); }
cleanup() {
  [ -n "${PID:-}" ] && kill "$PID" 2>/dev/null
  wait "${PID:-}" 2>/dev/null
  rm -rf "$HOME_DIR"
}
trap cleanup EXIT

echo "== 1) 构建 =="
if [ ! -x "$BIN" ]; then (cd ehr-api && cargo build --quiet) || { echo "FAIL cargo build"; exit 1; }; fi
[ -x "$BIN" ] || { echo "FAIL no binary at $BIN"; exit 1; }

echo "== 2) 启动真服务 =="
EHR_HOME="$HOME_DIR" "$BIN" --addr="127.0.0.1:${PORT}" >"$LOG" 2>&1 &
PID=$!
for i in $(seq 1 50); do
  curl -sS -o /dev/null "$API/healthz" 2>/dev/null && break
  sleep 0.2
done
if ! curl -sS -o /dev/null "$API/healthz" 2>/dev/null; then
  echo "FAIL 服务未起来"; cat "$LOG"; exit 1
fi

echo "== 3) 建档 + 上传**列布局**体检报告（合成 fixture，不含任何真实个人数据） =="
pid=$(curl -fsS -X POST "$API/persons" -H 'content-type: application/json' \
  -d '{"name":"列布局门禁·甲","gender":"M"}' \
  | python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])')
[ -n "$pid" ] && pass "person created ($pid)" || { fail "person create"; exit 1; }

cat > "$HOME_DIR/col.txt" <<'EOF'
健康体检报告 MEDICAL EXAMINATION REPORT
姓名:列布局门禁·甲
检查日期:2021.09.07
项目
结果
单位
参考区间
白细胞计数
6.20
10^9/L
3.5-9.5
空腹血葡萄糖
5.89
mmol/L
3.9-6.1
总胆固醇
4.62
mmol/L
2.9-5.2
EOF
SHA=$(curl -fsS -X POST "$API/persons/$pid/documents" -F "file=@$HOME_DIR/col.txt" \
  | python3 -c 'import json,sys; print(json.load(sys.stdin)["content_hash"])')
[ -n "$SHA" ] && pass "上传成功 sha=$SHA" || { fail "upload"; exit 1; }

echo "== 4) E1 缺口复现：正则路径 labs = 0 =="
curl -fsS "$API/persons/$pid/labs" -o "$HOME_DIR/labs0.json"
L0="$HOME_DIR/labs0.json" python3 <<'PY' && ok=y || ok=n
import json,os
d=json.load(open(os.environ["L0"]))
assert d["series_count"]==0, ("列布局文档在正则路径下应当抽不出指标（缺口本体）",d)
assert d["total_points"]==0, d
PY
[ "${ok:-n}" = y ] && pass "E1 缺口复现：列布局 labs series=0" || fail "E1 缺口复现"

echo "== 5) E2/E3 抽取路径：回写 extraction（字段逐字取自原文 + 一条伪造项） =="
SHA="$SHA" PID="$pid" API="$API" python3 <<'PY' && ok=y || ok=n
import json,os,urllib.request
api=os.environ["API"]; pid=os.environ["PID"]; sha=os.environ["SHA"]
ext={"doc_type":"other","doc_date":"2021-09-07","labs":[
 {"name":"白细胞计数","value":"6.20","unit":"10^9/L","ref_low":"3.5","ref_high":"9.5","flag":""},
 {"name":"空腹血葡萄糖","value":"5.89","unit":"mmol/L","ref_low":"3.9","ref_high":"6.1","flag":""},
 {"name":"总胆固醇","value":"4.62","unit":"mmol/L","ref_low":"2.9","ref_high":"5.2","flag":""},
 # 伪造项：原文里没有「血糖999」这个串 ⇒ 逐字闸必须判假 ⇒ 不得进真相层
 {"name":"血糖999","value":"99.9","unit":"mmol/L","ref_low":"","ref_high":"","flag":""}],
 "meds":[],"diagnoses":[],"impression":"","notes":""}
body=json.dumps({"result_json":json.dumps(ext,ensure_ascii=False),
                 "backend":"gate-fixture","model_version":"fixture-1",
                 "mode":"text","schema":1}).encode()
req=urllib.request.Request(f"{api}/persons/{pid}/documents/{sha}/extraction",
    data=body,headers={"content-type":"application/json"},method="PUT")
r=json.load(urllib.request.urlopen(req,timeout=30))
print("[证据] 回执 "+json.dumps(r,ensure_ascii=False))
assert r["labs_count"]>=3, ("逐字闸后至少应剩 3 条真实指标",r)
assert r["labs_count"]==3, ("伪造项必须被逐字闸判假并清空（原文里没有它）",r)
assert r["content_hash"]==sha, r
PY
[ "${ok:-n}" = y ] && pass "E2/E3 回写成功：真实指标 3 条留、伪造项被逐字闸清空" || fail "E2/E3 回写/逐字闸"

echo "== 6) labs 投影：series≥3 且伪造项不得出现 =="
curl -fsS "$API/persons/$pid/labs" -o "$HOME_DIR/labs1.json"
L1="$HOME_DIR/labs1.json" python3 <<'PY' && ok=y || ok=n
import json,os
d=json.load(open(os.environ["L1"]))
names=[s.get("group_name","") for s in d["series"]]
print("[证据] series=%d points=%d names=%s"%(d["series_count"],d["total_points"],names))
assert d["series_count"]>=3, ("抽取后必须真的多出指标序列",d)
assert d["total_points"]>=3, d
assert not any("999" in n for n in names), ("伪造项不得出现在 labs 里",names)
PY
[ "${ok:-n}" = y ] && pass "E2 labs 投影：抽取后 series≥3，伪造项不在其中" || fail "E2 labs 投影"

echo "== 7) E4 持久化：换新进程读同一 EHR_HOME =="
SERIES_BEFORE=$(python3 -c 'import json,os;print(json.load(open(os.environ["L1"]))["series_count"])' L1="$HOME_DIR/labs1.json" 2>/dev/null || \
  python3 -c 'import json,sys;print(json.load(open(sys.argv[1]))["series_count"])' "$HOME_DIR/labs1.json")
kill "$PID" 2>/dev/null; wait "$PID" 2>/dev/null
EHR_HOME="$HOME_DIR" "$BIN" --addr="127.0.0.1:${PORT}" >"$HOME_DIR/server2.log" 2>&1 &
PID=$!
for i in $(seq 1 50); do
  curl -sS -o /dev/null "$API/healthz" 2>/dev/null && break
  sleep 0.2
done
curl -fsS "$API/persons/$pid/labs" -o "$HOME_DIR/labs2.json"
L2="$HOME_DIR/labs2.json" SB="$SERIES_BEFORE" python3 <<'PY' && ok=y || ok=n
import json,os
d=json.load(open(os.environ["L2"])); before=int(os.environ["SB"])
print("[证据] 重启后 series=%d（重启前 %d）"%(d["series_count"],before))
assert d["series_count"]==before, ("抽取结果必须落在事件日志+派生索引里，而不是只在内存",d,before)
PY
[ "${ok:-n}" = y ] && pass "E4 持久化：重启后 series 数不变" || fail "E4 持久化"

echo "== 8) E5 契约一致：extraction 端点是真 operation（非 stub→501） =="
curl -fsS "$BASE/openapi.json" -o "$HOME_DIR/openapi.json"
STUB_MARK=$(EHR_HOME="$HOME_DIR" "$BIN" --print-rest-manifest 2>/dev/null | python3 -c 'import json,sys;print(json.load(sys.stdin)["stub_mark"])')
OC="$HOME_DIR/openapi.json" SM="$STUB_MARK" python3 <<'PY' && ok=y || ok=n
import json,os
d=json.load(open(os.environ["OC"])); mark=os.environ["SM"]
op=d["paths"]["/api/v1/persons/{pid}/documents/{sha}/extraction"]["put"]
assert mark not in (op.get("summary") or ""), ("extraction 端点仍被标成 stub",op)
assert "200" in op.get("responses",{}), op
PY
[ "${ok:-n}" = y ] && pass "E5 openapi：extraction 是真 operation" || fail "E5 openapi 契约"

echo "fails=$fails"
exit $([ "$fails" -eq 0 ] && echo 0 || echo 1)
