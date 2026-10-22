#!/usr/bin/env bash
# medme-ehr · D-026 用药端点 e2e 门禁（真进程 HTTP 面）
#
# 判据：GET /api/v1/persons/{pid}/medications 从 stub→501 毕业为真实现，且：
#   ① **类型分流**：化验单**不得**被读成用药（上游 wants_meds 只放行 prescription）
#   ② **证据回链**：每条 mention 的 docref ⊆ 本次上传集，且药名/剂量在**其原件原文**里逐字出现
#   ③ **契约一致**：OpenAPI 里该端点是**真 operation**（非 stub→501），且带 typed schema
#   ④ **错误语义**：未知人 → 404（不是 501 也不是 500）
#
# 依赖：cargo（构建）+ curl + python3；不依赖 docker。
set -uo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT"

export PATH="$HOME/.cargo/bin:$PATH"
. "$ROOT/ci/env.sh"
PORT="${PORT:-18660}"
BASE="http://127.0.0.1:${PORT}"
API="$BASE/api/v1"
BIN="$EHR_DBG_BIN"
HOME_DIR="$(mktemp -d /tmp/ehr-meds-e2e-XXXXXX)"
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

echo "== 3) 建档 + 上传两件原件（化验单 + 处方） =="
pid=$(curl -fsS -X POST "$API/persons" -H 'content-type: application/json' \
  -d '{"name":"林宝妹","sex":"F","birth_year":1988}' \
  | python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])')
[ -n "$pid" ] && pass "person created ($pid)" || { fail "person create"; exit 1; }

cat >"$HOME_DIR/lab.txt" <<'EOF'
XX医院检验科检验报告单
姓名:林宝妹  性别:女  年龄:37岁   样本编号:20251015-A042
检验项目          结果      参考区间     标志
白细胞计数        6.2       3.5--9.5     /
血红蛋白          118       115--150     /
糖化血红蛋白      7.1       4.0--6.0     H
报告日期:2025年10月15日
EOF
cat >"$HOME_DIR/rx.txt" <<'EOF'
复旦大学附属中山医院 门诊处方笺 (prescription)

姓名:林宝妹    性别:女    年龄:37岁    科室:心血管内科
处方日期:2024-06-10    诊断:高血压 3 级(很高危)

Rp.
1. 酒石酸美托洛尔片 Metoprolol  25mg × 20 片
   用法:口服,一次半片(12.5mg),一日 2 次
2. 苯磺酸氨氯地平片 Amlodipine  5mg × 14 片
   用法:口服,一次 1 片,一日 1 次(晨服)
3. 阿司匹林肠溶片 Aspirin  100mg × 30 片
   用法:口服,一次 1 片,一日 1 次(餐后)

医师:陈志强
EOF
sha_lab=$(sha256sum "$HOME_DIR/lab.txt" | cut -d' ' -f1)
sha_rx=$(sha256sum "$HOME_DIR/rx.txt" | cut -d' ' -f1)
up_lab=$(curl -fsS -X POST "$API/persons/$pid/documents" -F "file=@$HOME_DIR/lab.txt")
up_rx=$(curl -fsS -X POST "$API/persons/$pid/documents" -F "file=@$HOME_DIR/rx.txt")
echo "$up_lab" | grep -q "\"content_hash\":\"$sha_lab\"" && pass "lab doc uploaded (sha=$sha_lab)" || fail "lab upload: $up_lab"
echo "$up_rx" | grep -q "\"content_hash\":\"$sha_rx\"" && pass "rx doc uploaded (sha=$sha_rx)" || fail "rx upload: $up_rx"

echo "== 4) GET /medications ⇒ 200 + 类型分流 + 证据回链 =="
code=$(curl -sS -o "$HOME_DIR/meds.json" -w '%{http_code}' "$API/persons/$pid/medications")
[ "$code" = 200 ] && pass "medications → 200" || fail "medications → $code (期望 200；501 说明仍是 stub)"
MEDS="$HOME_DIR/meds.json" SHA_LAB="$sha_lab" SHA_RX="$sha_rx" python3 <<'PY' && ok=y || ok=n
import json,os
d=json.load(open(os.environ["MEDS"]))
lab=os.environ["SHA_LAB"]; rx=os.environ["SHA_RX"]
assert d["person_id"], d
assert d["documents_scanned"]==2, ("两份原件都应被扫描",d["documents_scanned"])
assert d["med_count"]>=3, ("处方应读出 ≥3 个药",d)
assert d["mention_count"]>=3, d
names=[m["name"] for m in d["medications"]]
for want in ("美托洛尔","氨氯地平","阿司匹林"):
    assert any(want in n for n in names), (want, names)
for m in d["medications"]:
    # ① 类型分流：化验单不得被读成用药
    assert lab not in m["docrefs"], ("化验单被读成了用药（doc_type 分流退化）",m)
    assert m["docrefs"]==[rx], ("docrefs 必须是处方原件",m)
    for mm in m["mentions"]:
        assert mm["docref"]==rx, mm
        assert mm["origin"]=="regex", ("origin 应透传上游标注",mm)
        assert mm["unverified"] is False, mm
    # status 透传上游（不推断停药）
    assert m["status"]=="active", m
PY
[ "${ok:-n}" = y ] && pass "M1 类型分流：用药只来自处方（化验单 docref 不出现）+ 药名/剂量/原文逐字" || fail "M1 meds 语义断言"

