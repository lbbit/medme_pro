#!/usr/bin/env bash
# medme-ehr · D-028 写侧手动条目 + 读侧 measurements 投影 e2e 门禁（真进程 HTTP 面）
#
# 判据：四个端点从 stub→501 毕业为真实现，且：
#   ① **写入真落地**：写完立刻可在 measurements / documents / labs 看到
#   ② **不得静默吞写**：两条内容相同的笔记 ⇒ 两条记录（CAS 按内容去重的坑）
#   ③ **数值不被粉饰**：不可能的值 ⇒ 400 + 原因；未知 analyte ⇒ 无区间无标志
#   ④ **契约一致**：OpenAPI 里是**真 operation**（非 stub→501）+ typed schema
#   ⑤ **错误语义**：未知人 → 404；不存在的 kind/日期 → 400
#   ⑥ **不回归**：labs / medications / conditions 仍正常（自测值不污染医院指标面）
#
# 依赖：cargo（构建）+ curl + python3；不依赖 docker。
set -uo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT"

export PATH="$HOME/.cargo/bin:$PATH"
. "$ROOT/ci/env.sh"
PORT="${PORT:-18671}"
BASE="http://127.0.0.1:${PORT}"
API="$BASE/api/v1"
BIN="$EHR_DBG_BIN"
HOME_DIR="$(mktemp -d /tmp/ehr-entries-e2e-XXXXXX)"
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

echo "== 3) 建档 =="
pid=$(curl -fsS -X POST "$API/persons" -H 'content-type: application/json' \
  -d '{"name":"林宝妹","sex":"F","birth_year":1988}' \
  | python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])')
[ -n "$pid" ] && pass "person created ($pid)" || { fail "person create"; exit 1; }

echo "== 4) 写侧：self-measurement / note / profile-event ⇒ 201 =="
code=$(curl -sS -o "$HOME_DIR/sm.json" -w '%{http_code}' -X POST "$API/persons/$pid/entries/self-measurement" \
  -H 'content-type: application/json' \
  -d '{"values":[{"analyte_key":"bp_systolic","value":128,"unit":"mmHg"},{"analyte_key":"bp_diastolic","value":82,"unit":"mmHg"}],"taken_at":"2026-10-23T08:30:00+08:00"}')
[ "$code" = 201 ] && pass "self-measurement → 201" || fail "self-measurement → $code (期望 201；501 说明仍是 stub)"
code=$(curl -sS -o "$HOME_DIR/n1.json" -w '%{http_code}' -X POST "$API/persons/$pid/entries/note" \
  -H 'content-type: application/json' -d '{"text":"今天有点头晕"}')
[ "$code" = 201 ] && pass "note → 201" || fail "note → $code"
code=$(curl -sS -o "$HOME_DIR/n2.json" -w '%{http_code}' -X POST "$API/persons/$pid/entries/note" \
  -H 'content-type: application/json' -d '{"text":"今天有点头晕"}')
[ "$code" = 201 ] && pass "note(重复内容) → 201" || fail "note2 → $code"
code=$(curl -sS -o "$HOME_DIR/pe.json" -w '%{http_code}' -X POST "$API/persons/$pid/entries/profile-event" \
  -H 'content-type: application/json' \
  -d '{"kind":"enable","package":"sle","at":"2026-10-20","payload":{"severity":"mild"}}')
[ "$code" = 201 ] && pass "profile-event → 201" || fail "profile-event → $code"

echo "== 5) M1 写入真落地 + 不得静默吞写（两条相同笔记 ⇒ 两条记录） =="
SM="$HOME_DIR/sm.json" N1="$HOME_DIR/n1.json" N2="$HOME_DIR/n2.json" PE="$HOME_DIR/pe.json" python3 <<'PY' && ok=y || ok=n
import json,os
sm=json.load(open(os.environ["SM"])); n1=json.load(open(os.environ["N1"]))
n2=json.load(open(os.environ["N2"])); pe=json.load(open(os.environ["PE"]))
for d in (sm,n1,n2,pe):
    assert len(d["content_hash"])==64, ("回执必须带 CAS sha256",d)
    assert d["deduped"] is False, ("正常路径不得判成重复",d)
