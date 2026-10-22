#!/usr/bin/env python3
"""medme-ehr · T24「产物新鲜度」门：门禁跑的那份二进制，必须不比源码旧。

为什么需要它（D-040 未销账②）：
  仓库里有**两份** ehr-api 产物 —— `ehr-api/target/{debug,release}/ehr-api`（本地构建目录）
  与 `~/medme-shared-target/{debug,release}/ehr-api`（多数门禁脚本实际跑的那份，见各脚本
  `EHR_BIN` / `CARGO_TARGET_DIR` 默认值）。两者靠**人工 `cp` 同步**，没有任何门。
  ⇒ 改完 Rust 代码忘了同步 ⇒ **门禁静默跑旧产物** ⇒ 门是绿的，但测的是上一轮的代码
  （D-035 新坑③「先问它跑的是哪一份产物」的正面处置）。

判据（三条独立方向，缺一即算不通过）：
  ① **新鲜度**：产物的 mtime 必须 **不早于** 最新源码 mtime（源码集合 = 本仓 ehr-api
     的 src/ tests/ Cargo.toml Cargo.lock **以及** 上游只读依赖 upstream-medme/packages
     的全部 .rs / Cargo.toml —— 上游改了同样会让产物过期）。
  ② **两份一致**：同一 profile 下，本地 target 产物与 shared-target 产物必须 **sha256 逐字相等**
     （否则就是「忘了同步」或「只同步了一半」）。
  ③ **可打印**：必须打印**产物路径 + 构建时间 + 源码最新时间**，让人一眼看出在跑哪一份。

注意：①是 **mtime 比较**（不是内容哈希）——Rust 产物里没有嵌入源码哈希，
   而 cargo 本身也是用 mtime 做增量判定的 ⇒ 与 cargo 的语义一致。
   ② 才是内容层面的硬判据。

退出码：0 = 全通过；1 = 有不通过项（逐条打印）。
"""
import hashlib
import os
import subprocess
import sys
import time

ROOT = os.path.dirname(os.path.abspath(__file__))

# 产物路径的**唯一来源**（ci/env.sh）—— 判据 D/E（ci_portability_check.py）：
#   「门禁跑哪份产物」这件事**只允许一份实现**。本文件原先自己写死
#   `os.environ.get("CARGO_TARGET_DIR", "~/medme-shared-target")`
#   ⇒ 那是**第二份实现**（且默认值指向某台机器上的固定路径）⇒ 必须改成
#   「没注入就明确失败」，不许悄悄用兜底值。
#
# 注入方式：artifact-freshness-check.sh 先 source ci/env.sh，再把
#   EHR_SHARED_TARGET_DIR / EHR_LOCAL_TARGET_DIR 传进来。
SHARED = os.environ.get("EHR_SHARED_TARGET_DIR")
LOCAL = os.environ.get("EHR_LOCAL_TARGET_DIR")
if not SHARED or not LOCAL:
    print("FAIL 产物路径未注入（EHR_SHARED_TARGET_DIR / EHR_LOCAL_TARGET_DIR）")
    print("     ⇒ 产物路径只能来自唯一来源 ci/env.sh；不许本文件自带兜底默认值")
    sys.exit(1)

# 源码集合：本仓 ehr-api + 上游只读依赖（上游改了对本仓同样生效）
SRC_DIRS = [os.path.join(ROOT, "ehr-api", "src"), os.path.join(ROOT, "ehr-api", "tests"),
            os.path.join(ROOT, "upstream-medme", "packages")]
SRC_FILES = [os.path.join(ROOT, "ehr-api", "Cargo.toml"), os.path.join(ROOT, "ehr-api", "Cargo.lock")]

PROFILES = ["debug", "release"]


def newest_source():
    """返回 (最新源码 mtime, 最新源码路径)。"""
    best, bestp = 0.0, None
    for d in SRC_DIRS:
        for dirpath, _dirs, files in os.walk(d):
            for f in files:
                if not (f.endswith(".rs") or f.endswith(".toml") or f.endswith(".lock")):
                    continue
                p = os.path.join(dirpath, f)
                try:
                    m = os.path.getmtime(p)
                except OSError:
                    continue
                if m > best:
                    best, bestp = m, p
    for p in SRC_FILES:
        if os.path.exists(p):
            m = os.path.getmtime(p)
            if m > best:
                best, bestp = m, p
    return best, bestp


def sha256(p):
    h = hashlib.sha256()
    with open(p, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def main():
    src_m, src_p = newest_source()
    fails = 0
    print("[证据] 源码最新 mtime = %s (%s)" % (
        time.strftime("%Y-%m-%d %H:%M:%S", time.localtime(src_m)),
        os.path.relpath(src_p, ROOT) if src_p else "NONE"))
    if src_m == 0:
        print("FAIL 新鲜度: 找不到任何源码文件（判据无法计算）")
        return 1

    for prof in PROFILES:
        rel = os.path.join("ehr-api", "target", prof, "ehr-api")
        local = os.path.join(LOCAL, prof, "ehr-api")
        shared = os.path.join(SHARED, prof, "ehr-api")
        for label, p in (("local", local), ("shared", shared)):
            if not os.path.exists(p):
                print("FAIL %s/%s 产物不存在: %s" % (prof, label, p))
                fails += 1
                continue
            m = os.path.getmtime(p)
            print("[证据] 产物 %s/%s = %s  mtime=%s  sha256=%s" % (
                prof, label, p, time.strftime("%Y-%m-%d %H:%M:%S", time.localtime(m)), sha256(p)))
            if m + 1 < src_m:  # 1s 容差：文件系统时间戳精度
                print("FAIL %s/%s 产物比源码旧（产物=%s 源码=%s）⇒ 门禁会静默跑旧产物" % (
                    prof, label,
                    time.strftime("%Y-%m-%d %H:%M:%S", time.localtime(m)),
                    time.strftime("%Y-%m-%d %H:%M:%S", time.localtime(src_m))))
                fails += 1
        if os.path.exists(local) and os.path.exists(shared):
            if sha256(local) != sha256(shared):
                print("FAIL %s 两份产物 sha256 不一致 ⇒ 忘了同步（local=%s shared=%s）" % (
                    prof, os.path.relpath(local, ROOT), shared))
                fails += 1

    print("[证据] 门禁实际使用的产物路径（各脚本默认值）:")
    print("[证据]   release 门（EHR_BIN 默认）: %s" % os.path.join(SHARED, "release", "ehr-api"))
    print("[证据]   debug 门（CARGO_TARGET_DIR 默认）: %s" % os.path.join(SHARED, "debug", "ehr-api"))
    print("[证据] 新鲜度门 fails=%d" % fails)
    return 1 if fails else 0


if __name__ == "__main__":
    sys.exit(main())
