#!/usr/bin/env bash
# medme-ehr · T4 鉴权 e2e 门禁（真进程）：起一个带 token 的 ehr-api，逐条断言 HTTP 行为。
# 判据（PLAN T4）：无 token 401 / 错 token 401 / ro token 读全绿 / ro token 写 403 / rw token 写全绿。
# 依赖：cargo（构建）+ curl + python3；不依赖 docker。
set -uo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT/ehr-api"

export PATH="$HOME/.cargo/bin:$PATH"
# 产物路径的**唯一来源**（ci/env.sh）——本脚本原先用相对路径 ./target/debug/ehr-api，
# 而 cargo build 写进 $CARGO_TARGET_DIR ⇒ 两者不同时它启动的是**另一份（可能很旧的）产物**，
# 而门禁照样是绿的（D-035 新坑③「先问它跑的是哪一份产物」）。
. "$ROOT/ci/env.sh"
PORT="${PORT:-18655}"
BASE="http://127.0.0.1:${PORT}"
HOME_DIR="$(mktemp -d /tmp/ehr-auth-e2e-XXXXXX)"
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
cargo build --quiet || { echo "FAIL cargo build"; exit 1; }
[ -x "$EHR_DBG_BIN" ] || { echo "FAIL no binary at $EHR_DBG_BIN"; exit 1; }
echo "[证据] 门禁使用的产物 = $EHR_DBG_BIN"

echo "== 2) 启动（auth 开启：rw token + ro token） =="
EHR_HOME="$HOME_DIR" EHR_API_TOKENS="rwsecret:rw,rosecret:ro" \
  "$EHR_DBG_BIN" --addr="127.0.0.1:${PORT}" >"$LOG" 2>&1 &
PID=$!

for i in $(seq 1 50); do
  curl -sS -o /dev/null "$BASE/api/v1/healthz" 2>/dev/null && break
  sleep 0.2
done
if ! curl -sS -o /dev/null "$BASE/api/v1/healthz" 2>/dev/null; then
  echo "FAIL 服务未起来"; cat "$LOG"; exit 1
fi

echo "== 3) 启动 [证据] 行必须显示 auth=bearer(tokens=2) =="
if grep -q "auth=bearer(tokens=2)" "$LOG"; then pass "证据行显示 auth=bearer(tokens=2)"; else
  fail "证据行未显示 auth=bearer(tokens=2)"; cat "$LOG"; fi

code() { curl -sS -o /dev/null -w '%{http_code}' "$@"; }

echo "== 4) 无 token / 错 token ⇒ 401 =="
c=$(code "$BASE/api/v1/persons"); [ "$c" = 401 ] && pass "无 token → 401" || fail "无 token → $c (期望 401)"
c=$(code -H "Authorization: Bearer wrongtoken" "$BASE/api/v1/persons"); [ "$c" = 401 ] && pass "错 token → 401" || fail "错 token → $c (期望 401)"
c=$(code -H "Authorization: rwsecret" "$BASE/api/v1/persons"); [ "$c" = 401 ] && pass "裸 token(缺 Bearer) → 401" || fail "裸 token → $c (期望 401)"
c=$(code "$BASE/mcp" -H 'content-type: application/json' -d '{"jsonrpc":"2.0","id":1,"method":"tools/list"}'); [ "$c" = 401 ] && pass "/mcp 无 token → 401" || fail "/mcp 无 token → $c (期望 401)"

echo "== 5) 公开路径免鉴权 =="
c=$(code "$BASE/api/v1/healthz"); [ "$c" = 200 ] && pass "healthz → 200" || fail "healthz → $c"
c=$(code "$BASE/openapi.json"); [ "$c" = 200 ] && pass "openapi.json → 200" || fail "openapi.json → $c"
c=$(code "$BASE/"); [ "$c" = 200 ] && pass "Web 首页 → 200" || fail "Web 首页 → $c"

echo "== 6) 有 token ⇒ 全绿 =="
c=$(code -H "Authorization: Bearer rwsecret" "$BASE/api/v1/persons"); [ "$c" = 200 ] && pass "rw token 读 persons → 200" || fail "rw token 读 persons → $c"
c=$(code -H "Authorization: Bearer rosecret" "$BASE/api/v1/persons"); [ "$c" = 200 ] && pass "ro token 读 persons → 200" || fail "ro token 读 persons → $c"
c=$(code -H "Authorization: Bearer rosecret" "$BASE/api/v1/persons/nonexistent/timeline"); [ "$c" = 404 ] && pass "ro token 读 timeline(未知人) → 404" || fail "ro token 读 timeline → $c (期望 404)"