assert n1["content_hash"]!=n2["content_hash"], ("两条内容相同的笔记必须落成两条（CAS 去重不得吞写）",n1,n2)
assert sm["doc_type"]=="self_measurement" and pe["doc_type"]=="profile_event", (sm,pe)
PY
[ "${ok:-n}" = y ] && pass "M1 三类写入都有 sha256 回执，且相同内容未被吞写" || fail "M1 写入落地/吞写判据"

echo "== 6) 读侧 measurements → 200 + 载荷逐字读回 =="
code=$(curl -sS -o "$HOME_DIR/meas.json" -w '%{http_code}' "$API/persons/$pid/measurements")
[ "$code" = 200 ] && pass "measurements → 200" || fail "measurements → $code (期望 200；501 说明仍是 stub)"
MEAS="$HOME_DIR/meas.json" python3 <<'PY' && ok=y || ok=n
import json,os
d=json.load(open(os.environ["MEAS"]))
assert d["person_id"], d
assert d["count"]==4, ("三条写入 + 一条重复笔记 ⇒ 4 条记录",d)
by={}
for m in d["measurements"]:
    assert m["payload_readable"] is True, m
    by.setdefault(m["kind"],[]).append(m)
assert len(by["note"])==2, ("两条笔记都在（不得被去重吞掉）",by)
sm=by["self_measurement"][0]
assert len(sm["values"])==2, ("血压是两个值",sm)
v={x["analyte_key"]:x for x in sm["values"]}
assert v["bp_systolic"]["value"]==128 and v["bp_systolic"]["unit"]=="mmHg", v
assert v["bp_systolic"]["ref_high"]==135, ("家庭区间上限应原样透传（上游 home_ref_range）",v)
assert v["bp_systolic"]["ref_source"], ("区间必须带出处",v)
assert v["bp_systolic"]["flag"]=="N", ("128 在上限内 ⇒ N",v)
pe=by["profile_event"][0]
assert pe["event_kind"]=="enable" and pe["event_package"]=="sle", pe
assert pe["event_at"]=="2026-10-20", ("事件发生日原样透传",pe)
assert pe["event_payload"]["severity"]=="mild", pe
for m in by["note"]:
    assert m["text"]=="今天有点头晕", ("note 原文原样透传（记录时间行已剥掉）",m)
PY
[ "${ok:-n}" = y ] && pass "M2 measurements 200：载荷逐字读回 + 区间带出处 + 事件日透传 + 笔记原文原样" || fail "M2 measurements 语义断言"

echo "== 7) M3 证据回链：docref 可取回原件字节且含上游载荷哨兵 =="
MEAS="$HOME_DIR/meas.json" API="$API" python3 <<'PY' && ok=y || ok=n
import json,os,urllib.request
d=json.load(open(os.environ["MEAS"])); api=os.environ["API"]; pid=d["person_id"]
checked=0
for m in d["measurements"]:
    with urllib.request.urlopen(f"{api}/persons/{pid}/documents/{m['docref']}",timeout=15) as r:
        dd=json.load(r)
    assert dd["content_hash"]==m["docref"], ("detail 的 hash 必须与 docref 一致",dd)
    assert dd.get("text"), ("docref 指向的文档必须有文本层",dd)
    with urllib.request.urlopen(f"{api}/persons/{pid}/documents/{m['docref']}/original",timeout=15) as r:
        raw=r.read()
    assert len(raw)>0, "原件字节不得为空"
    txt=raw.decode("utf-8")
    if m["kind"]=="self_measurement":
        assert "###MEDME-SELF-V1###" in txt, ("自测正文须带上游版本化载荷哨兵",txt[:200])
    if m["kind"]=="profile_event":
        assert "###MEDME-PROFILE-V1###" in txt, ("动作日志正文须带上游版本化载荷哨兵",txt[:200])
    checked+=1
print(f"M3 provenance verified measurements={checked}")
PY
[ "${ok:-n}" = y ] && pass "M3 每条记录的 docref 都能取回原件字节（含上游载荷哨兵）" || fail "M3 measurements 证据回链"

