#!/usr/bin/env bash
# medme-ehr · T3 门禁①：SKILL(REST 版)承诺的每个命令/字段名，必须在**真进程真实响应**里逐个存在。
# 沿用 D-016 教训：字段名对不上时 agent 看着一切正常但数据全空，只测 HTTP 200 是假门。
# 用法: ./skill-rest-check.sh [PORT]   默认 18715
set -uo pipefail
PORT="${1:-18715}"
BIN="${EHR_BIN:-$HOME/medme-shared-target/release/ehr-api}"
ROOT="$(cd "$(dirname "$0")" && pwd)"
E="$ROOT/skills/medme-ehr/scripts/ehr.sh"
SKILL="$ROOT/skills/medme-ehr/SKILL.md"
REFS="$ROOT/skills/medme-ehr/references/rest-contracts.md"
fails=0
gate() { if [ "$2" = "$3" ]; then echo "  PASS $1 = $2"; else echo "  FAIL $1: got=$2 want=$3"; fails=$((fails+1)); fi; }
[ -x "$BIN" ] || { echo "missing binary: $BIN"; exit 1; }
[ -x "$E" ] || { echo "missing helper (not executable): $E"; exit 1; }

H="$(mktemp -d)"; mkdir -p "$H/vaults"
EHR_HOME="$H" "$BIN" --addr="127.0.0.1:$PORT" 2>"$H/serve.log" &
SRV=$!
trap 'kill -9 $SRV 2>/dev/null; rm -rf "$H"' EXIT
for i in $(seq 1 40); do curl -sf "127.0.0.1:$PORT/api/v1/healthz" >/dev/null 2>&1 && break; sleep 0.5; done
export EHR_API_BASE="http://127.0.0.1:$PORT"
echo "[证据] server up pid=$SRV base=$EHR_API_BASE"

printf '姓名：钱八\n检查项目：血常规\n白细胞计数 12.3 10^9/L 偏高\n' > "$H/lab.txt"
cat > "$H/rx.txt" <<'RXEOF'
XX医院门诊处方笺 (prescription)

姓名：钱八    性别：男    科室：心血管内科
处方日期：2025-03-11    诊断：高血压 2 级

Rp.
1. 苯磺酸氨氯地平片 Amlodipine  5mg × 14 片
   用法:口服,一次 1 片,一日 1 次
2. 阿司匹林肠溶片 Aspirin  100mg × 30 片
   用法:口服,一次 1 片,一日 1 次(餐后)

医师：某某
RXEOF

# 0) helper 自带 --help 且不写死地址/token（零维护：全走 env）
grep -q 'EHR_API_BASE' "$E" || { echo "  FAIL helper 未读 EHR_API_BASE"; fails=$((fails+1)); }
grep -q 'EHR_API_TOKEN' "$E" || { echo "  FAIL helper 未读 EHR_API_TOKEN"; fails=$((fails+1)); }
if grep -q 'EHR_API_BASE' "$SKILL"; then echo "  PASS SKILL.md 用 env 变量取 API 地址"; else echo "  FAIL SKILL.md 未指明 EHR_API_BASE"; fails=$((fails+1)); fi
grep -qE 'Bearer [A-Za-z0-9]{8}' "$SKILL" && { echo "  FAIL SKILL.md 写死了 token"; fails=$((fails+1)); } || echo "  PASS SKILL.md 未写死 token"

# 1) 真实走一遍写侧 + 读侧（顺序要紧：先建档再查，否则 persons 为空、search 必然 0 命中 —— D-011「先怀疑测量方法」）
PERSONS_0=$("$E" persons)
PID=$("$E" person-create 门禁甲 --gender male --birthday 1990-01-02 | python3 -c 'import json,sys;print(json.load(sys.stdin)["id"])')
[ -n "$PID" ] && [ "$PID" != "None" ] || { echo "  FAIL person-create 无 id"; fails=$((fails+1)); }
echo "  PASS person-create id=$PID"
PERSONS=$("$E" persons)
echo "$PERSONS" | grep -q '"id"' || { echo "  FAIL persons 无 id"; fails=$((fails+1)); }
UP=$("$E" upload "$PID" "$H/lab.txt")
UP_RX=$("$E" upload "$PID" "$H/rx.txt")
SHA=$(echo "$UP" | python3 -c 'import json,sys;print(json.load(sys.stdin)["content_hash"])')
[ -n "$SHA" ] || { echo "  FAIL upload 无 content_hash"; fails=$((fails+1)); }

