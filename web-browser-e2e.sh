#!/usr/bin/env bash
# medme-ehr · T41「Web 管理端 · 浏览器级冒烟」门禁（真 Chromium + 真页面 + 真 API）。
#
# 为什么需要它（PLAN T30 遗留⑨ / D-058 未销账① 的前置）：
#   已有 Web 门禁 `web-e2e.sh` 只做到 **JS 语法**（node --check）+ 字段名核对
#   ⇒ **DOM 渲染、事件绑定、点击后的真实数据流** 从来没有任何门
#   ⇒ 页面 JS 整体抛异常、「建档按钮点了没反应」、表格渲染成空 —— 现状都是「门禁全绿」。
#   ⇒ 本门是 **T5「Web 管理端正式化」的前置**：没有浏览器级门，重构 Web 就是裸奔。
#
# 判据（在 tools/web_browser_probe.py 里，真浏览器逐条打印）：
#   B1 加载期无未捕获异常/console error  B2 openapi 拉到（#ver 显示 paths=）
#   B3 侧栏建档 ⇒ 人员出现  B4 时间线表格渲染  B5 上传原件 ⇒ 文档表出现
#   B6 点详情 ⇒ sha256 渲染  B7 指标页 series=  B8 自测趋势渲染出数值
#   B9 检索渲染  B10 页面不得渲染出 [ERROR]
#
# 测哪一份产物（D-035 新坑③ 的正面处置）：
#   与其余门一样**自起临时 ehr-api**（跑 $EHR_REL_BIN = 当前产物）⇒ 测的就是这一轮的代码。
#
# 依赖与通道（门禁侧依赖，不是产品依赖 —— D-016 同形）：
#   playwright + chromium。**不进仓库、不进 Cargo.lock、不进产品镜像**。
#   优先复用已装好的环境；没有则按 D-050 家族给**可复现的退化路径**：
#     ① python3 -m venv（可能缺 python3-venv）→ 装 playwright（pypi TUNA 优先，退官方 pypi）
#     ② 退化：`pip install --target` + PYTHONPATH
#     ③ 再退化：用 `EHR_BROWSER_PROBE_CMD` 指定的命令（由调用方/CI 提供）
#   ⚠️ **浏览器二进制**：playwright 需要 chromium。CI 里用 `playwright install chromium` 下载
#     （约 150MB）—— 装不到时本门**红**（不许静默 SKIP，见 D-042 纪律：静默跳过 = 一道不存在的门）。
#
# 用法: ./web-browser-e2e.sh [PORT]   默认 18699
set -uo pipefail
PORT="${1:-18699}"
ROOT="$(cd "$(dirname "$0")" && pwd)"
. "$ROOT/ci/env.sh"
BIN="$EHR_REL_BIN"
fails=0
note() { echo "[证据] $*"; }

[ -x "$BIN" ] || { echo "FAIL missing binary: $BIN"; echo "fails=1"; exit 1; }

# ---- 选一个能跑 playwright 的解释器 ----
PY=""
PROBE_CMD="${EHR_BROWSER_PROBE_CMD:-}"
if [ -n "$PROBE_CMD" ]; then
  note "使用外部探针命令: $PROBE_CMD"
else
  CANDIDATES="${EHR_BROWSER_PYTHON:-} $(command -v python3 2>/dev/null) $HOME/.qwenpaw/venv/bin/python3 $HOME/.cache/medme-ehr-browser-venv/bin/python"
  for c in $CANDIDATES; do
    [ -n "$c" ] || continue
    [ -x "$c" ] || continue
    if "$c" -c "import playwright" >/dev/null 2>&1; then PY="$c"; break; fi
  done
  if [ -z "$PY" ]; then
    note "本机没有可用的 playwright ⇒ 尝试按门禁侧依赖装（pypi TUNA 优先）"
    VENV="${EHR_BROWSER_VENV:-$HOME/.cache/medme-ehr-browser-venv}"
    PIP_INDEX="${EHR_PIP_INDEX:-https://pypi.tuna.tsinghua.edu.cn/simple}"
    PIP_TRUSTED="${EHR_PIP_TRUSTED_HOST:-pypi.tuna.tsinghua.edu.cn}"
    if [ -x "$VENV/bin/python" ] && "$VENV/bin/python" -c "import playwright" >/dev/null 2>&1; then
      note "复用已有门禁 venv: $VENV"; PY="$VENV/bin/python"
    elif python3 -m venv "$VENV" >/dev/null 2>&1; then
      "$VENV/bin/pip" install -q -i "$PIP_INDEX" --trusted-host "$PIP_TRUSTED" playwright \
        || "$VENV/bin/pip" install -q playwright || true
      [ -x "$VENV/bin/python" ] && "$VENV/bin/python" -c "import playwright" >/dev/null 2>&1 && PY="$VENV/bin/python"
    fi
    if [ -z "$PY" ]; then
      TGT="$HOME/.cache/medme-ehr-browser-pydeps"
      mkdir -p "$TGT"
      python3 -m pip install -q --target "$TGT" -i "$PIP_INDEX" --trusted-host "$PIP_TRUSTED" playwright \
        || python3 -m pip install -q --target "$TGT" playwright || true
      PYTHONPATH="$TGT${PYTHONPATH:+:$PYTHONPATH}" python3 -c "import playwright" >/dev/null 2>&1 && {
        PY="env PYTHONPATH=$TGT python3"; note "退化路径：--target 目录 + PYTHONPATH"; }
    fi
  fi
  [ -n "$PY" ] || { echo "  FAIL 装不上 playwright（门禁侧依赖拿不到）⇒ 浏览器级冒烟这道门不存在"; echo "fails=1"; exit 1; }
  note "playwright 解释器: $PY"
