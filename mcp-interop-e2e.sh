#!/usr/bin/env bash
# medme-ehr · T35「真·第三方 MCP 客户端互通」门禁（官方 MCP Python SDK 当第三方客户端）。
#
# 为什么需要它（PLAN 未销账项③「真·第三方 MCP 客户端互通仍未验（含 resources 面）」）：
#   本仓库自己的 mcp-e2e.sh / mcp-resources-e2e.sh 都是**自己写 curl**调自己的服务
#   ⇒ 只能证明「本实现自洽」，证不了「**官方 SDK 能不能吃**」。
#   ⇒ 一个只被自家 curl 门覆盖的协议面，可能整族不合规范而门禁全绿（T35 实测就抓到
#      resources/read 的失败形状不合规范 ⇒ 官方 SDK 抛 ValidationError 崩溃）。
#
# 判据（全部在 tools/mcp_interop_probe.py 里，逐条打印 [PASS]/[FAIL]，此处只汇总）：
#   I1 握手可解析（官方 pydantic 模型）  I2 工具数 == 干跑清单派生
#   I3 tools/call 真跑通（自己造人）    I4 资源/模板数 == 干跑清单派生
#   I5 跨面不变量在第三方客户端上复现（read_resource == call_tool 逐字相等）
#   I6 错误语义**可判读**（McpError + 错误码；不是 ValidationError 崩溃）
#
# 依赖与通道（D-004~D-007 配方）：
#   官方 `mcp` Python 包不在本仓库依赖面内（构建期零外网是本项目的不变量，D-016 同形）
#   ⇒ 它是**门禁侧**依赖，不是产品依赖 ⇒ 按需装到**门禁专用 venv**里（不进仓库）。
#   通道：TUNA pypi（https://pypi.tuna.tsinghua.edu.cn/simple）优先，失败退官方 pypi。
#   venv 路径由环境变量 EHR_MCP_VENV 覆盖；默认放在 $HOME 下（不进仓库、不污染）。
#
# D-050 纪律：**派生的期望值为空时必须红**（不许空值参与比较）。
#
# 用法: ./mcp-interop-e2e.sh [PORT]   默认 18695
set -uo pipefail
PORT="${1:-18695}"
ROOT="$(cd "$(dirname "$0")" && pwd)"
. "$ROOT/ci/env.sh"
BIN="$EHR_BIN"
fails=0
note() { echo "[证据] $*"; }

# ---- 门禁侧依赖：官方 mcp SDK（版本钉死 ⇒ 门禁行为可复现）----
MCP_PKG="mcp==1.30.0"
VENV="${EHR_MCP_VENV:-$HOME/.cache/medme-ehr-mcp-venv}"
PIP_INDEX="${EHR_PIP_INDEX:-https://pypi.tuna.tsinghua.edu.cn/simple}"
PIP_TRUSTED="${EHR_PIP_TRUSTED_HOST:-pypi.tuna.tsinghua.edu.cn}"
PY="$VENV/bin/python"

