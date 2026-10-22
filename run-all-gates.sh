#!/usr/bin/env bash
# medme-ehr · T25「一键跑全部门禁」统一入口 —— 漏跑一个脚本 = 那道门不存在。
#
# 为什么需要它（D-041 未销账② / PLAN 下一步②）：
#   仓库里已有 16 个门禁脚本（artifact-freshness / doc-contract / api-doc / skill-contract /
#   skill-rest / mcp / auth / entries / conds / meds / audit / ehtest / rebuild / rebuild-lock /
#   web-e2e / container-smoke / agent-e2e），**全靠人工逐个跑**。
#   ⇒ 一次「我跑了全部」无法证明 ⇒ 漏跑一个 = 那道门这一轮根本不存在。
#   ⇒ 人工清单本身也会漂移：新增一个门禁脚本却忘了加进清单 ⇒ 新门永远不跑（**还是没人知道**）。
#
# 判据（缺一即算不通过）：
#   A1 **登记表完整性**：仓库根目录每个 *.sh 门禁脚本，必须要么在本文件的 RUN 清单里、
#      要么在 SKIP 清单里并写明**为什么跳过** ⇒ 新增门禁脚本忘了登记 ⇒ 立刻红。
#      （这是 D-037「清单/注释会骗人」的正面处置：登记表由**文件系统派生**核对，不是靠人记得。）
#   A2 **每道门真跑且真绿**：逐个执行，退出码必须为 0，且输出里的 `fails=N` 计数必须为 0
#      （两个都要看：有的脚本 exit 非零但不改计数，有的脚本计数非零却仍 exit 0）。
#   A3 **汇总可见**：打印每个门禁的 fails 与总 fails，任一红 ⇒ 整体 exit 非零。
#
# 用法: ./run-all-gates.sh              # 跑全部（默认）
#       ./run-all-gates.sh --list       # 只打印登记表，不执行（dry-run）
#       ./run-all-gates.sh --only NAME  # 只跑一个（含 A1 完整性核对）
set -uo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT"
export PATH="$HOME/.cargo/bin:$PATH"
# 产物路径的**唯一来源**（ci/env.sh）——各门禁脚本自己也 source 它 ⇒ 同一事实一份实现。
# 注意：本入口只 export CARGO_TARGET_DIR（供 ehtest/rebuild 等需要显式路径的门），
# 门禁脚本内部一律用 $EHR_BIN / $EHR_DBG_BIN，不再自己拼路径。
. "$ROOT/ci/env.sh"
REL_BIN="$EHR_REL_BIN"

# ---- RUN 清单：默认参与统一入口的门禁（顺序 = 依赖顺序：产物/文档面在前，e2e 在后）----
RUN_LIST=(
  artifact-freshness-check.sh   # T24 产物新鲜度（先跑：后面所有门都依赖这份产物）
  ci-portability-check.sh       # T28 CI 可移植性（不许写死机器专属路径/产物路径单一来源）
  doc-contract-check.sh         # T23 文档字段面
  api-doc-check.sh              # D-025 docs/API.md 漂移
  skill-contract-check.sh       # D-018 SKILL/MCP 工具契约
  mcp-contract-check.sh         # T26 MCP 工具契约字段面（tool-contracts.md vs 真 MCP 响应）
  skill-doc-field-check.sh      # T27 SKILL.md 字段承诺面（逐命令 vs 真响应）
  skill-rest-check.sh           # T3 SKILL REST 承诺
  mcp-e2e.sh                    # D-017 MCP e2e
  mcp-resources-e2e.sh          # T34 MCP resources 面（跨面不变量：resources/read == tools/call）
  mcp-interop-e2e.sh            # T35 真·第三方 MCP 客户端互通（官方 MCP Python SDK 当客户端）
  mcp-interop-ts-e2e.sh         # T36 真·第三方 MCP 客户端互通（官方 MCP TypeScript SDK 当客户端）
  auth-e2e.sh                   # T4 鉴权 e2e
  entries-e2e.sh                # D-028 写侧条目
  extraction-e2e.sh             # T31 体检报告列布局 labs=0（抽取路径能救列布局；零外部依赖）
  conds-e2e.sh                  # D-027 病程泳道
  meds-e2e.sh                   # D-026 用药
  audit-e2e.sh                  # D-030 审计
  ehtest.sh                     # 端到端总验收
  registry-concurrency-e2e.sh   # T38 person registry 并发写完整性（真相层被打坏 ⇒ 读面永久 500）
  write-multiround-e2e.sh      # T40 写侧「多轮」：多轮累积/重复上传幂等/重建还原/MCP 面同源与并发
  rebuild-e2e.sh                # 目标验收④ 派生索引可重建
  rebuild-lock-e2e.sh           # D-024 重建期互斥
  web-e2e.sh                    # D-016 Web 管理端
  web-browser-e2e.sh            # T41 Web 管理端「浏览器级冒烟」（真 Chromium：DOM 渲染/事件绑定）
  container-smoke.sh            # D-050 容器化 e2e（本轮起进 RUN：镜像已重建为当前 HEAD）
)