echo "== 8) M4 数值不被粉饰：不可能的值 ⇒ 400 + 原因；未知 analyte ⇒ 无区间无标志 =="
code=$(curl -sS -o "$HOME_DIR/bad.json" -w '%{http_code}' -X POST "$API/persons/$pid/entries/self-measurement" \
  -H 'content-type: application/json' \
  -d '{"values":[{"analyte_key":"bp_systolic","value":138388,"unit":"mmHg"}]}')
[ "$code" = 400 ] && pass "不可能的值 → 400" || fail "不可能的值 → $code (期望 400)"
grep -q "implausible" "$HOME_DIR/bad.json" && pass "400 响应带具体原因" || fail "400 响应未带原因: $(cat "$HOME_DIR/bad.json")"
code=$(curl -sS -o "$HOME_DIR/unk.json" -w '%{http_code}' -X POST "$API/persons/$pid/entries/self-measurement" \
  -H 'content-type: application/json' \
  -d '{"values":[{"analyte_key":"mystery_marker","value":42,"unit":"x"}]}')
[ "$code" = 201 ] && pass "未知 analyte → 201（上游刻意不做白名单）" || fail "未知 analyte → $code"
code=$(curl -sS -o "$HOME_DIR/meas2.json" -w '%{http_code}' "$API/persons/$pid/measurements?kind=self_measurement")
[ "$code" = 200 ] && pass "kind 过滤器 → 200" || fail "kind 过滤器 → $code"
MEAS2="$HOME_DIR/meas2.json" python3 <<'PY' && ok=y || ok=n
import json,os
d=json.load(open(os.environ["MEAS2"]))
for m in d["measurements"]:
    for v in m.get("values",[]):
        if v["analyte_key"]=="mystery_marker":
            assert "ref_low" not in v and "ref_high" not in v, ("未知项不得有区间",v)
            assert "flag" not in v, ("无区间时不得猜标志",v)
PY
[ "${ok:-n}" = y ] && pass "M4 未知 analyte 无区间无标志" || fail "M4 未知 analyte 语义"

echo "== 9) M5 过滤器真过滤（参数生效有证据） =="
code=$(curl -sS -o "$HOME_DIR/m_note.json" -w '%{http_code}' "$API/persons/$pid/measurements?kind=note")
[ "$code" = 200 ] && pass "kind=note → 200" || fail "kind=note → $code"
MN="$HOME_DIR/m_note.json" python3 -c '
import json,os
d=json.load(open(os.environ["MN"]))
assert d["count"]==2 and all(m["kind"]=="note" for m in d["measurements"]), d
' && pass "kind=note 只回 2 条 note" || fail "kind=note 过滤未生效"
code=$(curl -sS -o /dev/null -w '%{http_code}' "$API/persons/$pid/measurements?from=2026-10-22&to=2026-10-24")
[ "$code" = 200 ] && pass "from/to → 200" || fail "from/to → $code"
code=$(curl -sS -o /dev/null -w '%{http_code}' "$API/persons/$pid/measurements?kind=nope")
[ "$code" = 400 ] && pass "非法 kind → 400" || fail "非法 kind → $code (期望 400)"
code=$(curl -sS -o /dev/null -w '%{http_code}' "$API/persons/$pid/measurements?from=2026-13-40")
[ "$code" = 400 ] && pass "非法日期 → 400" || fail "非法日期 → $code (期望 400)"

echo "== 10) M6 契约一致：OpenAPI 里是**真 operation**（非 stub→501）+ typed schema =="
curl -fsS "$BASE/openapi.json" -o "$HOME_DIR/openapi.json"
# D-021：「stub→501」字面量从实现派生清单取（`stub_mark`），不再手抄。
STUB_MARK=$(EHR_HOME="$HOME_DIR" "$BIN" --print-rest-manifest 2>/dev/null | python3 -c 'import json,sys;print(json.load(sys.stdin)["stub_mark"])')
OC="$HOME_DIR/openapi.json" STUB_MARK="$STUB_MARK" python3 <<'PY' && ok=y || ok=n
import json,os
d=json.load(open(os.environ["OC"]))
mark=os.environ["STUB_MARK"]
s=d["components"]["schemas"]
for p,m in [("/api/v1/persons/{pid}/measurements","get"),
            ("/api/v1/persons/{pid}/entries/self-measurement","post"),
            ("/api/v1/persons/{pid}/entries/note","post"),
            ("/api/v1/persons/{pid}/entries/profile-event","post")]:
    op=d["paths"][p][m]
    assert mark not in (op.get("summary") or ""), ("端点仍被标成 stub",p,op.get("summary"))
    assert "200" in op.get("responses",{}) or "201" in op.get("responses",{}), (p,op)