fi

# ---- 浏览器二进制：拿不到就红（不许静默跳过）----
if [ -z "$PROBE_CMD" ]; then
  if ! $PY - <<'PY' >/dev/null 2>&1
from playwright.sync_api import sync_playwright
with sync_playwright() as p:
    b = p.chromium.launch(args=["--no-sandbox"])
    b.close()
PY
  then
    echo "  FAIL 起不来 chromium ⇒ 浏览器级冒烟这道门不存在"
    echo "       修复：playwright install chromium（CI 里加这一步）"
    echo "fails=1"; exit 1
  fi
fi

# ---- 起真服务（真进程、真实数据、零 mock）----
H="$(mktemp -d /tmp/ehr-webbrowser-XXXXXX)"; mkdir -p "$H/vaults"
EHR_HOME="$H" "$BIN" --addr="127.0.0.1:$PORT" >"$H/serve.log" 2>&1 &
SRV=$!
trap 'kill $SRV 2>/dev/null; rm -rf "$H"' EXIT
for i in $(seq 1 40); do curl -sf "127.0.0.1:$PORT/api/v1/healthz" >/dev/null 2>&1 && break; sleep 0.5; done
curl -sf "127.0.0.1:$PORT/api/v1/healthz" >/dev/null 2>&1 || {
  echo "FAIL 服务未起来"; cat "$H/serve.log"; echo "fails=1"; exit 1; }
note "server up pid=$SRV home=$H port=$PORT"

# ---- 样本：上游 sample-records 的三份原件（缺失就现造等价样本）----
#   为什么是三份：浏览器门要覆盖**用药/病程/审计**三个读面（T42），
#   它们分别需要「处方」「出院记录」两类原文；只有血常规样本的话 meds/conds 必为空。
mk_sample() { # $1=glob  $2=文件名  $3=内容
  local f
  f=$(ls "$ROOT"/upstream-medme/examples/sample-records/"$1" 2>/dev/null | head -1)
  if [ -z "$f" ]; then
    f="$H/$2"; cat >"$f" <<EOF
$3
EOF
    note "上游样本 $1 缺失，改用脚本内造的等价样本（$f）" >&2
  fi
  echo "$f"
}
SAMPLE=$(mk_sample '*血常规*.txt' '血常规报告.txt' 'XX医院检验科检验报告单
姓名:门禁  性别:女  年龄:37岁   样本编号:20251015-A042
检验项目          结果      参考区间     标志
白细胞计数        11.8      3.5--9.5     H
血红蛋白          139       130--175     /
血小板计数        203       125--350     /
报告日期:2025年10月15日')
SAMPLE_MEDS=$(mk_sample '*处方*.txt' '处方.txt' 'XX医院 门诊处方笺 (prescription)
姓名:门禁  性别:女  年龄:37岁  科室:心血管内科
处方日期:2024-06-10    诊断:高血压 3 级(很高危)
Rp.
1. 苯磺酸氨氯地平片 Amlodipine  5mg × 14 片
   用法:口服,一次 1 片,一日 1 次(晨服)
2. 阿司匹林肠溶片 Aspirin  100mg × 30 片
   用法:口服,一次 1 片,一日 1 次(餐后)
医师:陈志强')
SAMPLE_CONDS=$(mk_sample '*出院记录*.txt' '出院记录.txt' 'XX医院 出院记录
姓名:门禁  性别:女  年龄:37岁  住院号:0628841
入院日期:2023-04-24    出院日期:2023-05-01    科室:神经内科
入院诊断:急性脑梗死(左侧基底节区)
既往史:高血压 10 年;2 型糖尿病 5 年。
出院诊断:1. 急性脑梗死  2. 高血压 3 级(很高危)  3. 2 型糖尿病
出院医嘱:继续口服阿司匹林 100mg qd、氨氯地平 5mg qd、二甲双胍 0.5g bid;门诊随访。')

PROBE_BASE="${EHR_BROWSER_PROBE_BASE:-http://127.0.0.1:$PORT}"
BASE="$PROBE_BASE" PERSON_NAME="浏览器冒烟·门禁" SAMPLE="$SAMPLE" \
  SAMPLE_MEDS="$SAMPLE_MEDS" SAMPLE_CONDS="$SAMPLE_CONDS" \
  $PY "$ROOT/tools/web_browser_probe.py"
RC=$?
[ "$RC" -ne 0 ] && fails=$((fails+1))

echo "[证据] web-browser-e2e.sh fails=$fails"
exit $([ "$fails" -eq 0 ] && echo 0 || echo 1)
