#!/usr/bin/env bash
# medme-ehr · D-030 审计端点 e2e 门禁（真进程 HTTP 面）
#
# 判据：GET /api/v1/audit 从 stub→501 毕业为真实现，且：
#   ① **口径分层**：审计条目分 source=registry（人员主数据真相层）与 source=vault（上游内核事件日志）；
#      两层都非空 ⇒ 审计没有「只看一层」的漏账。
#   ② **vault 侧复用上游判据**：导入条目的 sha256 = 上传的原件 sha256（内容哈希留痕，可核验）。
#   ③ **参数真进判据**：`?person=` / `?op=` 真过滤（数字变化即证据），不是摆设。
#   ④ **契约一致**：OpenAPI 里是**真 operation**（非 stub→501）+ typed schema；且不在 CONTRACT 表里。
#   ⑤ **错误语义**：未知人过滤 ⇒ 0 条（不是 500，也不是 404）。
#
# 依赖：cargo（构建）+ curl + python3；不依赖 docker。
set -uo pipefail
cd "$(dirname "$0")"

export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-/home/lbb/medme-shared-target}"
PORT="${PORT:-18670}"
BASE="http://127.0.0.1:${PORT}"
API="$BASE/api/v1"
BIN="$CARGO_TARGET_DIR/debug/ehr-api"
HOME_DIR="$(mktemp -d /tmp/ehr-audit-e2e-XXXXXX)"
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

echo "== 3) 建档两人 + 上传一件原件 =="
pid_a=$(curl -fsS -X POST "$API/persons" -H 'content-type: application/json' \
  -d '{"name":"审计门禁·甲","gender":"F"}' \
  | python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])')
pid_b=$(curl -fsS -X POST "$API/persons" -H 'content-type: application/json' \
  -d '{"name":"审计门禁·乙"}' \
  | python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])')
[ -n "$pid_a" ] && [ -n "$pid_b" ] && pass "persons created" || { fail "person create"; exit 1; }

cat >"$HOME_DIR/lab.txt" <<'EOF'
XX医院检验科检验报告单
姓名:审计门禁·甲  性别:女
检验项目          结果      参考区间     标志
白细胞计数        6.2       3.5--9.5     /
报告日期:2025年10月15日
EOF
sha=$(sha256sum "$HOME_DIR/lab.txt" | cut -d' ' -f1)
up=$(curl -fsS -X POST "$API/persons/$pid_a/documents" -F "file=@$HOME_DIR/lab.txt")
echo "$up" | grep -q "\"content_hash\":\"$sha\"" && pass "doc uploaded (sha=$sha)" || fail "upload: $up"

echo "== 4) GET /audit ⇒ 200 + 两层真相层都在 =="
code=$(curl -sS -o "$HOME_DIR/audit.json" -w '%{http_code}' "$API/audit")
[ "$code" = 200 ] && pass "audit → 200" || fail "audit → $code (期望 200；501 说明仍是 stub)"
AUDIT="$HOME_DIR/audit.json" PID_A="$pid_a" PID_B="$pid_b" SHA="$sha" python3 <<'PY' && ok=y || ok=n
import json,os
d=json.load(open(os.environ["AUDIT"]))
a=os.environ["PID_A"]; b=os.environ["PID_B"]; sha=os.environ["SHA"]
assert d["count"] >= 3, ("至少：甲 create + 乙 create + 甲导入", d)
ents=d["entries"]
reg=[e for e in ents if e["source"]=="registry"]
vau=[e for e in ents if e["source"]=="vault"]
assert len(reg)>=2, ("registry 侧应有两条 create（两人建档）", reg)
assert len(vau)>=1, ("vault 侧应有导入条目", vau)
# registry 侧：op=create、人读摘要、无内容哈希（不臆造）
for e in reg:
    assert e["op"]=="create", e
    assert e["person_id"] in (a,b), e
    assert "sha256" not in e or e["sha256"] is None, ("registry 事件没有内容哈希", e)
    assert e["detail"], e
# vault 侧：内容哈希 = 那份原件的 sha256（内容哈希留痕，可核验）
imp=[e for e in vau if e["sha256"]==sha]
assert imp, ("导入条目必须带原件 sha256", vau)
assert imp[0]["person_id"]==a, imp
assert imp[0]["op"]=="导入", ("vault 侧动作名原样透传上游中文名", imp)
print("count=%d registry=%d vault=%d" % (d["count"], len(reg), len(vau)))
PY
[ "${ok:-n}" = y ] && pass "M1 两层真相层都在（registry create + vault 导入）+ 内容哈希留痕" || fail "M1 审计口径分层"