for k in ("MeasurementsReply","MeasurementOut","MeasurementValueOut","EntryReply",
          "SelfMeasurementIn","NoteIn","ProfileEventIn","SelfMeasuredValueIn"):
    assert k in s, ("missing schema "+k, sorted(s))
assert {"person_id","count","measurements"} <= set(s["MeasurementsReply"]["properties"])
assert {"document_id","kind","taken_at","docref","payload_readable"} <= set(s["MeasurementOut"]["properties"])
PY
[ "${ok:-n}" = y ] && pass "M6 openapi：四个端点是真 operation + typed schema" || fail "M6 openapi 契约"

echo "== 11) M7 不回归：labs / medications / conditions 仍正常（自测值不污染医院指标面） =="
code=$(curl -sS -o "$HOME_DIR/labs.json" -w '%{http_code}' "$API/persons/$pid/labs")
[ "$code" = 200 ] && pass "labs → 200" || fail "labs → $code"
code=$(curl -sS -o "$HOME_DIR/meds.json" -w '%{http_code}' "$API/persons/$pid/medications")
[ "$code" = 200 ] && pass "medications → 200" || fail "medications → $code"
code=$(curl -sS -o "$HOME_DIR/conds.json" -w '%{http_code}' "$API/persons/$pid/conditions")
[ "$code" = 200 ] && pass "conditions → 200" || fail "conditions → $code"
LABS="$HOME_DIR/labs.json" python3 <<'PY' && ok=y || ok=n
import json,os
d=json.load(open(os.environ["LABS"]))
# 自测值走 GroupKey::SelfMeasured，与医院值永不同组；本箱只有自测值 ⇒ 只应有自测序列
assert d["total_points"]==3, ("自测的三条值（血压2 + 未知项1）应进 labs 的自测序列",d)
assert d["series_count"]==3, d
for s in d["series"]:
    # 关键：消费方必须能分辨「家庭自测」与「医院报告」——上游用 GroupKey 分开，
    # 但只有把 self_measured 透传出来，消费方才不会拿家庭区间当门诊区间读。
    assert s.get("self_measured") is True, ("自测序列必须标 self_measured=true",s)
    for p in s["points"]:
        assert p.get("unverified") is False, p
PY
[ "${ok:-n}" = y ] && pass "M7 labs 未退化：自测值只进自测序列且标 self_measured=true" || fail "M7 labs 受影响"

echo "== 12) 错误语义：未知人 → 404 =="
code=$(curl -sS -o /dev/null -w '%{http_code}' "$API/persons/00000000-0000-4000-8000-000000000000/measurements")
[ "$code" = 404 ] && pass "unknown person measurements → 404" || fail "unknown person measurements → $code (期望 404)"

echo "== 13) 门禁真的会红：四个端点不得留在 stub 清单 =="
# D-037：判据从「手写正则解析 lib.rs 源码」换成**实现派生清单**（`--print-rest-manifest`）。
# 旧实现的正则 `pub const CONTRACT[^=]*=\s*&\[(.*?)\n\];` 在 CONTRACT 变空后会
# **跨表匹配到 MOUNTED_UNDOCUMENTED** ⇒ 这道门实际在查错的表（假门）。
MAN=$(EHR_HOME="$HOME_DIR" "$BIN" --print-rest-manifest 2>/dev/null)
STUB_PATHS=$(echo "$MAN" | python3 -c 'import json,sys;print("\n".join(json.load(sys.stdin)["stub_paths"]))')
BAD=$(echo "$STUB_PATHS" | grep -E "measurements|entries/" || true)
if [ -z "$BAD" ]; then
  pass "M8 四个端点都不在 stub 清单（实装端点不得再被宣称 501）"
else
  fail "M8 仍有端点留在 stub 清单里"; echo "$BAD" | sed 's/^/    /'
fi

echo "fails=$fails"
exit $([ "$fails" -eq 0 ] && echo 0 || echo 1)