# ---- SKIP 清单：明确知道「本入口不跑」的脚本 + 原因（原因不得为空）----
SKIP_LIST=(
  "agent-e2e.sh|需要真·第三方 agent CLI（copilot）+ 外部服务态，例行入口不跑"
)

# ---- A1 登记表完整性核对：文件系统派生 vs 本文件的清单 ----
check_registry() {
  local bad=0
  local f name
  for f in *.sh; do
    [ "$f" = "run-all-gates.sh" ] && continue
    name=""
    for name in "${RUN_LIST[@]}"; do [ "$name" = "$f" ] && break; done
    if [ "$name" = "$f" ]; then continue; fi
    # 不在 RUN ⇒ 必须在 SKIP 且理由非空
    local reason found=""
    local entry
    for entry in "${SKIP_LIST[@]}"; do
      [ "${entry%%|*}" = "$f" ] && { found="${entry#*|}"; break; }
    done
    if [ -z "$found" ]; then
      echo "[FAIL] A1 门禁脚本未登记：$f ⇒ 既不在 RUN 清单也不在 SKIP 清单 ⇒ 这道门这一轮不会被跑"
      bad=$((bad+1))
    elif [ -z "${found// /}" ]; then
      echo "[FAIL] A1 门禁脚本 $f 在 SKIP 清单里但没写跳过原因"
      bad=$((bad+1))
    fi
  done
  # RUN 清单里指向不存在的文件 ⇒ 静默跳过 = 假绿
  for name in "${RUN_LIST[@]}"; do
    [ -f "$name" ] || { echo "[FAIL] A1 RUN 清单里的 $name 不存在"; bad=$((bad+1)); }
  done
  return $((bad > 0 ? 1 : 0))
}

if [ "${1:-}" = "--list" ]; then
  echo "== RUN 清单 =="
  for n in "${RUN_LIST[@]}"; do echo "  $n"; done
  echo "== SKIP 清单（带原因） =="
  for e in "${SKIP_LIST[@]}"; do echo "  $e"; done
  check_registry && echo "[证据] A1 登记表完整" || exit 1
  exit 0
fi

ONLY="${2:-}"
if [ "${1:-}" = "--only" ]; then ONLY="$2"; fi

fails=0
echo "== A1 登记表完整性 =="
if check_registry; then echo "[PASS] A1 登记表完整（RUN=${#RUN_LIST[@]} SKIP=${#SKIP_LIST[@]}）"; else fails=$((fails+1)); fi

run_one() {
  local script="$1"; shift
  local log out rc cnt
  log="$(mktemp /tmp/medme-gate-XXXXXX.log)"
  echo "---- $script $* "
  ( cd "$ROOT" && bash "./$script" "$@" ) >"$log" 2>&1
  rc=$?
  cnt="$(grep -oE 'fails=[0-9]+' "$log" | tail -n 1 | cut -d= -f2)"
  out="$(grep -E 'FAIL|失败' "$log" | head -n 5)"
  if [ "$rc" -ne 0 ]; then
    echo "[FAIL] $script 退出码=$rc ${cnt:+fails=$cnt}"
    [ -n "$out" ] && echo "$out" | sed 's/^/        /'
    fails=$((fails+1))
  elif [ -n "$cnt" ] && [ "$cnt" -ne 0 ]; then
    echo "[FAIL] $script 退出码=0 但 fails=$cnt ≠ 0"
    [ -n "$out" ] && echo "$out" | sed 's/^/        /'
    fails=$((fails+1))
  else
    echo "[PASS] $script ${cnt:+fails=$cnt}"
  fi
  rm -f "$log"
}

echo "== 逐门执行 =="
for script in "${RUN_LIST[@]}"; do
  if [ -n "$ONLY" ] && [ "$script" != "$ONLY" ]; then continue; fi
  case "$script" in
    ehtest.sh)            run_one "$script" "$REL_BIN" ;;
    rebuild-e2e.sh)       run_one "$script" "$REL_BIN" ;;
    rebuild-lock-e2e.sh)  run_one "$script" "$REL_BIN" ;;
    web-e2e.sh)           run_one "$script" ;;
    *)                    run_one "$script" ;;
  esac
done

echo "== 汇总 =="
ran=0
for s in "${RUN_LIST[@]}"; do
  if [ -z "$ONLY" ] || [ "$s" = "$ONLY" ]; then ran=$((ran+1)); fi
done
echo "[证据] run-all-gates.sh 门禁数=${#RUN_LIST[@]}（本次执行 $ran）总 fails=$fails"
[ "$fails" -eq 0 ] || exit 1
exit 0
