#!/usr/bin/env bash
# rebuild-e2e.sh —— 目标验收④「派生索引可全量重建」的端到端门禁。
#
# 判定（每门独立证据，不只看返回码）：
#   R1 dry-run 计划：列出在档人 + 派生文件清单，且**不删任何东西**（干跑后 medme.db 仍在）
#   R2 重建后计数不丢：timeline/labs 点数 与重建前**逐字相等**（重建前先量）
#   R3 FTS 热重建：重建后中文检索仍有命中（索引重建成功，不只是表建出来）
#   R4 真相层未被动：log/ 与 objects/ 目录仍在、CAS 原件 sha256 不变（重建只碰派生层）
#   R5 未知参数即拒（--rebuild-index=typo 之类的脏用法必须非零退出）
#   R6 重建后 HTTP 读侧仍一致：服务起来后 timeline/search/labs 计数与重建后相同
#
# 用法: rebuild-e2e.sh <abs-binary-path> [port]
set -u -o pipefail; umask 077
ROOT="$(cd "$(dirname "$0")" && pwd)"
# 产物路径的**唯一来源**（ci/env.sh）——本脚本原先自己拼 $HOME/medme-shared-target/...
. "$ROOT/ci/env.sh"
BIN="${1:-$EHR_REL_BIN}"; PORT="${2:-8961}"   # 缺省取唯一来源（ci/env.sh）
API="http://127.0.0.1:${PORT}/api/v1"
H=$(mktemp -d /tmp/ehrrb.XXXXXX) || { echo FATAL-TMPDIR; exit 3; }
WORK=$(mktemp -d /tmp/ehrrbw.XXXXXX) || { echo FATAL-TMPDIR; exit 3; }
fails=0; SRV_PID=""
g(){ if [[ "$2" == y ]]; then echo "[GATE PASS] $1"; else fails=$((fails+1)); echo "[GATE FAIL] $1${3:+ | $3}"; fi; }
cleanup(){ [[ -n "$SRV_PID" ]] && kill "$SRV_PID" 2>/dev/null; wait "$SRV_PID" 2>/dev/null; rm -rf "$H" "$WORK"; }
trap cleanup EXIT

test -x "$BIN" || { echo "GATE-FATAL-BIN $BIN"; exit 4; }
if timeout 2 bash -c "exec 3<>/dev/tcp/127.0.0.1/${PORT}" 2>/dev/null; then exec 3<&- 3>&-; g "R0 port ${PORT} free before run" n "stale listener?"; exit 6; else g "R0 port ${PORT} free before run" y; fi

# ---- 造数据：建档 + 上传两份含中文/数值的化验单（与 ehtest 同族样本）----
cat >"$WORK/a.txt" <<'EOF'
XX医院检验科检验报告单
姓名:林宝妹  性别:女  年龄:37岁   样本编号:20251015-A042
检验项目          结果      参考区间     标志
白细胞计数        6.2       3.5--9.5     /
血红蛋白          118       115--150     /
糖化血红蛋白      7.1       4.0--6.0     H
报告日期:2025年10月15日
EOF
cat >"$WORK/b.txt" <<'EOF'
YY社区体检中心生化筛查
姓名:林宝妹  性别:女   样本编号:B20260302-07
检验项目          结果      参考区间     标志
白细胞计数        5.1       3.5--9.5     /
低密度脂蛋白胆固醇 3.9      0--3.4       ↑
采集时间:2026/03/02
EOF

EHR_HOME="$H" "$BIN" --addr="127.0.0.1:${PORT}" >"$WORK/serve.log" 2>&1 & SRV_PID=$!
for i in $(seq 1 40); do curl -fsS "$API/healthz" >/dev/null 2>&1 && break; sleep 0.25; done
ok=n; for i in $(seq 1 20); do curl --max-time 5 -fsS "$API/healthz" | grep -q '"status":"ok"' && { ok=y; break; }; sleep 0.5; done
g "R0b server up at /api/v1/healthz" "$ok" "$(head -3 "$WORK/serve.log")"
[[ "$ok" == y ]] || { head -20 "$WORK/serve.log"; exit 7; }