ALL="$PERSONS"$'\n'"$("$E" timeline "$PID")"$'\n'"$("$E" search "$PID" 白细胞)"$'\n'"$("$E" labs "$PID")"$'\n'"$("$E" medications "$PID")"$'\n'"$("$E" documents "$PID")"$'\n'"$("$E" detail "$PID" "$SHA")"$'\n'"$UP"
check() { # check <label> <逗号分隔字段>
  local miss=""
  for f in ${2//,/ }; do case "$ALL" in *"\"$f\""*) ;; *) miss="$miss $f";; esac; done
  if [ -z "$miss" ]; then echo "  PASS $1 全部存在($(echo $2 | tr ',' ' ' | wc -w) 个)"; else echo "  FAIL $1 缺字段:$miss"; fails=$((fails+1)); fi
}
check "persons.fields"   "id,name,birthday,gender,id_note,created_at,updated_at"
check "timeline.fields"  "person_id,count,items,document_id,doc_date,doc_type,title"
check "search.fields"    "person_id,q,count,snippet"
check "labs.fields"      "person_id,documents_scanned,series_count,total_points,series,group_name,unit_canonical,any_abnormal,points,date,value,flag,docref"
check "documents.fields" "person_id,count,content_hash,original_name,mime_type,byte_size,doc_type,text_indexed"
check "detail.fields"    "content_hash,original_name,mime_type,byte_size,imported_at,document_id,doc_type,title,language,page_count,text_indexed,text_truncated_bytes"
check "upload.fields"    "content_hash,deduped,document_id,doc_type,text_indexed,stored_raw_only,messages"
check "medications.fields" "person_id,documents_scanned,med_count,mention_count,medications,drug_key,name,raw_names,atc,latest_dose,latest_raw_name,start,end,status,docrefs,unverified,mentions,date,dose,raw_name,docref,origin"

# 2) SKILL.md / rest-contracts.md 里承诺的命令必须都被 helper 支持
for c in health persons person-create timeline search labs medications documents detail original upload measurements self-measure note profile-event audit; do
  grep -q "ehr.sh $c" "$SKILL" || { echo "  FAIL SKILL.md 未提命令 $c"; fails=$((fails+1)); }
  "$E" --help | grep -qE "^  $c( |\$)" || { echo "  FAIL helper 不支持命令 $c"; fails=$((fails+1)); }
done
echo "  PASS 命令面 SKILL ↔ helper 对齐(16 个)"

# 2b) 写侧手动条目（D-028）：helper 三个写命令必须真写进去，读回 measurements
SM=$("$E" self-measure "$PID" --value bp_systolic=128 --value bp_diastolic=82 --unit mmHg --taken-at 2026-10-20T08:30:00+08:00)
gate "self_measure_created" "$(echo "$SM" | python3 -c 'import json,sys;d=json.load(sys.stdin);print(d["doc_type"])')" "self_measurement"
gate "self_measure_not_deduped" "$(echo "$SM" | python3 -c 'import json,sys;print(json.load(sys.stdin)["deduped"])')" "False"
NT=$("$E" note "$PID" "门禁笔记原文")
gate "note_created" "$(echo "$NT" | python3 -c 'import json,sys;d=json.load(sys.stdin);print(d["doc_type"])')" "note"
PE=$("$E" profile-event "$PID" --kind flare --package sle --at 2026-10-19 --payload '{"note":"关节痛"}')
gate "profile_event_created" "$(echo "$PE" | python3 -c 'import json,sys;d=json.load(sys.stdin);print(d["doc_type"])')" "profile_event"
MS=$("$E" measurements "$PID")
gate "measurements_count" "$(echo "$MS" | python3 -c 'import json,sys;print(json.load(sys.stdin)["count"])')" "3"
gate "measurements_note_text_roundtrip" "$(echo "$MS" | python3 -c 'import json,sys;d=json.load(sys.stdin);print(sum(1 for m in d["measurements"] if m.get("text")=="门禁笔记原文"))')" "1"
gate "measurements_has_ref_source" "$(echo "$MS" | python3 -c 'import json,sys;d=json.load(sys.stdin);print(sum(1 for m in d["measurements"] for v in m.get("values",[]) if v.get("ref_source")))')" "2"
gate "measurements_kind_filter" "$("$E" measurements "$PID" --kind note | python3 -c 'import json,sys;print(json.load(sys.stdin)["count"])')" "1"
gate "measurements_bad_kind_400" "$(curl -s -o /dev/null -w '%{http_code}' -G "http://127.0.0.1:$PORT/api/v1/persons/$PID/measurements" --data-urlencode "kind=nonsense")" "400"
gate "measurements_bad_date_400" "$(curl -s -o /dev/null -w '%{http_code}' -G "http://127.0.0.1:$PORT/api/v1/persons/$PID/measurements" --data-urlencode "from=not-a-date")" "400"
gate "self_measure_implausible_400" "$(curl -s -o /dev/null -w '%{http_code}' -X POST "http://127.0.0.1:$PORT/api/v1/persons/$PID/entries/self-measurement" -H 'content-type: application/json' -d '{"values":[{"analyte_key":"bp_systolic","value":138388,"unit":"mmHg"}]}')" "400"

# 2c) 审计视图（D-030）：helper 的 audit 命令真能调，且两层真相层/参数过滤都在
AU=$("$E" audit)
gate "audit_count_positive" "$(echo "$AU" | python3 -c 'import json,sys;print(1 if json.load(sys.stdin)["count"]>0 else 0)')" "1"
gate "audit_has_two_layers" "$(echo "$AU" | python3 -c 'import json,sys;d=json.load(sys.stdin);s={e["source"] for e in d["entries"]};print(1 if {"registry","vault"}<=s else 0)')" "1"
gate "audit_person_filter_narrows" "$("$E" audit --person "$PID" | python3 -c 'import json,sys;d=json.load(sys.stdin);print(1 if d["count"]>0 and all(e["person_id"]=="'"$PID"'" for e in d["entries"]) else 0)')" "1"
gate "audit_op_filter_works" "$("$E" audit --op create | python3 -c 'import json,sys;d=json.load(sys.stdin);print(1 if d["count"]>0 and all(e["op"]=="create" for e in d["entries"]) else 0)')" "1"
gate "audit_unknown_person_zero" "$("$E" audit --person 00000000-0000-4000-8000-000000000000 | python3 -c 'import json,sys;print(json.load(sys.stdin)["count"])')" "0"

# 3) 关键不变量：数值只在 labs 里、证据带 docref、search 0 命中不报错
LABS_JSON=$("$E" labs "$PID")
gate "labs_has_docref" "$(echo "$LABS_JSON" | grep -o '"docref"' | wc -l | tr -d ' ')" "$(echo "$LABS_JSON" | python3 -c 'import json,sys;d=json.load(sys.stdin);print(sum(len(s["points"]) for s in d["series"]))')"
gate "search_hit_count" "$("$E" search "$PID" 白细胞 | python3 -c 'import json,sys;print(json.load(sys.stdin)["count"])')" "1"
gate "search_rare_word_zero" "$("$E" search "$PID" zzqqww | python3 -c 'import json,sys;print(json.load(sys.stdin)["count"])')" "0"
gate "detail_text_present" "$(echo "$ALL" | grep -c '"text":')" "1"
MEDS_JSON=$("$E" medications "$PID")
gate "medications_has_docref" "$(echo "$MEDS_JSON" | grep -o '"docref"' | wc -l | tr -d ' ')" "$(echo "$MEDS_JSON" | python3 -c 'import json,sys;d=json.load(sys.stdin);print(sum(len(m["mentions"]) for m in d["medications"]))')"
gate "medications_status_active" "$("$E" medications "$PID" | python3 -c 'import json,sys;d=json.load(sys.stdin);print(1 if d["med_count"]>0 and all(m["status"]=="active" for m in d["medications"]) else 0)')" "1"

# 4) references 文件存在且被 SKILL 引用
[ -f "$REFS" ] || { echo "  FAIL 缺 rest-contracts.md"; fails=$((fails+1)); }
grep -q 'references/rest-contracts.md' "$SKILL" || { echo "  FAIL SKILL.md 未引用 rest-contracts.md"; fails=$((fails+1)); }

# 5) 零维护：换 base 不用改文件 —— 用另一个 base 指向同一服务，helper 仍工作
EHR_API_BASE="127.0.0.1:$PORT" "$E" health | grep -q '"status": "ok"' || { echo "  FAIL 裸 host:port 形式 base 不工作"; fails=$((fails+1)); }

echo "[证据] skill-rest-check.sh fails=$fails"
[ "$fails" -eq 0 ] || exit 1