echo "== 5) 证据回链第二道：药名/剂量必须在**其 docref 原件**里逐字出现（经 HTTP 取原文） =="
MEDS="$HOME_DIR/meds.json" API="$API" python3 <<'PY' && ok=y || ok=n
import json,os,urllib.request
d=json.load(open(os.environ["MEDS"])); api=os.environ["API"]; pid=d["person_id"]; texts={}
checked=0
for m in d["medications"]:
    for mm in m["mentions"]:
        h=mm["docref"]; checked+=1
        if h not in texts:
            with urllib.request.urlopen(f"{api}/persons/{pid}/documents/{h}",timeout=15) as r:
                dd=json.load(r); assert dd["content_hash"]==h and dd.get("text"), ("detail missing text",h[:8])
            texts[h]=dd["text"]
        t=texts[h]
        assert mm["raw_name"] in t, ("药名不在原件原文",mm)
        if mm.get("dose"): assert mm["dose"] in t, ("剂量不在原件原文",mm)
print(f"M2 provenance verified mentions={checked}")
PY
[ "${ok:-n}" = y ] && pass "M2 每条 mention 的药名/剂量都在其原件原文里逐字出现（只取自原件红线）" || fail "M2 meds 证据回链"

echo "== 6) labs 不受影响（同一份化验单仍能读出指标，且**没被**读成药） =="
code=$(curl -sS -o "$HOME_DIR/labs.json" -w '%{http_code}' "$API/persons/$pid/labs")
[ "$code" = 200 ] && pass "labs → 200" || fail "labs → $code"
LABS="$HOME_DIR/labs.json" SHA_LAB="$sha_lab" python3 <<'PY' && ok=y || ok=n
import json,os
d=json.load(open(os.environ["LABS"])); lab=os.environ["SHA_LAB"]
assert d["total_points"]>=3, ("化验单的指标应仍被读出",d)
for s in d["series"]:
    for p in s["points"]:
        assert p["docref"]==lab, p
PY
[ "${ok:-n}" = y ] && pass "M3 labs 读侧未因 doc_type 分流而退化（指标点仍在、docref 仍指向化验单）" || fail "M3 labs 受影响"

echo "== 7) 契约一致：OpenAPI 里是**真 operation**（非 stub→501）+ typed schema =="
curl -fsS "$BASE/openapi.json" -o "$HOME_DIR/openapi.json"
# D-021：「stub→501」字面量从实现派生清单取（`stub_mark`），不再手抄。
STUB_MARK=$(EHR_HOME="$HOME_DIR" "$BIN" --print-rest-manifest 2>/dev/null | python3 -c 'import json,sys;print(json.load(sys.stdin)["stub_mark"])')
OC="$HOME_DIR/openapi.json" STUB_MARK="$STUB_MARK" python3 <<'PY' && ok=y || ok=n
import json,os
d=json.load(open(os.environ["OC"]))
mark=os.environ["STUB_MARK"]
op=d["paths"]["/api/v1/persons/{pid}/medications"]["get"]
assert mark not in (op.get("summary") or ""), ("端点仍被标成 stub",op.get("summary"))
assert "200" in op.get("responses",{}), op
s=d["components"]["schemas"]
for k in ("MedicationsReply","MedSpanOut","MedMentionOut"):
    assert k in s, ("missing schema "+k, sorted(s))
props=s["MedicationsReply"]["properties"]
assert {"person_id","documents_scanned","med_count","medications"} <= set(props), list(props)
mp=s["MedSpanOut"]["properties"]
assert {"name","docrefs","mentions","status"} <= set(mp), list(mp)
mm=s["MedMentionOut"]["properties"]
assert {"raw_name","dose","docref","origin"} <= set(mm), list(mm)
PY
[ "${ok:-n}" = y ] && pass "M4 openapi: medications 是真 operation + typed schema（含 docref 证据字段）" || fail "M4 openapi 契约"

echo "== 8) 错误语义：未知人 → 404 =="
code=$(curl -sS -o /dev/null -w '%{http_code}' "$API/persons/00000000-0000-4000-8000-000000000000/medications")
[ "$code" = 404 ] && pass "unknown person medications → 404" || fail "unknown person medications → $code (期望 404)"

echo "== 9) 门禁真的会红（参数/状态生效有证据）：把端点挂回 stub 清单 ⇒ 该表断言必须红 =="
# 直接断言：medications 不得出现在 stub 清单（入表即宣称 stub→501，D-025 纪律）
# D-037：判据从「手写正则解析 lib.rs 源码」换成**实现派生清单**（`--print-rest-manifest`）。
# 旧实现的正则 `pub const CONTRACT[^=]*=\s*&\[(.*?)\n\];` 在 CONTRACT 变空后会
# **跨表匹配到 MOUNTED_UNDOCUMENTED** ⇒ 这道门实际在查错的表（假门）。
MAN=$(EHR_HOME="$HOME_DIR" "$BIN" --print-rest-manifest 2>/dev/null)
STUB_PATHS=$(echo "$MAN" | python3 -c 'import json,sys;print("\n".join(json.load(sys.stdin)["stub_paths"]))')
if ! echo "$STUB_PATHS" | grep -q "persons/{pid}/medications"; then
  pass "M5 medications 不在 stub 清单（实装端点不得再被宣称 501）"
else
  fail "M5 medications 仍在 stub 清单里"
fi

echo "fails=$fails"
exit $([ "$fails" -eq 0 ] && echo 0 || echo 1)