pid=$(curl -fsS -X POST "$API/persons" -H 'content-type: application/json' -d '{"name":"林宝妹","gender":"F","birthday":"1988-05-02"}' | python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])')
[[ "$pid" =~ ^[0-9a-f]{8}-[0-9a-f]{4}-4 ]] && ok=y||ok=n; g "R0c person created ($pid)" "$ok"
for f in a b; do curl --max-time 30 -fsS -X POST "$API/persons/$pid/documents" -F "file=@$WORK/$f.txt" -o "$WORK/up_$f.json" || true; done
grep -q '"text_indexed":true' "$WORK/up_a.json" && grep -q '"text_indexed":true' "$WORK/up_b.json" && ok=y||ok=n; g "R0d both docs ingested+indexed" "$ok"

# ---- 重建前基线（由 HTTP 读侧量，与重建后逐字比对）----
BASE_TL=$(curl -fsS "$API/persons/$pid/timeline" | python3 -c 'import json,sys; d=json.load(sys.stdin); print(len(d) if isinstance(d,list) else len(d.get("items",d.get("timeline",[]))))')
BASE_LABS=$(curl -fsS "$API/persons/$pid/labs" | python3 -c 'import json,sys; d=json.load(sys.stdin); print(d["series_count"],d["total_points"])')
BASE_HITS=$(curl -fsS --get --data-urlencode "q=白细胞" "$API/persons/$pid/search" | python3 -c 'import json,sys; d=json.load(sys.stdin); print(len(d) if isinstance(d,list) else len(d.get("items",[])))')
g "R0e baseline measured (timeline=$BASE_TL labs=$BASE_LABS search_hits=$BASE_HITS)" y
[[ "$BASE_TL" -ge 2 && "$BASE_HITS" -ge 1 ]] && ok=y||ok=n; g "R0f baseline sane(timeline>=2, search>=1)" "$ok" "tl=$BASE_TL hits=$BASE_HITS"

# ---- R1: dry-run 计划（不删东西）----
kill "$SRV_PID" 2>/dev/null; wait "$SRV_PID" 2>/dev/null; SRV_PID=""
DB="$H/vaults/$pid/medme.db"
[[ -f "$DB" ]] && ok=y||ok=n; g "R1a derived db exists before dry-run" "$ok" "$DB"
plan=$(EHR_HOME="$H" "$BIN" --rebuild-index=dry-run 2>"$WORK/plan.err"); prc=$?
[[ $prc -eq 0 ]] && ok=y||ok=n; g "R1b dry-run exit=0" "$ok" "rc=$prc"
echo "$plan" | python3 -c 'import json,sys; d=json.load(sys.stdin); assert len(d)>=1, d; assert d[0]["person_id"], d; assert d[0]["truth_present"] is True, d; assert any("medme.db" in f for f in d[0]["derived_files_present"]), d' \
  && ok=y||ok=n; g "R1c dry-run plan lists vault+derived files+truth_present" "$ok"
[[ -f "$DB" ]] && ok=y||ok=n; g "R1d dry-run did NOT delete derived db" "$ok" "db gone after dry-run?!"
grep -q "mode=rebuild(dry-run)" "$WORK/plan.err" && ok=y||ok=n; g "R1e evidence line shows mode=rebuild(dry-run)" "$ok" "$(head -2 "$WORK/plan.err")"

# ---- R2/R3/R4: 真重建 ----
out=$(EHR_HOME="$H" "$BIN" --rebuild-index 2>"$WORK/rb.err"); rrc=$?
[[ $rrc -eq 0 ]] && ok=y||ok=n; g "R2a rebuild exit=0" "$ok" "rc=$rrc"
echo "$out" | python3 -c 'import json,sys; d=json.load(sys.stdin); assert len(d)>=1, d; assert d[0]["derived_db_removed"] is True, d; assert d[0]["rebuilt_from_log"] is True, d; assert d[0]["timeline_entries"]>=2, d; assert d[0]["search_probe_hits"]>=1, d; print("tl=%d series=%d pts=%d hits=%d"%(d[0]["timeline_entries"],d[0]["lab_series"],d[0]["lab_points"],d[0]["search_probe_hits"]), file=sys.stderr)' \
  && ok=y||ok=n; g "R2b rebuild report: db removed + rebuilt_from_log + counts>0" "$ok" "$out"
