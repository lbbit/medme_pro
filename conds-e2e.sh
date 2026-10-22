#!/usr/bin/env bash
# medme-ehr · D-027 病程泳道（conditions）端点 e2e 门禁（真进程 HTTP 面）
#
# 判据：GET /api/v1/persons/{pid}/conditions 从 stub→501 毕业为真实现，且：
#   ① **不得凭空造诊断**：没有「诊断:」小节标签的化验单**不得**被读出诊断
#   ② **证据回链**：每条诊断的 docref ⊆ 本次上传集，且诊断原文在**其原件原文**里逐字出现
#   ③ **ICD 码只透传不臆造**：原件印了 (E11.9) 才有，没印的必须没有
#   ④ **契约一致**：OpenAPI 里是**真 operation**（非 stub→501），且带 typed schema
#   ⑤ **错误语义**：未知人 → 404（不是 501 也不是 500）
#   ⑥ **不回归**：labs / medications 仍正常（同一批文档、同一份取样判据）
#
# 依赖：cargo（构建）+ curl + python3；不依赖 docker。
set -uo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT"

export PATH="$HOME/.cargo/bin:$PATH"
. "$ROOT/ci/env.sh"
PORT="${PORT:-18661}"
BASE="http://127.0.0.1:${PORT}"
API="$BASE/api/v1"
BIN="$EHR_DBG_BIN"
HOME_DIR="$(mktemp -d /tmp/ehr-conds-e2e-XXXXXX)"
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

echo "== 3) 建档 + 上传三件原件（出院小结 + 处方 + 化验单） =="
pid=$(curl -fsS -X POST "$API/persons" -H 'content-type: application/json' \
  -d '{"name":"林宝妹","sex":"F","birth_year":1988}' \
  | python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])')
[ -n "$pid" ] && pass "person created ($pid)" || { fail "person create"; exit 1; }

cat >"$HOME_DIR/dis.txt" <<'EOF'
XX医院出院记录
姓名:林宝妹  性别:女  年龄:37岁
住院日期:2024-05-01  出院日期:2024-05-08
主要诊断:2型糖尿病(E11.9)
出院诊断:
1. 高血压病3级
2. 慢性肾脏病
医师:陈志强
EOF
cat >"$HOME_DIR/rx.txt" <<'EOF'
复旦大学附属中山医院 门诊处方笺 (prescription)

姓名:林宝妹    性别:女    年龄:37岁    科室:心血管内科
处方日期:2024-06-10    诊断:高血压 3 级(很高危)

Rp.
1. 酒石酸美托洛尔片 Metoprolol  25mg × 20 片
   用法:口服,一次半片(12.5mg),一日 2 次

医师:陈志强
EOF
cat >"$HOME_DIR/lab.txt" <<'EOF'
XX医院检验科检验报告单
姓名:林宝妹  性别:女  年龄:37岁   样本编号:20251015-A042
检验项目          结果      参考区间     标志
白细胞计数        6.2       3.5--9.5     /
血红蛋白          118       115--150     /
糖化血红蛋白      7.1       4.0--6.0     H
报告日期:2025年10月15日
EOF
sha_dis=$(sha256sum "$HOME_DIR/dis.txt" | cut -d' ' -f1)
sha_rx=$(sha256sum "$HOME_DIR/rx.txt" | cut -d' ' -f1)
sha_lab=$(sha256sum "$HOME_DIR/lab.txt" | cut -d' ' -f1)
up_dis=$(curl -fsS -X POST "$API/persons/$pid/documents" -F "file=@$HOME_DIR/dis.txt")
up_rx=$(curl -fsS -X POST "$API/persons/$pid/documents" -F "file=@$HOME_DIR/rx.txt")
up_lab=$(curl -fsS -X POST "$API/persons/$pid/documents" -F "file=@$HOME_DIR/lab.txt")
echo "$up_dis" | grep -q "\"content_hash\":\"$sha_dis\"" && pass "dis doc uploaded" || fail "dis upload: $up_dis"
echo "$up_rx"  | grep -q "\"content_hash\":\"$sha_rx\""  && pass "rx doc uploaded"  || fail "rx upload: $up_rx"
echo "$up_lab" | grep -q "\"content_hash\":\"$sha_lab\"" && pass "lab doc uploaded" || fail "lab upload: $up_lab"

