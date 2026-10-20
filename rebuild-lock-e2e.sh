#!/usr/bin/env bash
# rebuild-lock-e2e.sh —— D-024 门禁：重建期互斥（锁文件 + 服务面 503 拒写 + 陈旧锁回收 + 并发重建被拒）
#
# 判定（每门独立证据）：
#   L1 服务起来后业务路径 200（基线：没锁时一切正常）
#   L2 持锁（模拟重建进行中）⇒ 业务路径 503 + body 含 "rebuild in progress" + Retry-After 头
#   L3 持锁期间公开路径仍 200（healthz/openapi.json 不碰派生层，不该被误伤）
#   L4 释放锁后立刻恢复 200（锁不是"只能重启"的闸）
#   L5 并发重建被拒：持锁时另起 --rebuild-index ⇒ 非零退出 + 报 "already held"
#   L6 陈旧锁（pid 已死）不阻塞服务（200），且可被重建命令回收
#   L7 锁文件不在 vaults/ 下（重建不会删掉自己的锁 ⇒ 不会自锁）
set -u -o pipefail; umask 077
BIN="${1:?usage: rebuild-lock-e2e.sh abs-binary-path [port]}"; PORT="${2:-8963}"
API="http://127.0.0.1:${PORT}/api/v1"
H=$(mktemp -d /tmp/ehrrl.XXXXXX) || { echo FATAL-TMPDIR; exit 3; }
fails=0; SRV_PID=""
g(){ if [[ "$2" == y ]]; then echo "[GATE PASS] $1"; else fails=$((fails+1)); echo "[GATE FAIL] $1${3:+ | $3}"; fi; }
cleanup(){ [[ -n "$SRV_PID" ]] && kill "$SRV_PID" 2>/dev/null; wait "$SRV_PID" 2>/dev/null; rm -rf "$H"; }
trap cleanup EXIT

test -x "$BIN" || { echo "GATE-FATAL-BIN $BIN"; exit 4; }
LOCK="$H/.rebuild.lock"

EHR_HOME="$H" "$BIN" --addr="127.0.0.1:${PORT}" >"$H/serve.log" 2>&1 & SRV_PID=$!
ok=n; for i in $(seq 1 40); do curl --max-time 5 -fsS "$API/healthz" | grep -q '"status":"ok"' && { ok=y; break; }; sleep 0.25; done
g "L0 server up" "$ok" "$(head -3 "$H/serve.log")"
[[ "$ok" == y ]] || exit 7

# L1 基线：业务路径 200
code=$(curl -s -o /dev/null -w '%{http_code}' "$API/persons")
[[ "$code" == 200 ]] && ok=y||ok=n; g "L1 baseline business path 200" "$ok" "code=$code"

# L2 持锁 ⇒ 503 + 原因
printf 'pid=%s\nts=2026-01-01T00:00:00Z\nmode=rebuild\n' "$$" > "$LOCK"
body=$(curl -s -w '\n%{http_code}' "$API/persons"); code="${body##*$'\n'}"; btxt="${body%$'\n'*}"
[[ "$code" == 503 ]] && ok=y||ok=n; g "L2a business path 503 while lock held" "$ok" "code=$code body=$btxt"
echo "$btxt" | grep -q "rebuild in progress" && ok=y||ok=n; g "L2b 503 body explains rebuild in progress" "$ok" "$btxt"
ra=$(curl -s -D - -o /dev/null "$API/persons" | grep -i '^retry-after' | tr -d '\r')
[[ -n "$ra" ]] && ok=y||ok=n; g "L2c Retry-After header present" "$ok" "$ra"

# L3 公开路径仍 200
for p in /api/v1/healthz /openapi.json /; do
  code=$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:${PORT}$p")
  [[ "$code" == 200 ]] && ok=y||ok=n; g "L3 public path $p still 200" "$ok" "code=$code"
done

# L5 并发重建被拒（锁仍在）
EHR_HOME="$H" "$BIN" --rebuild-index >/dev/null 2>"$H/conc.err"; rc=$?
[[ $rc -ne 0 ]] && ok=y||ok=n; g "L5a concurrent rebuild rejected (rc=$rc)" "$ok"
grep -q "already held" "$H/conc.err" && ok=y||ok=n; g "L5b error says 'already held'" "$ok" "$(head -2 "$H/conc.err")"

# L4 释放锁 ⇒ 恢复 200
rm -f "$LOCK"
code=$(curl -s -o /dev/null -w '%{http_code}' "$API/persons")
[[ "$code" == 200 ]] && ok=y||ok=n; g "L4 business path 200 again after lock released" "$ok" "code=$code"

# L6 陈旧锁（pid 指向不存在的进程）⇒ 不阻塞服务，且可被重建回收
printf 'pid=4294967295\nts=2020-01-01T00:00:00Z\nmode=rebuild\n' > "$LOCK"
code=$(curl -s -o /dev/null -w '%{http_code}' "$API/persons")
[[ "$code" == 200 ]] && ok=y||ok=n; g "L6a stale lock does not block service" "$ok" "code=$code"
EHR_HOME="$H" "$BIN" --rebuild-index >/dev/null 2>"$H/stale.err"; rc=$?
[[ $rc -eq 0 ]] && ok=y||ok=n; g "L6b rebuild reclaims stale lock (rc=$rc)" "$ok" "$(head -2 "$H/stale.err")"
[[ -f "$LOCK" ]] && ok=n||ok=y; g "L6c lock released after rebuild finished" "$ok"

# L7 锁文件不在 vaults/ 下
[[ ! -d "$H/vaults/.rebuild.lock" ]] && ok=y||ok=n; g "L7 lock file not inside vaults/" "$ok"

echo "== RESULT fails=$fails"
exit $(( fails>0 ? 1 : 0 ))
