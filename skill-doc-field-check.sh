#!/usr/bin/env bash
# medme-ehr · T27「SKILL.md 字段面」门禁：SKILL.md（agent 读到的第一手说明）承诺的字段名，
# 必须与**真进程真实响应**一致；命令面与工具数也必须与实现一致。
# 内层实现见 skill_doc_field_check.py。
#
# 为什么需要它（D-043 未销账①「SKILL.md 的字段描述面仍无门」的正面处置）：
#   D-040 把 rest-contracts.md（REST 面）、D-043 把 tool-contracts.md（MCP 面）纳入了机器核对；
#   但 **SKILL.md** 里承诺的字段名仍是人读文档里的手抄实现细节 ⇒ 实现改了它不会红
#   ⇒ agent 照它取字段 → 取不到 → 数据全空（D-016 家族的病，主角换成 SKILL.md）。
#   既有门 skill-contract-check.sh 只核工具数/路径数/工具名；
#   skill-rest-check.sh 核的是**它自己手写的期望字段表**（又是一份手抄清单，且不来自 SKILL.md）
#   ⇒ SKILL.md 自身的字段承诺至今无门。
#
# 判据（逐命令，不把多个响应拼一起查——D-043 决策②的纪律迁移）：
#   A 逐命令正向：承诺的字段名必须出现在该命令自己的真响应里；
#   B 归属正确：字段若属于别的命令，那一行必须点名那个命令；
#   C 命令面真实（双向）：SKILL.md 提到的命令 helper 真支持，helper 支持的命令 SKILL.md 都提到；
#   D 数量真实：SKILL.md 写的工具数 / 读侧+写侧分栏 == 实现派生（--print-mcp-tools）。
#
# 用法: ./skill-doc-field-check.sh [PORT]   默认 18735
set -uo pipefail
PORT="${1:-18735}"
ROOT="$(cd "$(dirname "$0")" && pwd)"
. "$ROOT/ci/env.sh"
BIN="$EHR_BIN"
SKILL="$ROOT/skills/medme-ehr/SKILL.md"
HELPER="$ROOT/skills/medme-ehr/scripts/ehr.sh"
fails=0
[ -x "$BIN" ] || { echo "missing binary: $BIN"; exit 1; }
[ -f "$SKILL" ] || { echo "missing $SKILL"; exit 1; }
[ -x "$HELPER" ] || { echo "missing helper: $HELPER"; exit 1; }

EHR_BIN="$BIN" python3 "$ROOT/skill_doc_field_check.py" "$PORT" "$SKILL" "$HELPER"
rc=$?
[ $rc -eq 0 ] || fails=$((fails+1))
echo "[证据] skill-doc-field-check.sh fails=$fails"
[ "$fails" -eq 0 ] || exit 1
