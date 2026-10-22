#!/usr/bin/env bash
# medme-ehr · T36「真·第三方 MCP 客户端互通（TypeScript 官方 SDK 面）」门禁。
#
# 为什么需要它（D-052 决策②续作 / PLAN 未销账项②）：
#   T35 只验了**官方 MCP Python SDK**；官方还有 TypeScript 实现
#   （@modelcontextprotocol/sdk —— 本仓库宿主生态就是 TS/Node）。
#   两个实现的解析器不同（TS=zod/v4，Python=pydantic v2）⇒ 只验一个实现
#   ⇒ 不能声称「协议面合规」⇒ 本门与 mcp-interop-e2e.sh（Python 面）**同形对照**。
#
# 依赖与通道（D-004~D-007 配方 / D-016 同形：门禁侧依赖，不进产品面）：
#   官方 TS SDK 是**门禁侧**依赖 ⇒ 装到门禁专用目录（$HOME/.cache/medme-ehr-mcp-ts），
#   **不进仓库**、不进 package.json、不进产品镜像。
#   通道：npmmirror（registry.npmmirror.com）优先，失败退官方 registry.npmjs.org。
#   版本钉死（@modelcontextprotocol/sdk@1.32.1）⇒ 门禁行为可复现。
#
# D-050 纪律：**派生的期望值为空时必须红**（不许空值参与比较）。
#
# 用法: ./mcp-interop-ts-e2e.sh [PORT]   默认 18696
set -uo pipefail
PORT="${1:-18696}"
ROOT="$(cd "$(dirname "$0")" && pwd)"
. "$ROOT/ci/env.sh"
BIN="$EHR_BIN"
fails=0
note() { echo "[证据] $*"; }

MCP_NPM_PKG="@modelcontextprotocol/sdk@1.32.1"
MCP_TS_DIR="${EHR_MCP_TS_DIR:-$HOME/.cache/medme-ehr-mcp-ts}"
NPM_REGISTRY="${EHR_NPM_REGISTRY:-https://registry.npmmirror.com}"
NPM_FALLBACK_REGISTRY="${EHR_NPM_FALLBACK_REGISTRY:-https://registry.npmjs.org}"

ensure_sdk() {
  if [ -d "$MCP_TS_DIR/node_modules/@modelcontextprotocol/sdk" ]; then
    note "复用已有门禁 TS 依赖目录: $MCP_TS_DIR"
    return 0
  fi
  note "安装门禁侧依赖 $MCP_NPM_PKG（通道 $NPM_REGISTRY）"
  mkdir -p "$MCP_TS_DIR" || { echo "  FAIL 建目录失败: $MCP_TS_DIR"; return 1; }
  ( cd "$MCP_TS_DIR" && npm init -y >/dev/null 2>&1 ) || { echo "  FAIL npm init 失败"; return 1; }
  if ! ( cd "$MCP_TS_DIR" && npm install --no-audit --no-fund --registry="$NPM_REGISTRY" "$MCP_NPM_PKG" >/dev/null 2>&1 ); then
    note "npmmirror 通道失败，退官方 registry"
    ( cd "$MCP_TS_DIR" && npm install --no-audit --no-fund --registry="$NPM_FALLBACK_REGISTRY" "$MCP_NPM_PKG" >/dev/null 2>&1 ) \
      || { echo "  FAIL 装 $MCP_NPM_PKG 失败"; return 1; }
  fi
  [ -d "$MCP_TS_DIR/node_modules/@modelcontextprotocol/sdk" ] || { echo "  FAIL 装完仍找不到 SDK"; return 1; }
  note "[证据] 门禁侧 SDK 版本 $MCP_NPM_PKG"
}

[ -x "$BIN" ] || { echo "missing binary: $BIN"; exit 1; }
command -v node >/dev/null 2>&1 || { echo "  FAIL 缺 node 运行时（TS 面门禁需要）"; exit 1; }
command -v npm >/dev/null 2>&1 || { echo "  FAIL 缺 npm（TS 面门禁需要）"; exit 1; }
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

# 探针放在门禁依赖目录里跑（ESM 依赖解析需要 node_modules 在其上级目录）⇒ 仓库不留 node_modules
cp "$ROOT/tools/mcp_interop_ts_probe.mjs" "$MCP_TS_DIR/mcp_interop_ts_probe.mjs"
MCP_BASE="http://127.0.0.1:$PORT" \
MCP_TOOLS_EXPECTED="$EXP_TOOLS" \
MCP_RESOURCES_EXPECTED="$EXP_RES" \
MCP_TEMPLATES_EXPECTED="$EXP_TPL" \
  node "$MCP_TS_DIR/mcp_interop_ts_probe.mjs"
RC=$?
[ "$RC" -ne 0 ] && fails=$((fails+1))

echo "[证据] mcp-interop-ts-e2e.sh fails=$fails"
exit $([ "$fails" -eq 0 ] && echo 0 || echo 1)