echo "== 4) GET /conditions ⇒ 200 + 不得凭空造诊断 + ICD 只透传 =="
code=$(curl -sS -o "$HOME_DIR/conds.json" -w '%{http_code}' "$API/persons/$pid/conditions")
[ "$code" = 200 ] && pass "conditions → 200" || fail "conditions → $code (期望 200；501 说明仍是 stub)"
CONDS="$HOME_DIR/conds.json" SHA_DIS="$sha_dis" SHA_RX="$sha_rx" SHA_LAB="$sha_lab" python3 <<'PY' && ok=y || ok=n
import json,os
d=json.load(open(os.environ["CONDS"]))
dis=os.environ["SHA_DIS"]; rx=os.environ["SHA_RX"]; lab=os.environ["SHA_LAB"]
assert d["person_id"], d
assert d["documents_scanned"]==3, ("三份原件都应被扫描",d["documents_scanned"])
assert d["condition_count"]>=3, ("出院小结应读出 ≥3 条诊断",d)
texts={d["raw_text"]:d for d in d["conditions"]}
# ① 不得凭空造诊断：化验单没有「诊断:」小节 ⇒ 不得被读出诊断
for c in d["conditions"]:
    assert lab not in c["docrefs"], ("化验单被读出了诊断（无凭无据的诊断不得凭空出现）",c)
    assert c["docrefs"], c
    for r in c["docrefs"]:
        assert r in (dis,rx), ("docref 必须是本次上传集",c)
# ③ ICD 码只透传不臆造
for c in d["conditions"]:
    if c["raw_text"]=="2型糖尿病":
        assert c.get("icd_code")=="E11.9", ("ICD 码应原样透传",c)
    else:
        assert "icd_code" not in c, ("没印 ICD 码的诊断不得凭空带码",c)
# 原文写法原样保留（上游无术语归一字典）
assert "高血压 3 级(很高危)" in texts, ("原文写法必须原样保留",sorted(texts))
assert "2型糖尿病" in texts, ("ICD 码应从展示名剥掉",sorted(texts))
PY
[ "${ok:-n}" = y ] && pass "M1 不得凭空造诊断 + ICD 只透传不臆造 + 原文写法原样" || fail "M1 conditions 语义断言"

echo "== 5) 证据回链第二道：诊断原文必须在**其 docref 原件**里逐字出现（经 HTTP 取原文） =="
CONDS="$HOME_DIR/conds.json" API="$API" python3 <<'PY' && ok=y || ok=n
import json,os,urllib.request
d=json.load(open(os.environ["CONDS"])); api=os.environ["API"]; pid=d["person_id"]; texts={}
checked=0
for c in d["conditions"]:
    for h in c["docrefs"]:
        if h not in texts:
            with urllib.request.urlopen(f"{api}/persons/{pid}/documents/{h}",timeout=15) as r:
                dd=json.load(r); assert dd["content_hash"]==h and dd.get("text"), ("detail missing text",h[:8])
            texts[h]=dd["text"]
        assert c["raw_text"] in texts[h], ("诊断原文不在其原件原文里",c,h[:8])
        checked+=1
print(f"M2 provenance verified conditions={checked}")
PY
[ "${ok:-n}" = y ] && pass "M2 每条诊断的原文都在其 docref 原件里逐字出现（只取自原件红线）" || fail "M2 conditions 证据回链"

echo "== 6) 不回归：labs / medications 仍正常（同一批文档、同一份取样判据） =="
code=$(curl -sS -o "$HOME_DIR/labs.json" -w '%{http_code}' "$API/persons/$pid/labs")
[ "$code" = 200 ] && pass "labs → 200" || fail "labs → $code"
LABS="$HOME_DIR/labs.json" SHA_LAB="$sha_lab" python3 <<'PY' && ok=y || ok=n
import json,os
d=json.load(open(os.environ["LABS"])); lab=os.environ["SHA_LAB"]
assert d["total_points"]>=3, ("化验单的指标应仍被读出",d)
for s in d["series"]:
    for p in s["points"]:
        assert p["docref"]==lab, p
        # 诊断行不得被读成化验指标（units 形如「型糖尿病(...)」即为假指标）
        assert "糖尿病" not in (p.get("unit") or ""), ("诊断行被读成了化验指标",p)
