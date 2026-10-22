#!/usr/bin/env bash
# medme-ehr · T26「MCP 工具契约字段面」门禁：tool-contracts.md 的字段承诺必须与**真 MCP 响应**一致。
# 内层实现见 mcp_contract_check.py。
#
# 为什么需要它（D-041 未销账①「SKILL.md / tool-contracts.md 的字段面仍无门」的正面处置）：
#   D-040 把 **REST 面**（rest-contracts.md）纳入了机器核对；**MCP 面**（tool-contracts.md）仍是
#   人读文档里的**手抄实现细节** ⇒ 实现改了它不会红 ⇒ agent 照文档取字段 → 取不到 → 数据全空
#   （D-016 家族的病，主角换成 MCP 契约文档）。`skill-contract-check.sh` 只核**工具数/路径数/工具名**，
#   **不核字段** ⇒ 这道门是补上那个缺口。
#
# 判据三条独立方向：① 正向（文档承诺的字段真存在）② 反向（真响应的键文档都提及）
#   ③ 工具覆盖（实现派生的每个工具都有文档行）。逐工具核对，**不把多个响应拼一起查**
#   （拼一起查 ⇒ 「A 工具承诺了 B 工具才有的字段」照样绿 ⇒ 门在测错对象）。
#
# 用法: ./mcp-contract-check.sh [PORT]   默认 18734
set -uo pipefail
PORT="${1:-18734}"
ROOT="$(cd "$(dirname "$0")" && pwd)"
. "$ROOT/ci/env.sh"
BIN="$EHR_BIN"
RDOC="$ROOT/skills/medme-ehr/references/tool-contracts.md"
fails=0
[ -x "$BIN" ] || { echo "missing binary: $BIN"; exit 1; }
[ -f "$RDOC" ] || { echo "missing $RDOC"; exit 1; }

python3 "$ROOT/mcp_contract_check.py" "$PORT" "$RDOC" "$BIN"
rc=$?
[ $rc -eq 0 ] || fails=$((fails+1))
echo "[证据] mcp-contract-check.sh fails=$fails"
[ "$fails" -eq 0 ] || exit 1