grep -qE "mode=rebuild .*timeline_entries=[0-9]+" "$WORK/rb.err" && ok=y||ok=n; g "R2c evidence line mode=rebuild with counts" "$ok" "$(head -2 "$WORK/rb.err")"
NEW_TL=$(echo "$out" | python3 -c 'import json,sys; print(json.load(sys.stdin)[0]["timeline_entries"])')
NEW_HITS=$(echo "$out" | python3 -c 'import json,sys; print(json.load(sys.stdin)[0]["search_probe_hits"])')
[[ "$NEW_TL" == "$BASE_TL" ]] && ok=y||ok=n; g "R2d timeline entries equal before/after rebuild ($NEW_TL vs $BASE_TL)" "$ok"
[[ "$NEW_HITS" -ge 1 ]] && ok=y||ok=n; g "R3a FTS hot after rebuild (probe hits=$NEW_HITS)" "$ok"

# R4: 真相层未被动 —— log/objects 在、CAS 原件字节 sha 不变
[[ -d "$H/vaults/$pid/log" && -d "$H/vaults/$pid/objects" ]] && ok=y||ok=n; g "R4a truth layer (log/ + objects/) intact after rebuild" "$ok"
CAS_SHA=$(find "$H/vaults/$pid/objects" -type f | sort | head -1 | xargs -r sha256sum | cut -d' ' -f1)
[[ -n "$CAS_SHA" ]] && ok=y||ok=n; g "R4b CAS object present & hashable" "$ok" "$CAS_SHA"

# ---- R5: 未知/脏参数即拒 ----
EHR_HOME="$H" "$BIN" --rebuild-index=nonsense-arg >/dev/null 2>&1; urc=$?
[[ $urc -ne 0 ]] && ok=y||ok=n; g "R5 unknown --rebuild-index=<garbage> rejected (rc=$urc)" "$ok"

# ---- R6: 重建后 HTTP 读侧一致 ----
EHR_HOME="$H" "$BIN" --addr="127.0.0.1:${PORT}" >"$WORK/serve2.log" 2>&1 & SRV_PID=$!
for i in $(seq 1 40); do curl -fsS "$API/healthz" >/dev/null 2>&1 && break; sleep 0.25; done
ok=n; for i in $(seq 1 20); do curl --max-time 5 -fsS "$API/healthz" | grep -q '"status":"ok"' && { ok=y; break; }; sleep 0.5; done
g "R6a server up again after rebuild" "$ok"
POST_TL=$(curl -fsS "$API/persons/$pid/timeline" | python3 -c 'import json,sys; d=json.load(sys.stdin); print(len(d) if isinstance(d,list) else len(d.get("items",[])))')
POST_LABS=$(curl -fsS "$API/persons/$pid/labs" | python3 -c 'import json,sys; d=json.load(sys.stdin); print(d["series_count"],d["total_points"])')
POST_HITS=$(curl -fsS --get --data-urlencode "q=白细胞" "$API/persons/$pid/search" | python3 -c 'import json,sys; d=json.load(sys.stdin); print(len(d) if isinstance(d,list) else len(d.get("items",[])))')
[[ "$POST_TL" == "$BASE_TL" && "$POST_LABS" == "$BASE_LABS" && "$POST_HITS" == "$BASE_HITS" ]] && ok=y||ok=n \
  && g "R6b HTTP read-side identical before/after rebuild (tl/labs/search)" "$ok" "post=($POST_TL,$POST_LABS,$POST_HITS) base=($BASE_TL,$BASE_LABS,$BASE_HITS)"

echo "== RESULT fails=$fails"
exit $(( fails>0 ? 1 : 0 ))