PY
[ "${ok:-n}" = y ] && pass "M3a labs 未因 conditions 实装而退化，且诊断行未被读成假指标" || fail "M3a labs 受影响"
code=$(curl -sS -o "$HOME_DIR/meds.json" -w '%{http_code}' "$API/persons/$pid/medications")
[ "$code" = 200 ] && pass "medications → 200" || fail "medications → $code"
MEDS="$HOME_DIR/meds.json" SHA_RX="$sha_rx" python3 <<'PY' && ok=y || ok=n
import json,os
d=json.load(open(os.environ["MEDS"])); rx=os.environ["SHA_RX"]
assert d["med_count"]>=1, ("处方应仍读出用药",d)
for m in d["medications"]:
    assert rx in m["docrefs"], m
PY
[ "${ok:-n}" = y ] && pass "M3b medications 未因 conditions 实装而退化" || fail "M3b medications 受影响"

echo "== 7) 契约一致：OpenAPI 里是**真 operation**（非 stub→501）+ typed schema =="
curl -fsS "$BASE/openapi.json" -o "$HOME_DIR/openapi.json"
# D-021：「stub→501」字面量从实现派生清单取（`stub_mark`），不再手抄。
STUB_MARK=$(EHR_HOME="$HOME_DIR" "$BIN" --print-rest-manifest 2>/dev/null | python3 -c 'import json,sys;print(json.load(sys.stdin)["stub_mark"])')
OC="$HOME_DIR/openapi.json" STUB_MARK="$STUB_MARK" python3 <<'PY' && ok=y || ok=n
import json,os
d=json.load(open(os.environ["OC"]))
mark=os.environ["STUB_MARK"]
op=d["paths"]["/api/v1/persons/{pid}/conditions"]["get"]
assert mark not in (op.get("summary") or ""), ("端点仍被标成 stub",op.get("summary"))
assert "200" in op.get("responses",{}), op
s=d["components"]["schemas"]
for k in ("ConditionsReply","ConditionOut"):
    assert k in s, ("missing schema "+k, sorted(s))
props=s["ConditionsReply"]["properties"]
assert {"person_id","documents_scanned","condition_count","conditions"} <= set(props), list(props)
cp=s["ConditionOut"]["properties"]
assert {"raw_text","onset","icd_code","docrefs"} <= set(cp), list(cp)
PY
[ "${ok:-n}" = y ] && pass "M4 openapi: conditions 是真 operation + typed schema（含 docref 证据字段）" || fail "M4 openapi 契约"

echo "== 8) 错误语义：未知人 → 404 =="
code=$(curl -sS -o /dev/null -w '%{http_code}' "$API/persons/00000000-0000-4000-8000-000000000000/conditions")
[ "$code" = 404 ] && pass "unknown person conditions → 404" || fail "unknown person conditions → $code (期望 404)"

echo "== 9) 门禁真的会红（参数/状态生效有证据）：conditions 不得留在 stub 清单 =="
# D-037：判据从「手写正则解析 lib.rs 源码」换成**实现派生清单**（`--print-rest-manifest`）。
# 旧实现的正则 `pub const CONTRACT[^=]*=\s*&\[(.*?)\n\];` 在 CONTRACT 变空后会
# **跨表匹配到 MOUNTED_UNDOCUMENTED** ⇒ 这道门实际在查错的表（假门）。
MAN=$(EHR_HOME="$HOME_DIR" "$BIN" --print-rest-manifest 2>/dev/null)
STUB_PATHS=$(echo "$MAN" | python3 -c 'import json,sys;print("\n".join(json.load(sys.stdin)["stub_paths"]))')
if ! echo "$STUB_PATHS" | grep -q "persons/{pid}/conditions"; then
  pass "M5 conditions 不在 stub 清单（实装端点不得再被宣称 501）"
else
  fail "M5 conditions 仍在 stub 清单里"
fi

echo "fails=$fails"
exit $([ "$fails" -eq 0 ] && echo 0 || echo 1)
