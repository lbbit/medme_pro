#!/usr/bin/env bash
# medme-ehr · REST helper（skill 侧唯一调用面）
# 设计约束（见 docs/DECISIONS.md D-018/T3）：
#   - 零依赖：只用 curl + python3(json 美化)，不装任何 agent/SDK；
#   - API base 与 token 全部来自环境变量 ⇒ 升级/换机器不改文件（D4:参数传进去≠生效，故 --help 会回显生效值）；
#   - 输出统一为 JSON（人读友好 + 可被 agent 解析）。
#
# 用法：  ehr.sh <命令> [参数...]
# 环境：
#   EHR_API_BASE  默认 http://127.0.0.1:8585   （= 服务的 /api/v1 根；不写 api/v1 也行，本脚本自动补）
#   EHR_API_TOKEN 可选，设置了就带 Authorization: Bearer
set -uo pipefail

BASE="${EHR_API_BASE:-http://127.0.0.1:8585}"
BASE="${BASE%/}"
case "$BASE" in */api/v1) ;; *) BASE="$BASE/api/v1";; esac
TOKEN="${EHR_API_TOKEN:-}"

usage() {
  cat <<'EOF'
用法: ehr.sh <命令> [参数]
  health                      健康检查
  capabilities                本构建的 ingest 能力清单（能吃哪些文件类型/哪种被拒+怎么开）
  persons                     人员列表
  person-create NAME [--birthday YYYY-MM-DD] [--gender G] [--id-note S]
  timeline PID                某人健康事件时间线
  search PID Q [--limit N]    某人档案全文检索（命中片段带 [高亮]）
  labs PID                    某人检验指标趋势（含 H/L 标记与 docref 证据）
  medications PID             某人用药跨度（药名/剂量/ATC + 逐次提及 docref 证据）
  documents PID               某人文档列表
  measurements PID [--kind K] [--from YYYY-MM-DD] [--to YYYY-MM-DD]
                            某人手动条目投影（自测值/笔记/动作事件）
  self-measure PID --value KEY=VALUE [--value KEY=VALUE] [--unit U] [--taken-at RFC3339]
                            写入一条自测记录（血压给两个 --value）
  note PID TEXT                写入一条纯文本笔记
  profile-event PID --kind K --package P --at YYYY-MM-DD [--payload JSON]
                            写入一条病程动作日志
  detail PID SHA              文档详情（含 OCR 文本层 / extraction）
  extraction PID SHA FILE     回写 LLM 结构化抽取结果（FILE=deid schema v1 JSON）
                            --backend B --model-version M [--mode text|image] [--schema N]
                            字段级 evidence 闸：每条字段须是原文逐字子串；判假的字段被清空/标 unverified
  original PID SHA            下载原件字节（CAS 按 hash 寻址，每次核 sha256）
  token-list                  列出 API 令牌（需 rw）
  token-create NAME [--scope ro|rw]
                              生成新令牌（明文只显示一次；需 rw）
  token-revoke NAME           吊销令牌（即时生效；需 rw）
  upload PID FILE             上传原件（multipart，仅 .txt/.pdf）
  audit [--person PID] [--op OP]
                            审计视图（两层真相层：registry 建档/改档/归档 + vault 导入/删除）
                            --op 过滤：create|update|delete（registry 侧）或 导入|导出|分享|删除（vault 侧）
EOF
}

# curl 统一出口：token 有则加头
_c() {
  if [ -n "$TOKEN" ]; then
    curl -sS -H "Authorization: Bearer $TOKEN" "$@"
  else
    curl -sS "$@"
  fi
}

# 统一 JSON 美化 + HTTP 错误可读化
_pp() {
  python3 -c '
import json,sys
raw=sys.stdin.read()
try:
    d=json.loads(raw)
except Exception:
    print(raw); sys.exit(0)
print(json.dumps(d,ensure_ascii=False,indent=2))
'
}

die() { echo "ehr.sh: $*" >&2; exit 2; }

CMD="${1:-}"; shift || true
case "$CMD" in
  ""|-h|--help|help) usage; exit 0;;
esac

