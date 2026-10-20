#!/usr/bin/env bash
# T3 门禁②：真·第三方 agent（Copilot CLI）用 SKILL 完成一次真实查档。
# 关键点：agent 侧只能看到 SKILL.md + references + scripts（无我们的上下文），
# 只能靠 EHR_API_BASE 环境变量找到服务 —— 验证「零维护接入」真的成立。
set -uo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"
WORK=/tmp/medme-ehr-agent-test
rm -rf "$WORK"; mkdir -p "$WORK/.agents/skills"
cp -r "$ROOT/skills/medme-ehr" "$WORK/.agents/skills/"
cd "$WORK"
export EHR_API_BASE="${EHR_API_BASE:-http://127.0.0.1:8585}"
export PATH="$WORK/.agents/skills/medme-ehr/scripts:$PATH"
source ~/qwen3.8-27b-project/scripts/copilot-strata.sh >/dev/null 2>&1
PROMPT='使用 medme-ehr skill 回答：列出档案里都有哪些人，并找出「一键部署验证」这个人的白细胞检验值，只输出结论和数值来源(docref)。不要修改任何文件。'
OUT=$(timeout 300 copilot -p "$PROMPT" --allow-all-tools 2>&1)
echo "$OUT" | tail -40
echo "=== 判定 ==="
echo "$OUT" | grep -q "一键部署验证" && echo "PASS agent 找到目标人" || echo "FAIL agent 未找到目标人"
echo "$OUT" | grep -qE "13\.5" && echo "PASS agent 报出数值 13.5" || echo "FAIL agent 未报出数值"
echo "$OUT" | grep -qE "docref|[0-9a-f]{64}" && echo "PASS agent 给出证据来源" || echo "FAIL agent 未给证据来源"