echo "== 7) ro token 写 ⇒ 403；rw token 写 ⇒ 201 =="
c=$(code -X POST -H "Authorization: Bearer rosecret" -H 'content-type: application/json' \
     -d '{"name":"只读不该建成"}' "$BASE/api/v1/persons"); [ "$c" = 403 ] && pass "ro token 写 → 403" || fail "ro token 写 → $c (期望 403)"
c=$(code -X POST -H "Authorization: Bearer rwsecret" -H 'content-type: application/json' \
     -d '{"name":"鉴权验证人"}' "$BASE/api/v1/persons"); [ "$c" = 201 ] && pass "rw token 写 → 201" || fail "rw token 写 → $c (期望 201)"
# 403 的写入不得留下半成品
n=$(curl -sS -H "Authorization: Bearer rwsecret" "$BASE/api/v1/persons" | python3 -c 'import json,sys;print(len(json.load(sys.stdin)))')
p=$(curl -sS -H "Authorization: Bearer rwsecret" "$BASE/api/v1/persons" | python3 -c 'import json,sys;print(sum(1 for x in json.load(sys.stdin) if x["name"]=="只读不该建成"))')
[ "$p" = 0 ] && pass "被拒的写未留半成品(persons=$n)" || fail "被拒的写留下了数据(persons=$n)"

echo "== 8) MCP 工具粒度分权 =="
v=$(curl -sS -H "Authorization: Bearer rosecret" -H 'content-type: application/json' \
     -d '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"create_person","arguments":{"name":"x"}}}' "$BASE/mcp" \
     | python3 -c 'import json,sys;print(json.load(sys.stdin)["result"]["_code"])')
[ "$v" = "-32004" ] && pass "ro token 调写侧工具 → -32004" || fail "ro token 调写侧工具 → $v (期望 -32004)"
v=$(curl -sS -H "Authorization: Bearer rosecret" -H 'content-type: application/json' \
     -d '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"list_persons","arguments":{}}}' "$BASE/mcp" \
     | python3 -c 'import json,sys;d=json.load(sys.stdin);print("ok" if d.get("error") is None else "err")')
[ "$v" = "ok" ] && pass "ro token 调读侧工具 → ok" || fail "ro token 调读侧工具 → $v"

echo "== 9) OpenAPI 标 securitySchemes =="
curl -sS "$BASE/openapi.json" > "$HOME_DIR/openapi.json"
v=$(python3 - "$HOME_DIR/openapi.json" <<'PY'
import json,sys
d=json.load(open(sys.argv[1]))
s=d.get("components",{}).get("securitySchemes",{}).get("bearerAuth",{})
print("ok" if s.get("type")=="http" and s.get("scheme")=="bearer" and d.get("security") else "bad")
PY
)
[ "$v" = "ok" ] && pass "openapi 含 bearerAuth + 全局 security" || fail "openapi securitySchemes 缺失"

echo "== 10) open 模式（不配置 token）⇒ 放行且证据行标 auth=open =="
PORT2=$((PORT+1)); HOME2="$(mktemp -d /tmp/ehr-auth-open-XXXXXX)"
EHR_HOME="$HOME2" "$EHR_DBG_BIN" --addr="127.0.0.1:${PORT2}" >"$HOME_DIR/open.log" 2>&1 &
PID2=$!
for i in $(seq 1 50); do curl -sS -o /dev/null "http://127.0.0.1:${PORT2}/api/v1/healthz" 2>/dev/null && break; sleep 0.2; done
c=$(code "http://127.0.0.1:${PORT2}/api/v1/persons")
[ "$c" = 200 ] && pass "open 模式放行 → 200" || fail "open 模式放行 → $c"
grep -q "auth=open" "$HOME_DIR/open.log" && pass "证据行标 auth=open" || fail "证据行未标 auth=open"
kill $PID2 2>/dev/null; wait $PID2 2>/dev/null

echo
echo "fails=$fails"
exit $((fails>0))