echo "== 5) 参数真进判据（?person= / ?op= 真过滤，数字变化即证据） =="
ca=$(curl -sS "$API/audit" | python3 -c 'import json,sys; print(json.load(sys.stdin)["count"])')
cb=$(curl -sS "$API/audit?person=$pid_a" | python3 -c 'import json,sys; print(json.load(sys.stdin)["count"])')
cc=$(curl -sS "$API/audit?op=create" | python3 -c 'import json,sys; print(json.load(sys.stdin)["count"])')
cd_=$(curl -sS "$API/audit?op=%E5%AF%BC%E5%85%A5" | python3 -c 'import json,sys; print(json.load(sys.stdin)["count"])')
ce=$(curl -sS "$API/audit?person=00000000-0000-4000-8000-000000000000" | python3 -c 'import json,sys; print(json.load(sys.stdin)["count"])')
echo "counts: all=$ca person_a=$cb op_create=$cc op_导入=$cd_ unknown_person=$ce"
[ "$cb" -lt "$ca" ] && pass "M2a person 过滤真生效（$cb < $ca）" || fail "M2a person 过滤未生效"
[ "$cc" -lt "$ca" ] && pass "M2b op=create 过滤真生效（$cc < $ca）" || fail "M2b op 过滤未生效"
[ "$cd_" -lt "$ca" ] && [ "$cd_" -ge 1 ] && pass "M2c op=导入（上游中文动作名）过滤真生效（$cd_）" || fail "M2c op 中文名过滤未生效"
[ "$ce" = 0 ] && pass "M2d 未知人过滤 ⇒ 0 条（不是 404/500）" || fail "M2d 未知人过滤 ⇒ $ce（期望 0）"

echo "== 6) 契约一致：OpenAPI 里是**真 operation**（非 stub→501）+ typed schema =="
curl -fsS "$BASE/openapi.json" -o "$HOME_DIR/openapi.json"
OC="$HOME_DIR/openapi.json" python3 <<'PY' && ok=y || ok=n
import json,os
d=json.load(open(os.environ["OC"]))
op=d["paths"]["/api/v1/audit"]["get"]
assert "stub→501" not in (op.get("summary") or ""), ("端点仍被标成 stub", op.get("summary"))
assert "200" in op.get("responses",{}), op
s=d["components"]["schemas"]
for k in ("AuditReply","AuditEntryOut"):
    assert k in s, ("missing schema "+k, sorted(s))
props=s["AuditReply"]["properties"]
assert {"persons_scanned","count","entries"} <= set(props), list(props)
ep=s["AuditEntryOut"]["properties"]
assert {"source","op","ts","person_id"} <= set(ep), list(ep)
PY
[ "${ok:-n}" = y ] && pass "M3 openapi: audit 是真 operation + typed schema（含 source 口径字段）" || fail "M3 openapi 契约"

echo "== 7) audit 不在 CONTRACT 表（实装端点不得再被宣称 501） =="
python3 - <<'PY'
import re,sys
s=open("/home/lbb/medme-ehr/ehr-api/src/lib.rs",encoding="utf-8").read()
m=re.search(r"pub const CONTRACT[^=]*=\s*&\[(.*?)\n\];", s, re.S)
body=m.group(1)
bad=[l for l in body.splitlines() if '"/api/v1/audit"' in l]
sys.exit(1 if bad else 0)
PY
[ $? -eq 0 ] && pass "M4 audit 不在 CONTRACT 表" || fail "M4 audit 仍在 CONTRACT 表里"

echo "== 8) stub 端点计数：应剩 1 个（extraction） =="
python3 - <<'PY'
import re,sys
s=open("/home/lbb/medme-ehr/ehr-api/src/lib.rs",encoding="utf-8").read()
m=re.search(r"pub const CONTRACT[^=]*=\s*&\[(.*?)\n\];", s, re.S)
n=len([l for l in m.group(1).splitlines() if l.strip().startswith('("')])
print("CONTRACT stub count=%d" % n)
sys.exit(0 if n==1 else 1)
PY
[ $? -eq 0 ] && pass "M5 CONTRACT 表里剩 1 个 stub（extraction）" || fail "M5 stub 计数不是 1"

echo "fails=$fails"
exit $([ "$fails" -eq 0 ] && echo 0 || echo 1)