ensure_sdk() {
  if [ -x "$PY" ] && "$PY" -c "import mcp" >/dev/null 2>&1; then
    note "复用已有门禁 venv: $VENV"
    return 0
  fi
  note "安装门禁侧依赖 $MCP_PKG（通道 $PIP_INDEX）"
  # ⚠️ 建 venv 可能失败：CI runner 的 python3 常缺 python3-venv（Debian 拆包）
  #   ⇒ 退化到「装到 --target 目录 + PYTHONPATH」，仍不进仓库、不污染系统。
  #   （D-050 家族：**期望的依赖面拿不到时不能直接红**，要有可复现的退化路径。）
  if python3 -m venv "$VENV" >/dev/null 2>&1; then
    if ! "$VENV/bin/pip" install -q -i "$PIP_INDEX" --trusted-host "$PIP_TRUSTED" "$MCP_PKG"; then
      note "TUNA 通道失败，退官方 pypi"
      "$VENV/bin/pip" install -q "$MCP_PKG" || { echo "  FAIL 装 $MCP_PKG 失败"; return 1; }
    fi
    if ! "$PY" -c "import mcp" >/dev/null 2>&1; then
      echo "  FAIL 装完仍 import 不到 mcp"; return 1
    fi
  else
    note "建 venv 失败（多半缺 python3-venv）⇒ 退化到 --target 目录安装"
    TGT="$HOME/.cache/medme-ehr-mcp-pydeps"
    mkdir -p "$TGT" || { echo "  FAIL 建依赖目录失败: $TGT"; return 1; }
    if ! python3 -m pip install -q --target "$TGT" -i "$PIP_INDEX" --trusted-host "$PIP_TRUSTED" "$MCP_PKG"; then
      note "TUNA 通道失败，退官方 pypi"
      python3 -m pip install -q --target "$TGT" "$MCP_PKG" || { echo "  FAIL 装 $MCP_PKG 失败"; return 1; }
    fi
    PY="python3"
    export PYTHONPATH="$TGT${PYTHONPATH:+:$PYTHONPATH}"
    python3 -c "import mcp" || { echo "  FAIL 装完仍 import 不到 mcp"; return 1; }
  fi
  "$PY" -c "import importlib.metadata as m; print('[证据] 门禁侧 SDK 版本 mcp==%s' % m.version('mcp'))" 2>/dev/null || true
}

[ -x "$BIN" ] || { echo "missing binary: $BIN"; exit 1; }
ensure_sdk || exit 1

# ---- 期望值从**干跑清单**派生（不手抄；D-050：为空必须红）----
H="$(mktemp -d)"; mkdir -p "$H/vaults"
MAN_TOOLS=$(EHR_HOME="$H" "$BIN" --print-mcp-tools 2>/dev/null)
MAN_RES=$(EHR_HOME="$H" "$BIN" --print-mcp-resources 2>/dev/null)
EXP_TOOLS=$(echo "$MAN_TOOLS" | python3 -c "import json,sys;print(json.load(sys.stdin)['count'])" 2>/dev/null)
EXP_RES=$(echo "$MAN_RES" | python3 -c "import json,sys;print(json.load(sys.stdin)['count'])" 2>/dev/null)
EXP_TPL=$(echo "$MAN_RES" | python3 -c "import json,sys;print(json.load(sys.stdin)['templates'])" 2>/dev/null)
if [ -z "$EXP_TOOLS" ] || [ -z "$EXP_RES" ] || [ -z "$EXP_TPL" ] \
   || [ "$EXP_TOOLS" = "0" ] || [ "$EXP_RES" = "0" ] || [ "$EXP_TPL" = "0" ]; then
  echo "  FAIL 干跑清单派生的期望值为空(tools=$EXP_TOOLS resources=$EXP_RES templates=$EXP_TPL) ⇒ 门无判别力"
  fails=$((fails+1))
fi
note "派生期望值: tools=$EXP_TOOLS resources=$EXP_RES templates=$EXP_TPL"

# ---- 起真服务（真进程、真实数据、零 mock）----
EHR_HOME="$H" "$BIN" --addr="127.0.0.1:$PORT" 2>"$H/serve.log" &
SRV=$!
trap 'kill $SRV 2>/dev/null; rm -rf "$H"' EXIT
for i in $(seq 1 40); do curl -sf "127.0.0.1:$PORT/healthz" >/dev/null 2>&1 && break; sleep 0.5; done
note "server up pid=$SRV home=$H"

MCP_BASE="http://127.0.0.1:$PORT" \
MCP_TOOLS_EXPECTED="$EXP_TOOLS" \
MCP_RESOURCES_EXPECTED="$EXP_RES" \
MCP_TEMPLATES_EXPECTED="$EXP_TPL" \
  "$PY" "$ROOT/tools/mcp_interop_probe.py"
RC=$?
[ "$RC" -ne 0 ] && fails=$((fails+1))

echo "[证据] mcp-interop-e2e.sh fails=$fails"
exit $([ "$fails" -eq 0 ] && echo 0 || echo 1)
