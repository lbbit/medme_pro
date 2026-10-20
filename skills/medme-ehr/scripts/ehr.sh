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
  persons                     人员列表
  person-create NAME [--birthday YYYY-MM-DD] [--gender G] [--id-note S]
  timeline PID                某人健康事件时间线
  search PID Q [--limit N]    某人档案全文检索（命中片段带 [高亮]）
  labs PID                    某人检验指标趋势（含 H/L 标记与 docref 证据）
  medications PID             某人用药跨度（药名/剂量/ATC + 逐次提及 docref 证据）
  documents PID               某人文档列表
  detail PID SHA              文档详情（含 OCR 文本层 / extraction）
  original PID SHA            下载原件字节（CAS 按 hash 寻址，每次核 sha256）
  upload PID FILE             上传原件（multipart，仅 .txt/.pdf）
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
  documents)
    PID="${1:-}"; [ -n "$PID" ] || die "documents 需要 person_id"
    _c "$BASE/persons/$PID/documents" | _pp ;;
  detail)
    PID="${1:-}"; SHA="${2:-}"; [ -n "$PID" ] && [ -n "$SHA" ] || die "detail 需要 person_id 与 sha256"
    _c "$BASE/persons/$PID/documents/$SHA" | _pp ;;
  original)
    PID="${1:-}"; SHA="${2:-}"; [ -n "$PID" ] && [ -n "$SHA" ] || die "original 需要 person_id 与 sha256"
    _c "$BASE/persons/$PID/documents/$SHA/original" ;;
  upload)
    PID="${1:-}"; FILE="${2:-}"; [ -n "$PID" ] && [ -n "$FILE" ] || die "upload 需要 person_id 与文件路径"
    [ -f "$FILE" ] || die "文件不存在: $FILE"
    _c -X POST "$BASE/persons/$PID/documents" -F "file=@$FILE" | _pp ;;
  *)
    usage; die "未知命令 $CMD";;
esac
