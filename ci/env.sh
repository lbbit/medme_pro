#!/usr/bin/env bash
# medme-ehr · 门禁用二进制路径的**唯一事实来源**（被各门禁脚本 source）
#
# 为什么需要它（D-041 纪律「同一事实一份实现」的跨脚本应用）：
#   本仓库有**两类**门禁脚本，各自取产物路径的方式**不一样**：
#     ① debug 侧：entries/conds/meds/audit/auth/api-doc-check —— 用 `$CARGO_TARGET_DIR/debug/ehr-api`
#     ② release 侧：mcp-e2e / mcp-contract / doc-contract / skill-contract /
#                   skill-rest / skill-doc-field / ehtest / rebuild / rebuild-lock
#                  —— 用 `$HOME/medme-shared-target/release/ehr-api`（**写死**）
#   ⇒ 同一件事（"门禁跑哪份产物"）有**两份实现** ⇒ 改一处忘了另一处 ⇒
#     门禁静默跑旧产物（D-035 新坑③「先问它跑的是哪一份产物」）。
#   ⇒ 之前还有一个更硬的洞：`auth-e2e.sh` 用**相对路径** `./target/debug/ehr-api`，
#     而 `cargo build` 写进 `$CARGO_TARGET_DIR` ⇒ 在 CARGO_TARGET_DIR≠仓库内 target 时，
#     它启动的是**另一份（可能很旧的）产物**，而门禁是绿的。
#
# 契约（本文件是唯一来源，各门禁 source 它，不再自己拼路径）：
#   CARGO_TARGET_DIR      —— cargo 构建输出根；默认 $HOME/medme-shared-target（可用环境变量覆盖）
#   EHR_DBG_BIN           —— debug 产物路径
#   EHR_REL_BIN           —— release 产物路径
#   EHR_BIN               —— 门禁默认使用的产物（= EHR_REL_BIN；可被调用方环境变量覆盖）
#   EHR_LOCAL_TARGET_DIR  —— **仓库内**的 cargo 输出根（ehr-api/target）——新鲜度门要比
#                            「local 产物」与「shared 产物」两份 ⇒ 它的路径也必须由本文件给出
#                            （T29：判据 D/E —— 产物根目录与产物路径都只有一份实现）
#
# 纪律：**改产物路径只能改这里**。ci-portability-check.sh 会机器核对这件事
#      （任何门禁脚本或 Python 内层实现里出现硬编码的机器专属绝对路径、
#        第二份产物根目录、或产物路径变量自带兜底默认值 ⇒ 立刻红）。

# CARGO_TARGET_DIR：唯一来源。CI 里可用它把构建产物放 runner 本地磁盘。
# ROOT：仓库根（由本文件位置推导，不依赖调用方 cwd ⇒ source 本文件的脚本在哪都能拿到）
ROOT="${ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/medme-shared-target}"

EHR_DBG_BIN="${EHR_DBG_BIN:-$CARGO_TARGET_DIR/debug/ehr-api}"
EHR_REL_BIN="${EHR_REL_BIN:-$CARGO_TARGET_DIR/release/ehr-api}"
EHR_BIN="${EHR_BIN:-$EHR_REL_BIN}"
# 仓库内 cargo 输出根（新鲜度门要比 local vs shared 两份产物）
EHR_LOCAL_TARGET_DIR="${EHR_LOCAL_TARGET_DIR:-$ROOT/ehr-api/target}"
export EHR_BIN EHR_DBG_BIN EHR_REL_BIN EHR_LOCAL_TARGET_DIR CARGO_TARGET_DIR