case "$CMD" in
  health)
    _c "$BASE/healthz" | _pp ;;
  capabilities)
    # D-034：能力清单**从实现派生**（不是手抄表）⇒ agent 自己能判断「我的文件能不能传」。
    _c "$BASE/capabilities" | _pp ;;
  persons)
    _c "$BASE/persons" | _pp ;;
  person-create)
    NAME="${1:-}"; [ -n "$NAME" ] || die "person-create 需要姓名"
    shift || true
    BODY="{\"name\":$(python3 -c 'import json,sys;print(json.dumps(sys.argv[1]))' "$NAME")"
    BD=""; GD=""; IN=""
    while [ $# -gt 0 ]; do
      case "$1" in
        --birthday) BD="$2"; shift 2;;
        --gender) GD="$2"; shift 2;;
        --id-note) IN="$2"; shift 2;;
        *) die "未知参数 $1";;
      esac
    done
    # ⚠️ D4 教训：拼 JSON 时每加一个字段必须自己带前导逗号，否则拼出 {"name":"x"},"gender":... 这种非法 JSON。
    [ -n "$BD" ] && BODY="$BODY,\"birthday\":$(python3 -c 'import json,sys;print(json.dumps(sys.argv[1]))' "$BD")"
    [ -n "$GD" ] && BODY="$BODY,\"gender\":$(python3 -c 'import json,sys;print(json.dumps(sys.argv[1]))' "$GD")"
    [ -n "$IN" ] && BODY="$BODY,\"id_note\":$(python3 -c 'import json,sys;print(json.dumps(sys.argv[1]))' "$IN")"
    BODY="$BODY}"
    _c -X POST "$BASE/persons" -H 'content-type: application/json' -d "$BODY" | _pp ;;
  timeline)
    PID="${1:-}"; [ -n "$PID" ] || die "timeline 需要 person_id"
    _c "$BASE/persons/$PID/timeline" | _pp ;;
  search)
    PID="${1:-}"; Q="${2:-}"; [ -n "$PID" ] && [ -n "$Q" ] || die "search 需要 person_id 与关键词"
    shift 2 || true
    LIMIT=20
    if [ "${1:-}" = "--limit" ]; then LIMIT="$2"; fi
    _c -G "$BASE/persons/$PID/search" --data-urlencode "q=$Q" --data-urlencode "limit=$LIMIT" | _pp ;;
  labs)
    PID="${1:-}"; [ -n "$PID" ] || die "labs 需要 person_id"
    _c "$BASE/persons/$PID/labs" | _pp ;;
  medications)
    PID="${1:-}"; [ -n "$PID" ] || die "medications 需要 person_id"
    _c "$BASE/persons/$PID/medications" | _pp ;;
  measurements)
    PID="${1:-}"; [ -n "$PID" ] || die "measurements 需要 person_id"
    shift || true
    Q=()
    while [ $# -gt 0 ]; do
      case "$1" in
        --kind) Q+=(--data-urlencode "kind=$2"); shift 2;;
        --from) Q+=(--data-urlencode "from=$2"); shift 2;;
        --to)   Q+=(--data-urlencode "to=$2");   shift 2;;
        *) die "未知参数 $1";;
      esac
    done
    _c -G "$BASE/persons/$PID/measurements" ${Q[@]+"${Q[@]}"} | _pp ;;
  self-measure)
    PID="${1:-}"; [ -n "$PID" ] || die "self-measure 需要 person_id"
    shift || true
    # 收集 KEY=VALUE 对（--value 可多次），单位用 --unit 统一给（可省略 ⇒ 服务端必填，
    # 这里不替用户猜单位：宁少不臆造）。
    PAIRS=(); UNIT=""; TAKEN=""
    while [ $# -gt 0 ]; do
      case "$1" in
        --value)
          KV="${2:-}"; [ -n "$KV" ] || die "--value 需要 KEY=VALUE"
          case "$KV" in *=*) ;; *) die "--value 形状应为 KEY=VALUE";; esac
          PAIRS+=("$KV"); shift 2;;
        --unit) UNIT="${2:-}"; shift 2;;
        --taken-at) TAKEN="${2:-}"; shift 2;;
        *) die "未知参数 $1";;
      esac
    done
    [ ${#PAIRS[@]} -gt 0 ] || die "self-measure 至少要一个 --value KEY=VALUE"
    [ -n "$UNIT" ] || die "self-measure 需要 --unit（不替用户猜单位）"
    BODY=$(python3 -c '
import json,sys
pairs=sys.argv[1].split("\t")
unit=sys.argv[2]; taken=sys.argv[3]
vals=[]
for kv in pairs:
    if not kv.strip(): continue
    k,v=kv.split("=",1)
    vals.append({"analyte_key":k,"value":float(v),"unit":unit})
b={"values":vals}
if taken: b["taken_at"]=taken
print(json.dumps(b,ensure_ascii=False))
' "$(printf '%s\t' "${PAIRS[@]}")" "$UNIT" "$TAKEN")
    _c -X POST "$BASE/persons/$PID/entries/self-measurement" -H 'content-type: application/json' -d "$BODY" | _pp ;;
  note)
    PID="${1:-}"; TEXT="${2:-}"
    [ -n "$PID" ] && [ -n "$TEXT" ] || die "note 需要 person_id 与正文"
    BODY="{\"text\":$(python3 -c 'import json,sys;print(json.dumps(sys.argv[1]))' "$TEXT")}"
    _c -X POST "$BASE/persons/$PID/entries/note" -H 'content-type: application/json' -d "$BODY" | _pp ;;
  profile-event)
    PID="${1:-}"; [ -n "$PID" ] || die "profile-event 需要 person_id"
    shift || true
    KIND=""; PKG=""; AT=""; PAYLOAD='{}'
    while [ $# -gt 0 ]; do
      case "$1" in
        --kind) KIND="${2:-}"; shift 2;;
        --package) PKG="${2:-}"; shift 2;;
        --at) AT="${2:-}"; shift 2;;
        --payload) PAYLOAD="${2:-}"; shift 2;;
        *) die "未知参数 $1";;
      esac
    done
    [ -n "$KIND" ] && [ -n "$PKG" ] && [ -n "$AT" ] || die "profile-event 需要 --kind/--package/--at"
    BODY=$(python3 -c 'import json,sys;print(json.dumps({"kind":sys.argv[1],"package":sys.argv[2],"at":sys.argv[3],"payload":json.loads(sys.argv[4])},ensure_ascii=False))' "$KIND" "$PKG" "$AT" "$PAYLOAD")
    _c -X POST "$BASE/persons/$PID/entries/profile-event" -H 'content-type: application/json' -d "$BODY" | _pp ;;
  documents)
    PID="${1:-}"; [ -n "$PID" ] || die "documents 需要 person_id"
    _c "$BASE/persons/$PID/documents" | _pp ;;
  detail)
    PID="${1:-}"; SHA="${2:-}"; [ -n "$PID" ] && [ -n "$SHA" ] || die "detail 需要 person_id 与 sha256"
    _c "$BASE/persons/$PID/documents/$SHA" | _pp ;;
  extraction)
    PID="${1:-}"; SHA="${2:-}"; FILE="${3:-}"
    [ -n "$PID" ] && [ -n "$SHA" ] && [ -n "$FILE" ] || die "extraction 需要 person_id、sha256 与 JSON 文件路径"
    [ -f "$FILE" ] || die "文件不存在: $FILE"
    shift 3 || true
    BACKEND="llm-http"; MODEL="unknown"; MODE="text"; SCHEMA=""
    while [ $# -gt 0 ]; do
      case "$1" in
        --backend) BACKEND="$2"; shift 2;;
        --model-version) MODEL="$2"; shift 2;;
        --mode) MODE="$2"; shift 2;;
        --schema) SCHEMA="$2"; shift 2;;
        *) die "未知参数 $1";;
      esac
    done
    # result_json 是一段 JSON **字符串**（不是嵌套对象）：服务端按 deid schema 原文解析。
    # ⚠️ mode 必填且只认 text|image：拼错服务端会 400（不静默换校验口径）。
    BODY=$(python3 -c '
import json,sys
raw=open(sys.argv[1],encoding="utf-8").read()
json.loads(raw)  # 本地先校一次形状：非法 JSON 别浪费一次往返
b={"result_json":raw,"backend":sys.argv[2],"model_version":sys.argv[3],"mode":sys.argv[4]}
if sys.argv[5]: b["schema"]=int(sys.argv[5])
print(json.dumps(b,ensure_ascii=False))
' "$FILE" "$BACKEND" "$MODEL" "$MODE" "$SCHEMA")
    _c -X PUT "$BASE/persons/$PID/documents/$SHA/extraction" -H 'content-type: application/json' -d "$BODY" | _pp ;;
  original)
    PID="${1:-}"; SHA="${2:-}"; [ -n "$PID" ] && [ -n "$SHA" ] || die "original 需要 person_id 与 sha256"
    _c "$BASE/persons/$PID/documents/$SHA/original" ;;
  token-list)
    _c "$BASE/tokens" | _pp ;;
  token-create)
    # 参数形态: token-create NAME [--scope ro|rw]（支持任意顺序）
    ARGS=""
    while [ $# -gt 0 ]; do
      case "$1" in
        --scope) SCOPE_ARG="$2"; shift 2;;
        *) ARGS="$ARGS $1"; shift;;
      esac
    done
    set -- $ARGS
    NAME="${1:-}"; SCOPE="${SCOPE_ARG:-rw}"; [ -n "$NAME" ] || die "token-create 需要 name（可选 scope: ro|rw，默认 rw）"
    _c -X POST "$BASE/tokens" -H 'content-type: application/json' -d "{\"name\":\"$NAME\",\"scope\":\"$SCOPE\"}" | _pp
    echo "明文只显示这一次，请立即复制保存" >&2 ;;
  token-revoke)
    NAME="${1:-}"; [ -n "$NAME" ] || die "token-revoke 需要 name"
    _c -X DELETE "$BASE/tokens/$NAME" | _pp ;;
  upload)
    PID="${1:-}"; FILE="${2:-}"; [ -n "$PID" ] && [ -n "$FILE" ] || die "upload 需要 person_id 与文件路径"
    [ -f "$FILE" ] || die "文件不存在: $FILE"
    _c -X POST "$BASE/persons/$PID/documents" -F "file=@$FILE" | _pp ;;
  audit)
    # ⚠️ 外层 case 已 shift 过 CMD ⇒ 这里**不能再 shift**（否则会吃掉第一个参数）。
    Q=()
    while [ $# -gt 0 ]; do
      case "$1" in
        --person) Q+=(--data-urlencode "person=$2"); shift 2;;
        --op)     Q+=(--data-urlencode "op=$2");     shift 2;;
        *) die "未知参数 $1";;
      esac
    done
    _c -G "$BASE/audit" ${Q[@]+"${Q[@]}"} | _pp ;;
  *)
    usage; die "未知命令 $CMD";;
esac
