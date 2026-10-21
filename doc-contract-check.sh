#!/usr/bin/env bash
# medme-ehr · T23「文档面」门禁：rest-contracts.md 的承诺（字段名 / 端点覆盖）必须与**真进程真实响应**一致。
# 内层实现见 doc_contract_check.py。
#
# 为什么需要它（D-039 未销账①「文档面未逐条核」的正面处置）：
#   D-037~D-039 把「stub 清单」「REST 路径数」「响应形状 vs OpenAPI」三处硬编码治成了「实现派生 + 机器核对」，
#   但 **rest-contracts.md 的「返回关键字段」列仍是人读文档里的手抄实现细节**——它抄的是当时的实现形状，
#   实现改了它不会红（没有门）⇒ 它会**悄悄变假**（agent 照文档取字段 → 取不到 → 数据全空，D-016 家族的病）。
#   ⇒ 本门把「文档」也纳入机器核对：**正向**（文档承诺的字段真存在）+ **反向**（真响应的顶层键文档都有）
#   + **端点覆盖**（OpenAPI 的每个 operation 都有文档行）。
#
# 用法: ./doc-contract-check.sh [PORT]   默认 18733
set -uo pipefail
PORT="${1:-18733}"
BIN="${EHR_BIN:-$HOME/medme-shared-target/release/ehr-api}"
ROOT="$(cd "$(dirname "$0")" && pwd)"
RDOC="$ROOT/skills/medme-ehr/references/rest-contracts.md"
fails=0
[ -x "$BIN" ] || { echo "missing binary: $BIN"; exit 1; }
[ -f "$RDOC" ] || { echo "missing $RDOC"; exit 1; }

H="$(mktemp -d)"; mkdir -p "$H/vaults"
EHR_HOME="$H" "$BIN" --addr="127.0.0.1:$PORT" 2>"$H/serve.log" &
SRV=$!
trap 'kill -9 $SRV 2>/dev/null; rm -rf "$H"' EXIT
for i in $(seq 1 40); do curl -sf "127.0.0.1:$PORT/api/v1/healthz" >/dev/null 2>&1 && break; sleep 0.5; done
echo "[证据] server up pid=$SRV"

python3 "$ROOT/doc_contract_check.py" "$PORT" "$RDOC" "$BIN"
rc=$?
[ $rc -eq 0 ] || fails=$((fails+1))
echo "[证据] doc-contract-check.sh fails=$fails"
[ "$fails" -eq 0 ] || exit 1
