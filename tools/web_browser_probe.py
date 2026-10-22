#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""medme-ehr · T41「浏览器级冒烟」探针（真浏览器 + 真页面 + 真 API）。

为什么需要它（PLAN T30 遗留⑨ / T5 前置）：
    仓库里已有的 Web 门禁 `web-e2e.sh` 只做到 **JS 语法**（node --check）+ 字段名核对
    ⇒ **DOM 渲染、事件绑定、点击后的真实数据流** 从来没有任何门
    ⇒ 页面 JS 整体抛异常、按钮点了没反应、表格渲染成空，现状都是「门禁全绿」。

本探针做的事（全部在真 Chromium 里，真进程真数据零 mock）：
    B1  页面加载无未捕获异常（pageerror）+ 无 console error
    B2  #ver 显示 openapi 标题与 paths 数（证明页面自己的 XHR 真的通了）
    B3  侧栏建档（填 #newName → 点 #btnAdd）⇒ 人员出现在 #plist
    B4  时间线表格渲染（表头 + 行）
    B5  上传原件（set_input_files(#f) → 点 #up）⇒ 文档表出现详情链接
    B6  点文档详情链接 ⇒ 详情面板出现 sha256 与正文
    B7  指标页渲染出 series=
    B8  自测趋势页：先经 API 写入一条自测（写进**页面上正选着的那个人**）⇒ 页面渲染出分组与数值
    B9  检索页：填 #q → 点 #go ⇒ 命中结果渲染
    B10 页面不得把错误当内容渲染出来（出现 [ERROR] ⇒ 红）

**门自身的纪律**（D-011 家族：门红了先怀疑门）：
    每一步都包在 try/except 里 ⇒ 任何一步抛异常都记成该步 FAIL 并**继续往下跑**，
    最后**总是**打印 `== RESULT fails=N` ⇒ 门不会以「崩溃」代替「判定」。

用法： BASE=http://127.0.0.1:PORT PERSON_NAME=xxx SAMPLE=<文件> python3 tools/web_browser_probe.py
"""
import json
import os
import sys
import urllib.request

# ⚠️ 门禁脚本会把输出重定向到文件 ⇒ 必须**行缓冲**，否则进程被杀时缓冲里的判据全丢
#   （本轮实撞：探针被 timeout 杀掉后，日志里一个字都没有 ⇒ 门既没 PASS 也没 FAIL
#     ⇒ 「以崩溃代替判定」，正是 D-011 要防的形态。）
try:
    sys.stdout.reconfigure(line_buffering=True)
except Exception:  # noqa: BLE001
    pass

BASE = os.environ.get("BASE", "http://127.0.0.1:18699")
PERSON = os.environ.get("PERSON_NAME", "浏览器冒烟·门禁")
SAMPLE = os.environ.get("SAMPLE", "")

fails = 0
step_no = 0


def ok(msg):
    print("[GATE PASS] %s" % msg)


def bad(msg):
    global fails
    fails += 1
    print("[GATE FAIL] %s" % msg)


def step(label):
    """跑一步；异常 ⇒ 记 FAIL 并继续（D-011：门不能以崩溃代替判定）。"""
    global step_no
    step_no += 1

    def deco(fn):
        try:
            fn()
        except Exception as e:  # noqa: BLE001 —— 门必须捕获一切
            bad("%s 步骤异常(%s): %s: %s" % (label, type(e).__name__, e, repr(e)[:200]))
        return fn

    return deco


def main():
    from playwright.sync_api import sync_playwright

    with sync_playwright() as p:
        browser = p.chromium.launch(args=["--no-sandbox"])
        page = browser.new_page()
        # ⚠️ 默认动作超时必须压短（playwright 默认 30s/动作 ⇒ 页面坏掉时十来个步骤
        #   串起来会把门拖到几十分钟，门就变成「卡住」而不是「判红」⇒ 本轮实撞过一次）
        page.set_default_timeout(6000)
        page_errors = []
        console_errors = []
        page.on("pageerror", lambda e: page_errors.append(str(e)))
        page.on("console", lambda m: console_errors.append(m.text) if m.type == "error" else None)
        page.goto(BASE + "/", wait_until="networkidle")

        # ---- B1 加载期无未捕获异常 ----
        @step("B1")
        def _b1():
            if page_errors:
                bad("B1 页面加载期有未捕获异常: %s" % page_errors)
            else:
                ok("B1 加载期无未捕获异常")

        # ---- B2 页面自己的 openapi 拉取成功 ----
        @step("B2")
        def _b2():
            ver = (page.inner_text("#ver") or "").strip()
            if "paths=" in ver and "不可达" not in ver:
                ok("B2 版本行=%s" % ver)
            else:
                bad("B2 版本行异常: %r（openapi 不可达？）" % ver)

        # ---- B3 侧栏建档 ----
        @step("B3")
        def _b3():
            page.fill("#newName", PERSON)
            page.click("#btnAdd")
            page.wait_for_function(
                "(name) => Array.from(document.querySelectorAll('#plist li')).some(li => li.textContent.includes(name))",
                arg=PERSON, timeout=6000,
            )
            ok("B3 建档后人员出现在侧栏")

        # ---- B5 上传原件（在 documents 页）----
        @step("B5")
        def _b5():
            if not SAMPLE or not os.path.exists(SAMPLE):
                bad("B5 没有样本文件可上传（SAMPLE=%r）" % SAMPLE)
                return
            page.click('button[data-t="documents"]')
            page.wait_for_timeout(400)
            page.set_input_files("#f", SAMPLE)
            with page.expect_event("dialog") as di:
                page.click("#up")
            dlg = di.value
            msg = dlg.message
            dlg.accept()
            if "已入库" in msg:
                ok("B5 上传成功(alert): %s" % msg[:80])
            else:
                bad("B5 上传失败(alert): %s" % msg[:200])
            page.wait_for_timeout(1500)

        # ---- B4 时间线渲染 ----
        @step("B4")
        def _b4():
            page.click('button[data-t="timeline"]')
            page.wait_for_timeout(800)
            rows = page.query_selector_all("#pane table tr")
            if len(rows) >= 2:
                ok("B4 时间线渲染 %d 行(含表头)" % len(rows))
            else:
                bad("B4 时间线表格未渲染（tr=%d）" % len(rows))

        # ---- B5b 文档表 ----
        @step("B5b")
        def _b5b():
            page.click('button[data-t="documents"]')
            page.wait_for_timeout(800)
            det = page.query_selector_all("a.det")
            if det:
                ok("B5b 文档表渲染 %d 行，含详情链接" % len(det))
            else:
                bad("B5b 文档表没有详情链接（上传未落到页面？）")

        # ---- B6 详情面板 ----
        @step("B6")
        def _b6():
            det = page.query_selector_all("a.det")
            if not det:
                bad("B6 没有详情链接可点")
                return
            det[0].click()
            page.wait_for_timeout(800)
            body = page.inner_text("#pane")
            if "sha256=" in body:
                ok("B6 详情面板渲染出 sha256")
            else:
                bad("B6 详情面板没渲染出 sha256（点击无效果？）")

        # ---- B7 指标页 ----
        @step("B7")
        def _b7():
            page.click('button[data-t="labs"]')
            page.wait_for_timeout(800)
            pane = page.inner_text("#pane")
            if "series=" in pane:
                ok("B7 指标页渲染: %s" % pane.split("\n")[0][:80])
            else:
                bad("B7 指标页未渲染 series=")

        # ---- B8 自测趋势（写进**页面上正选着的那个人**）----
        @step("B8")
        def _b8():
            cur = page.get_attribute("#plist li.on", "data-id")
            if not cur:
                bad("B8 读不到页面上正选的人的 id（li.on 无 data-id）")
                return
            req2 = urllib.request.Request(
                BASE + "/api/v1/persons/%s/entries/self-measurement" % cur,
                data=json.dumps({
                    "taken_at": "2026-10-01T08:00:00Z",
                    "values": [
                        {"analyte_key": "bp_systolic", "value": 128, "unit": "mmHg"},
                        {"analyte_key": "bp_diastolic", "value": 82, "unit": "mmHg"},
                    ],
                }).encode(),
                headers={"content-type": "application/json"},
                method="POST",
            )
            urllib.request.urlopen(req2).read()
            page.click('button[data-t="home"]')
            page.wait_for_timeout(1000)
            pane = page.inner_text("#pane")
            if "bp_systolic" in pane and "128" in pane:
                ok("B8 自测趋势渲染出 bp_systolic/128 (pid=%s)" % cur)
            else:
                bad("B8 自测趋势页未渲染自测数据: %r" % pane[:200])

        # ---- B9 检索 ----
        @step("B9")
        def _b9():
            page.click('button[data-t="search"]')
            page.wait_for_timeout(400)
            page.fill("#q", "血常规")
            page.click("#go")
            page.wait_for_timeout(1200)
            res = page.inner_text("#res")
            if "q=" in res:
                ok("B9 检索渲染: %s" % res.split("\n")[0][:80])
            else:
                bad("B9 检索无结果渲染: %r" % res[:200])

        # ---- B11/B12/B13 用药 / 病程 / 审计 三个读面（T42：读面已毕业但页面此前未接）----
        def upload_to_cur(path):
            """把样本经**真 API** 传进页面上正选着的那个人（与 B8 同款纪律：不靠同名匹配去猜人）。"""
            cur = page.get_attribute("#plist li.on", "data-id")
            if not cur:
                return None
            boundary = "----probe%s" % os.getpid()
            with open(path, "rb") as f:
                body = b"".join([
                    ("--%s\r\n" % boundary).encode(),
                    b'Content-Disposition: form-data; name="file"; filename="probe.txt"\r\n',
                    b"Content-Type: text/plain\r\n\r\n", f.read(), b"\r\n",
                    ("--%s--\r\n" % boundary).encode(),
                ])
            req = urllib.request.Request(
                BASE + "/api/v1/persons/%s/documents" % cur, data=body,
                headers={"content-type": "multipart/form-data; boundary=%s" % boundary},
                method="POST")
            urllib.request.urlopen(req).read()
            return cur

        @step("B11")
        def _b11():
            meds = os.environ.get("SAMPLE_MEDS", "")
            if not meds or not os.path.exists(meds):
                bad("B11 没有处方样本可上传（SAMPLE_MEDS=%r）" % meds)
                return
            upload_to_cur(meds)
            page.click('button[data-t="meds"]')
            page.wait_for_timeout(1200)
            pane = page.inner_text("#pane")
            if "med_count=" in pane and "氨氯地平" in pane:
                ok("B11 用药页渲染出 med_count 与药名: %s" % pane.split("\n")[0][:80])
            else:
                bad("B11 用药页未渲染出药品: %r" % pane[:200])

        @step("B12")
        def _b12():
            conds = os.environ.get("SAMPLE_CONDS", "")
            if not conds or not os.path.exists(conds):
                bad("B12 没有出院记录样本可上传（SAMPLE_CONDS=%r）" % conds)
                return
            upload_to_cur(conds)
            page.click('button[data-t="conds"]')
            page.wait_for_timeout(1200)
            pane = page.inner_text("#pane")
            if "condition_count=" in pane and "脑梗死" in pane:
                ok("B12 病程页渲染出 condition_count 与诊断: %s" % pane.split("\n")[0][:80])
            else:
                bad("B12 病程页未渲染出诊断: %r" % pane[:200])

        @step("B13")
        def _b13():
            page.click('button[data-t="audit"]')
            page.wait_for_timeout(1200)
            pane = page.inner_text("#pane")
            if "count=" in pane and ("导入" in pane or "create" in pane):
                ok("B13 审计页渲染出条目: %s" % pane.split("\n")[0][:80])
            else:
                bad("B13 审计页未渲染出条目: %r" % pane[:200])

        # ---- B14/B15/B16 写侧「记录」页（T5 第 3 小步：页面此前只能读不能写）----
        @step("B14")
        def _b14():
            page.click('button[data-t="records"]')
            page.wait_for_timeout(800)
            pane = page.inner_text("#pane")
            if "自测值" not in pane or "addMeas" not in page.inner_html("#pane"):
                bad("B14 记录页未渲染出自测值写入表单")
                return
            page.select_option("#mKey", "heart_rate")
            page.fill("#mVal", "72")
            with page.expect_event("dialog") as di:
                page.click("#addMeas")
            dlg = di.value
            msg = dlg.message
            dlg.accept()
            if "自测值成功" in msg:
                ok("B14 自测值写入成功(alert): %s" % msg[:80])
            else:
                bad("B14 自测值写入失败(alert): %s" % msg[:200])

        @step("B14b")
        def _b14b():
            # 判据落在**回放投影**（页面把刚写的东西列出来）⇒ 证明「写进去且读得回来」
            page.click('button[data-t="records"]')
            page.wait_for_timeout(1200)
            pane = page.inner_text("#pane")
            if "heart_rate=72" in pane:
                ok("B14b 记录页回放投影渲染出 heart_rate=72")
            else:
                bad("B14b 记录页回放投影未渲染出写入的自测值: %r" % pane[:300])

        @step("B15")
        def _b15():
            page.click('button[data-t="records"]')
            page.wait_for_timeout(800)
            page.fill("#nText", "浏览器门禁写的笔记")
            with page.expect_event("dialog") as di:
                page.click("#addNote")
            dlg = di.value
            msg = dlg.message
            dlg.accept()
            if "笔记成功" not in msg:
                bad("B15 笔记写入失败(alert): %s" % msg[:200])
                return
            page.wait_for_timeout(1000)
            pane = page.inner_text("#pane")
            if "浏览器门禁写的笔记" in pane:
                ok("B15 笔记写入后回放投影渲染出该笔记")
            else:
                bad("B15 笔记未出现在回放投影: %r" % pane[:300])

        @step("B16")
        def _b16():
            page.click('button[data-t="records"]')
            page.wait_for_timeout(800)
            page.fill("#pKind", "enable")
            page.fill("#pPkg", "sle")
            page.fill("#pAt", "2026-10-01")
            page.fill("#pPayload", "{}")
            with page.expect_event("dialog") as di:
                page.click("#addPe")
            dlg = di.value
            msg = dlg.message
            dlg.accept()
            if "病程事件成功" not in msg:
                bad("B16 病程事件写入失败(alert): %s" % msg[:200])
                return
            page.wait_for_timeout(1000)
            pane = page.inner_text("#pane")
            if "profile_event" in pane and "2026-10-01" in pane:
                ok("B16 病程事件写入后回放投影渲染出该事件")
            else:
                bad("B16 病程事件未出现在回放投影: %r" % pane[:300])

        # ---- B10 页面不得把错误当内容渲染出来 ----
        @step("B10")
        def _b10():
            # 判据落在**整页**而不是 #pane：pane 会被后一步的视图替换 ⇒ 只看 pane 会漏判
            body = page.inner_text("body")
            if "[ERROR]" in body:
                bad("B10 页面渲染出 [ERROR]（错误被当成内容显示）")
            else:
                ok("B10 页面无 [ERROR] 残留")

        # ---- B1b console error ----
        @step("B1b")
        def _b1b():
            if console_errors:
                bad("B1b console error: %s" % console_errors[:3])
            else:
                ok("B1b 无 console error")

        browser.close()

    print("== RESULT fails=%d" % fails)
    return 1 if fails else 0


if __name__ == "__main__":
    sys.exit(main())
