#!/usr/bin/env bash
# medme-ehr · T28「CI 可移植性」门禁 —— 门禁脚本不许写死机器专属路径。
# 内层实现见 ci_portability_check.py。
#
# 为什么需要它（PLAN 下一步①「统一入口尚未接入 CI」的正面处置）：
#   要把 `run-all-gates.sh` 放进 GitHub Actions，就必须先回答一个问题：
#   **这些脚本在别人的机器上跑得起来吗？**
#   本轮审计发现 **3 类真 portability 缺陷**（都不是猜测，是实测出来的）：
#     ① **硬编码机器专属绝对路径**：5 个脚本写死 `/home/lbb/medme-shared-target`，
#        1 个脚本写死 `/home/lbb/medme-ehr/upstream-medme/...`
#        ⇒ 在 CI runner（路径不同、用户不同）上**必然跑不起来**。
#     ② **产物路径有两份实现**：debug 侧用 `$CARGO_TARGET_DIR/debug/...`，
#        release 侧用写死的 `$HOME/medme-shared-target/release/...`
#        ⇒ 改一处忘了另一处 ⇒ 门禁静默跑旧产物（D-035 新坑③）。
#     ③ **启动的产物与构建的产物不是同一份**：`auth-e2e.sh` 用**相对路径**
#        `./target/debug/ehr-api`，而 `cargo build` 写进 `$CARGO_TARGET_DIR`
#        ⇒ 两者不同时它启动的是**另一份（可能很旧的）产物**，而门禁是绿的。
#   ⇒ 本门把「不许写死机器专属路径」变成**机器判据**，而不是靠人记得。
#
# 判据（三条独立方向，缺一即算不通过）：
#   A **无机器专属绝对路径**：门禁脚本（仓库根 *.sh + ci/*.sh）里不得出现
#     `/home/<某个用户名>/` 这类**绑定到某台机器**的路径 ⇒ 命中即红。
#     （只放行 `$HOME/...` 与 `/tmp/...` 这类**可移植**写法）
#   B **产物路径单一来源**：门禁脚本里**不许**自己拼 `.../debug/ehr-api` 或
#     `.../release/ehr-api` 这样的产物路径 ⇒ 必须用 `$EHR_BIN` / `$EHR_DBG_BIN` /
#     `$EHR_REL_BIN`（由 ci/env.sh 统一提供）⇒ 命中即红。
#   C **门禁都 source 了唯一来源**：每个起真进程的门禁脚本必须 source `ci/env.sh`
#     ⇒ 漏 source ⇒ 拿不到统一路径 ⇒ 命中即红。
#
# 用法: ./ci-portability-check.sh
set -uo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"
fails=0
python3 "$ROOT/ci_portability_check.py"; rc=$?
[ $rc -eq 0 ] || fails=$((fails+1))
echo "[证据] ci-portability-check.sh fails=$fails"
[ "$fails" -eq 0 ] || exit 1
