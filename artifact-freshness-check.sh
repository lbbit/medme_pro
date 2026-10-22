#!/usr/bin/env bash
# medme-ehr · T24「产物新鲜度」门禁 —— 门禁跑的那份二进制必须不比源码旧。
# 内层实现见 artifact_freshness_check.py。
#
# 为什么需要它（D-040 未销账② / PLAN 下一步②）：
#   仓库里有**两份** ehr-api 产物：`ehr-api/target/{debug,release}/ehr-api` 与
#   `~/medme-shared-target/{debug,release}/ehr-api`（多数 e2e 脚本实际跑的那份，
#   见 EHR_BIN / CARGO_TARGET_DIR 的默认值）。两者靠**人工 cp 同步**，没有门。
#   ⇒ 改完 Rust 代码忘了同步 ⇒ **门禁静默跑旧产物** ⇒ 门是绿的，但测的是上一轮代码
#   （D-035 新坑③「先问它跑的是哪一份产物」的正面处置）。
#
# 判据：①产物 mtime 不早于最新源码 mtime（含上游 upstream-medme/packages）；
#       ②同一 profile 下两份产物 sha256 逐字相等（= 没忘了同步）。
#
# 用法: ./artifact-freshness-check.sh
set -uo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"
# 产物路径的**唯一来源**（ci/env.sh）—— 本门要比「local 产物」与「shared 产物」两份，
# 两份路径都从这里来 ⇒ 内层实现不许自带默认值（T29 判据 D/E）。
. "$ROOT/ci/env.sh"
fails=0
EHR_SHARED_TARGET_DIR="$CARGO_TARGET_DIR" \
EHR_LOCAL_TARGET_DIR="$EHR_LOCAL_TARGET_DIR" \
  python3 "$ROOT/artifact_freshness_check.py"; rc=$?
[ $rc -eq 0 ] || fails=$((fails+1))
echo "[证据] artifact-freshness-check.sh fails=$fails"
[ "$fails" -eq 0 ] || exit 1
