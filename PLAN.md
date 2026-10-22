# medme-ehr 阶段计划

> 长期项目。以 MedMe(Rust 内核, AGPL-3.0)为底座做多人数电子健康档案(EHR)后端服务。
> 每次开工先读「当前状态」。上游只读参考在 `upstream-medme/`。

## 目标验收标准

- [ ] docker-compose 一键起：EHR API + Web 管理端（+数据库如需要）
- [ ] REST API（OpenAPI 自动生成）覆盖：人员 CRUD、指定人员的健康事件/报告/指标查询、数据维护写入
- [ ] QwenPaw SKILL + MCP server 接入，AI agent 可按人查询并辅助诊断
- [ ] 原件(CAS)+事件日志是唯一真相；派生索引可全量重建（有重建脚本证明）
- [x] docs/API.md 人类可读接口文档 —— **D-025：从运行时 OpenAPI 自动生成 + 漂移门禁**

## 架构方向（初稿，待源码勘察后定稿）

```
客户端: QwenPaw SKILL / MCP client / Web(React) / 手机 App(未来)
        │ HTTPS + Bearer token
        ▼
 ehr-api (Rust, axum) ──┬── 复用 medme core-model(事件日志+CAS+物化)
                        ├── 新增 person registry（多人档案层）
                        ├── 每人的 vault = 一个 MedMe 保险箱目录
                        └── 派生 SQLite/FTS5 读缓存（可重建）
```

倾向：**同步改造上游内核**（方案 A），而非 Python 包一层 CLI（慢且功能缺口大）。
理由待定稿于 DECISIONS.md。备选：B=HTTP 壳调 medme-cli；C=FHIR 标准服务(FHIR Server)兼容导出。

## 阶段

| # | 阶段 | 产出 | 状态 |
|---|------|------|------|
| 0 | 上游勘察 | 《upstream-study.md》：core-model API 面、单人假设深度、services/api 现状、可否独立编译成库 | ✅ 完成(2026-10-19) |
| 1 | POC：单人不带 UI 跑通内核导入→查询(library级+346单测全绿) | poc/src/import_demo.rs 9步验收全PASS(D-009) | ✅ GREEN(2026-10-19) |
| 2 | person 层设计 + ehr-api 骨架（axum, OpenAPI）——**已完成** | /api/v1 MVP；curl版抽查对齐POC证据链(D-011:34门全绿)；读侧聚合面已毕业 labs(D-025)/medications(D-026)/conditions(D-027)；写侧手动条目已毕业 entries×3 + measurements(D-028)；审计视图已毕业 audit(D-030) | ✅ GREEN(2026-10-24) |
| 3 | docker-compose（server+web）+ Web 管理端最小版 | 一键部署可用 | ✅ GREEN(2026-10-20，D-014/D-016) |
| 4 | MCP server + QwenPaw SKILL | agent 可按人查档/写档 | 🔄 MCP **读侧 11 + 写侧 2 = 13 工具已交付**（D-017/D-019/**D-029**/**D-031**）；工具数改为**从实现派生**（D-032：`--print-mcp-tools`，不再硬编码三处）；SKILL REST 面已扩到 **16 命令**（含 D-028 四个新命令），注册进 skill_pool 待验 |
| 5 | 认证与多客户端（token/token scope），手机客户端预留 | 安全基线 | 🔄 T4 静态 Bearer token 已交付（D-022）；多客户端/手机端待做 |

## 已知事实（已核实，来源 README/GitHub 页面 2026-10-19）

- 栈: Rust workspace(crates: core-model/parser/pipeline/ocr/dicom/medme-share)；桌面 Tauri v2+React；移动 Flutter(frb)。
- 存储: 事件溯源 JSONL(按设备分段、追加免冲突) + CAS sha256；SQLite FTS5 是派生缓存可重建。
- `services/api`、`services/claim-signer` 目录存在（内容未勘察）。
- AGPL-3.0。路线图含 FHIR/OMOP 导出、AI 健康洞察。OCR：Linux 走 PP-OCRv5(内置模型)。
- 「数据整理工具，非医疗器械」免责声明精神保留。

## 待验证假设（阶段 0 逐项销账）

1. core-model 等 crate 不依赖 Tauri/guizmo，可作为纯库被 axum 服务调用
2. vault 根目录只是普通文件夹 → 一人一 vault 即可实现多人隔离（或需加 person 维度进事件？）
3. services/api 现状是什么语言/RPC？与 medme-cli 的复用关系
4. 事件 schema 是否已有 subject/patient 概念预留
5. 中文全文检索(FTS5 分词器)可否直接用于跨人查询层
6. OCR pipeline 作为服务端异步任务的可行性（长任务队列怎么表达）

## 当前状态（2026-10-23 · **T40 收尾——T40 改动（Rust 源码 + 一处文档面 + 新门）已在真 GitHub runner 上验过**：push `cd8c594` ⇒ run **39828219504** 全绿，25 门 fails=0 ⇒ 「本机绿」升级为「CI 上也绿」· D-058 落地**）

- ✅ **T40-⑦ 本机全量复跑**：`./run-all-gates.sh` ⇒ A1 PASS + **25 个 `[PASS] … fails=0`**（末行「门禁数=25（本次执行 25）总 fails=0」，exit=0）⇒ 与上轮记录一致。
- ✅ **T40-⑧ 真 runner 复验（PLAN T40 下一步⑨ 销账）**：
  - `git push origin main` ⇒ `3f3a773..cd8c594`；workflow `gates` 触发 run **39828219504**（head_sha=cd8c5948）⇒ **conclusion=success**；
  - runner 上的 13 个 step **逐个 success**（装 Rust / 装 node / 拉上游 / TUNA 依赖通道 / 构建 debug+release --locked / cargo test / 产物同步 / 构建容器镜像 / 起 compose / **统一门禁入口 run-all-gates.sh** success / 「门禁失败时打印原始输出」skipped）⇒
  - runner 日志里量到**同一份汇总数字**：「`[证据] run-all-gates.sh 门禁数=25（本次执行 25）总 fails=0`」⇒ 与本机**逐字相同** ⇒ **门禁数没有被 CI 跳过/漏跑**（不是「CI 少跑了几道所以也绿」）；
  - ⇒ **纪律再确认**：「改了 Rust 源码 ⇒ 本机绿**不算**交付，必须有一次真 runner 绿」⇒ T39/T40 连续两轮都靠这条才补上。
- ✅ **后台卡死检查（D-010）**：开工时无 cargo/rustc 残留、无 done 哨兵残留、无孤儿 ehr-api 手工进程；本轮临时 ehr-api（`:18696` 等）由门禁脚本 `trap` 自清 ⇒ 常驻只剩两个 **docker 容器**内的 ehr-api ⇒ **非卡死**。
- 🔄 **下一步**：①**T5 Web 管理端正式化**（目标验收①最后一块，**选型先报方案再动手**）；②**MCP 侧 prompts** 仍显式钉住「不提供」；③官方 SDK 的 Java 实现仍未验；④公网暴露前 `EHR_BIND_ADDR` 改 `0.0.0.0` 前**必须**先设 token；⑤ghcr 包可见性仍 private；⑥`.dcm`/DICOM worker 未做；⑦上游分类缺口仍在；⑧人员域三端点仍 501 且刻意不进 OpenAPI；⑨**T5 Web 正式化时必须配「浏览器级冒烟」**（W2 只查 JS 语法，DOM 渲染仍无门 —— T30 遗留）；⑩**agent-e2e 真 agent 门**仍 SKIP（需真 agent CLI + 外部服务态）。
- ⚠️ 未销账：目标验收①**Web 管理端仍是单文件最小版**；`docs/API.md` 是**生成物**；文档覆盖 **REST 面** + **MCP 面** + **SKILL.md 面** + **CI 可移植性面** + **Web 页面 JS 读面** + **第三方客户端消费面（两个官方实现）** 六份文档面全部有门；**「手抄实现细节」残留**已治到十四个面；**「真相层完整性」**（第十五个面）与**「跨面同一事实（同一份上限、同一份回执）」**各为一道被门钉住的面。

## 当前状态（2026-10-23 · **T40「写侧多轮」——幂等（重传同字节）路径的**回执与真相层不一致**：回执说「没建档、没索引」，而真相层里那份文档明明已建档、已索引 ⇒ 修法：**回执回链内核既有事实**（不再自造）· D-057 落地**）

- ✅ **T40-① 缺口定位**：PLAN 从 T35 起挂着同一条未销账项——写侧「**多轮/并发/大载荷**」三维。T38 关「**并发**」（D-055）、T39 关「**大载荷**」（D-056）⇒ 本轮关「**多轮**」。事实核对：写侧在真进程里**只被调用过 1 次建档 + 1 次上传** ⇒ 「多轮累积后派生读面还准不准」「重传幂等吗」「重建后还原吗」「MCP 面与 REST 面同源吗」「MCP 面并发建档安全吗」全都**没有门**。
- ✅ **T40-② 干跑在前（先量再动手）**：先用一次性干跑把「多轮」这一维在 REST/MCP 两面**逐项量过**（20 轮建档+上传+幂等重传 · 5 份与 60 份同一人多文档 + 重建 · 30 个 MCP 并发建档 · 跨人去重核对）⇒ 量到的结论是「**多轮在真进程下没有暴露结构性缺陷**」（registry 逐行完整 · 读面计数与重建后逐字相等 · 幂等判定正确 · 两面同源 · 跨人去重不串档）⇒ ⇒ **本轮交付形态定为「把这一维钉成门禁」而非「改产品代码」**（纪律：验证类小步优先选让失败原因唯一的那一步）。
  - ⚠️ **但门禁在编写过程中自己撞出一个真缺陷**（见 T40-③）⇒ **干跑没抓到、门抓到了** ⇒ 纪律再确认：「干跑跑过」≠「有门」，两种不同的存在。
- ✅ **T40-③ 真缺陷（门抓到，实测非推演）：幂等路径的回执与真相层不一致**：
  - 场景：**同一份字节第二次上传**（CAS 按内容去重 ⇒ `deduped=true`）且该件**已建档**；
  - 旧实现走「已完整索引 ⇒ 零改动直接返回」分支 ⇒ 回执 `document_id=null` / `text_indexed=false` / `doc_type=null` ⇒ **回执说「没进档、没索引」**，而**真相层里它明明已建 Document 行、已有 OCR 文本层**（读面写着 `document_id=1, text_indexed=true`）⇒ **同一事实两处不一致**；
  - ⇒ 危害：不是数据丢失（真相层是对的），而是**回执说谎** ⇒ 违反 D5（字段不许粉饰）与 D2（一个语义只允许一处实现）⇒ 是 **D-034（OCR 路径「回执 document_id=null」）的同族复发**。
- ✅ **T40-④ 结构决策①：回执不是「本次做了什么」的流水，而是「**现在真相层里是什么**」的快照**。
  - ⇒ 修法：幂等分支**也回填**内核既有 Document/OCR 事实（`document_by_source_file_id` + `ocr_page_numbers` ⇒ 与读面 `list_document_items()` 同一份实现）⇒ 重传回执的 `document_id`/`doc_type`/`text_indexed` 与**首次上传**、与**读面**逐字相同；
  - ⇒ 纪律（记入 DECISIONS）：**回执是对真相层的投影**——凡回执字段都能从真相层读出来，就必须**回链**读它，而不是**自造**。
- ✅ **T40-⑤ 证据链**：
  - ①**库级门 `ehr-api/tests/write_multiround.rs`（5 条全绿）**：多轮累积（20 份不同内容 ⇒ timeline/documents/labs 点数 == 20）· 重传幂等（5 次 ⇒ 箱里恰好 1 条，**且每次重传回执与首传、与读面逐字相同**）· 重建还原（12 份 ⇒ 重建后读面计数逐字相等）· **MCP 面多轮写 ⇒ REST 读面同源**（8 份）· **MCP 面并发建档**（32 个 ⇒ registry 逐行完整、读面仍可用 ⇒ T38 的修复**同时覆盖 MCP 面**这条「应当覆盖」现在有门了）。
  - ②**新门 `write-multiround-e2e.sh`（真进程 + 真 HTTP + 真 MCP，6 判据全绿）**：M1 多轮累积（15/15/15）· M2 重传同字节幂等且回执与真相层一致 · M3 重建后还原 · M4 MCP 面多轮写 ⇒ REST 同源 · M5 MCP 面并发建档（registry 22 行 0 破损）· M6 真相层逐行完整（直接验落盘字节）。
  - ③**负向证据（实撞 2 次，均已复原）**：退回旧实现 ⇒ **库级门红**（`重传回执的 document_id 必须与首次一致… left: None, right: Some(1)`）+ **e2e 门红**（`FAIL 第 1 次重传回执 document_id=None ≠ 首传 1`）⇒ 两道门都**精确指认那一个缺陷** ⇒ 复原 ⇒ 全绿。
  - ④**Rust 侧零破坏**：`cargo test --all-targets --locked` ⇒ **99 门全绿**（原 94 + 本轮新 5）；`Cargo.toml`/`Cargo.lock` **未改动** ⇒ 零新依赖。
  - ⑤**产物新鲜度**：改了 Rust 源码 ⇒ 重编译 debug+release（local + shared 两份，D-029）⇒ `artifact-freshness-check.sh` **fails=0**（debug `842c1561…` / release `5e349c63…`）。
  - ⑥**文档面同步（本轮真撞到一处漂移）**：幂等回执多了 `doc_type` ⇒ **`skill-doc-field-check.sh` 当场红**（`FAIL upload 真响应有但表格未提及的字段: ['doc_type']`）⇒ 补齐 `SKILL.md` 的 `upload` 字段表 ⇒ 转绿 ⇒ ⇒ 纪律（D-039⑩）再确认：**改一个共享响应结构 ⇒ 所有承诺它的文档面都要同步**，而这件事**已由门机器守住**。
  - ⑦**统一入口全量复跑：25 门全绿，总 fails=0**（`run-all-gates.sh` ⇒ A1 PASS + 25 个 `[PASS] … fails=0`；末行「门禁数=25（本次执行 25）总 fails=0」）⇒ 覆盖面净增一道真跑的门（24 → 25）；新门已登记进 RUN 清单与 `ci_portability_check.py` 的 `NEEDS_ENV`。
  - ⑧**后台卡死检查（D-010）**：开工时无 cargo/rustc 残留、无 done 哨兵残留；本轮起的临时 ehr-api（`:18901`~`:18905`、`:18698`）**均已收割**（`trap` 自清）⇒ 常驻只剩两个 **docker 容器**内的 ehr-api ⇒ **非卡死**。
  - ⑨**门自身的坑（实撞并修好 3 处，都是「假红/假弱」不是假绿）**：`rpc` 包装吞掉 `-o` ⇒ 并发响应没落盘 ⇒ 判据收到 0 个 id（**假红**，实为门的错）；「重传样本」误用新内容 ⇒ 期望值写错 ⇒ 改为**复用 M1 的第 1 份**（同字节 ⇒ 真·重传）且期望从「恰好 1 条」改成「**仍是 N 条**」（重复不新增）；**裸 `wait`** 会等常驻服务 ⇒ 门挂住（T38 已记录，本轮又踩）⇒ 一律只 wait curl 的 PID。
- ✅ **T40-⑥ 销账**：写侧「**多轮/并发/大载荷**」三维**已全部有门**（T38 · T39 · T40）⇒ 这条从 T35 挂到 T40 的未销账项**可以销账**。
- 🔄 **下一步**：①**T5 Web 管理端正式化**（目标验收①最后一块，**选型先报方案再动手**）；②**MCP 侧 prompts** 仍显式钉住「不提供」；③官方 SDK 的 Java 实现仍未验；④公网暴露前 `EHR_BIND_ADDR` 改 `0.0.0.0` 前**必须**先设 token；⑤ghcr 包可见性仍 private；⑥`.dcm`/DICOM worker 未做；⑦上游分类缺口仍在；⑧人员域三端点仍 501 且刻意不进 OpenAPI；⑨~~本轮改动尚未在真 GitHub runner 上验过~~ ⇒ **已销账**（见上方 T40 收尾节：run 39828219504 全绿）。
- ⚠️ 未销账：目标验收①**Web 管理端仍是单文件最小版**；`docs/API.md` 是**生成物**；文档覆盖 **REST 面** + **MCP 面** + **SKILL.md 面** + **CI 可移植性面** + **Web 页面 JS 读面** + **第三方客户端消费面（两个官方实现）** 六份文档面全部有门；**「手抄实现细节」残留**已治到十四个面；**「真相层完整性」**（第十五个面）与**「跨面同一事实（同一份上限、同一份回执）」**各为一道被门钉住的面。

## 当前状态（2026-10-23 · **T39「写侧大载荷」——同一个动作（上传一份原件）在 REST 与 MCP 两个面上有两个不同的上限，且 MCP 宣称的 32MB 根本不可达（实测 400）；把「上限」收成一份事实存进 `AppState`，并在能力清单里公布，超限语义从 400 改成 413** · D-056 落地**）

- ✅ **T39-① 缺口定位**：PLAN 从 T35 起挂着同一条未销账项——写侧「多轮/并发/大载荷」三维。T38 关掉「并发」；本轮关「**大载荷**」。
- ✅ **T39-② 干跑在前（先抓缺陷再改代码）**：动手前用一次性干跑量出两个面的**真实**上限 ⇒ **抓到三个真缺陷**（非推演）：
  - ①**跨面不一致**：REST multipart 落进 **axum 默认 body limit ≈ 2.1MB**（隐式、未公布），MCP 工具层自己判 **32MB** ⇒ 同一动作，REST 拒得比 MCP 严 15 倍；
  - ②**MCP 宣称的上限不可达**：base64 体长 2_666_668 字节 ⇒ HTTP 层直接 **400「length limit exceeded」** ⇒ 那个 32MB 是**假的**；
  - ③**超限的错误语义误导**：超限报 **400「bad multipart / invalid JSON body」** ⇒ 调用方以为自己格式写错了 ⇒ 去改格式重试 ⇒ 永远失败。
- ✅ **T39-③ 结构决策①：上限收成「一份事实」，住在 `AppState.max_upload_bytes`**。上限被**四处**读（路由 body limit / REST handler 判据 / MCP 工具判据 / 能力清单公布值）⇒ 各读各的 env 会让「启动后改 env」导致清单**说谎** ⇒ 改为**启动期解析一次 → 存进状态 → 全进程读这一处** ⇒ 四处**由构造保证相等**。⇒ 纪律：**env 是启动期配置**，要改就重启。
- ✅ **T39-④ 结构决策②：清单新增 `max_upload_bytes` 字段，公布「当前真实生效值」**（此前 REST 的上限既没常量也没公布，调用方只能「先传超限文件再看 413」）。⇒ `capability_manifest(max_upload_bytes)` 改成**必填参数** ⇒ 没有无参版 ⇒ 忘了传会**编译不过**。
- ✅ **T39-⑤ 结构决策③：超限语义 400 → 413**（multipart 与 MCP JSON 两面）⇒ 报文**带出上限数值**并指路 `GET /api/v1/capabilities`；其余错（语法错/缺 content-type）**仍 400**，不误伤既有语义。
- ✅ **T39-⑥ 结构决策④：把上限做成可注入**（`AppState::from_home_with_limit`）⇒ 门禁**不必传 32MB 真文件、不必起子进程、不必手工编 rustc** ⇒ 纯库级判据、**0.58s**、**不会假红**。
- ✅ **T39-⑦ 证据链**：
  - ①**参数生效有证据**：`EHR_MAX_UPLOAD_BYTES=4096` 起真服务 ⇒ `GET /api/v1/capabilities` ⇒ **`max_upload_bytes = 4096`**（**参数确实生效且被公布**）⇒ 传 1KB `.txt` ⇒ **200** + 回执含真 CAS 哈希 `f77f722d…`（原件真进 CAS）⇒ 传 8KB ⇒ **413**「upload too large: 8192 bytes exceeds limit 4096 bytes」⇒ 读面 `GET /persons` 仍 **200**（超限不污染服务）。
  - ②**两面同参**：MCP `get_capabilities` 返回 **4096**（与 REST **逐字相等**）⇒ MCP `upload_document` 传 8KB ⇒ `isError=true` + 「decoded size 8192 exceeds limit 4096 bytes」。
  - ③**库级门 `ehr-api/tests/upload_limits.rs`（6 条全绿）**：清单/状态/注入值**三重相等** · HTTP 面 == 库级清单**逐字相等** · REST 低限真落地+真 CAS、超限 **413** 带上限数值 · MCP 超限**不是 400** · **两面同参** · 非法 env ⇒ 如实回落缺省且**缺省非零够大**（防「回落=全线拒收」的假绿）⇒ **fails=0**。
  - ④**Rust 侧零破坏**：`cargo test --all-targets --locked` ⇒ **94 门全绿**（原 88 + 新 6）；`Cargo.toml`/`Cargo.lock` **未改动** ⇒ 零新依赖。
  - ⑤**产物新鲜度**：改了 Rust 源码 ⇒ 重编译 debug+release（**local + shared 两份**，D-029）⇒ `artifact-freshness-check.sh` **fails=0**。
  - ⑥**文档面同步（本轮真撞到三处漂移）**：清单多一个字段 ⇒ **三道文档门同时红**（`mcp-contract-check` / `skill-doc-field-check` / `doc-contract-check`，各报「真响应有但文档未提及的字段: ['max_upload_bytes']」）⇒ 补齐三处文档（`tool-contracts.md` · `SKILL.md` 字段表 · `rest-contracts.md`）⇒ 三门转绿 ⇒ ⇒ 纪律：**改一个共享响应结构 ⇒ 所有承诺它的文档面都要同步**（这次**由门机器守住**，没漏）。
  - ⑦**统一入口全量复跑：24 门全绿，总 fails=0**（`run-all-gates.sh` ⇒ A1 PASS + 24 个 `[PASS] … fails=0`；末行「门禁数=24（本次执行 24）总 fails=0」）⇒ 覆盖面**不变**（本轮是**加固**既有面，不是新面）。
  - ⑧**后台卡死检查（D-010）**：**无 cargo/rustc 残留**；本轮起的临时 ehr-api（`:18777`）已收割；常驻只剩两个 **docker 容器**内的 ehr-api ⇒ **非卡死**；无 done 哨兵残留。
- 🔄 **下一步**：①**写侧剩余一维：「多轮」**（同一会话连续多次建档/上传）⇒ 下一轮关它；②**MCP 写侧的「并发」门**仍未做（T38 修了 `store.create()` ⇒ MCP 面同时被修好，但**没有门**钉住）；③**T5 Web 管理端正式化**（目标验收①最后一块，**选型先报方案再动手**）；④官方 SDK 的 Java 实现仍未验；⑤公网暴露前 `EHR_BIND_ADDR` 改 `0.0.0.0` 前**必须**先设 token；⑥ghcr 包可见性仍 private；⑦`.dcm`/DICOM worker 未做；⑧上游分类缺口仍在；⑨人员域三端点仍 501 且刻意不进 OpenAPI；⑩MCP 侧**仍无 prompts**（已显式钉住「不提供」）；⑪**本轮改动尚未在真 GitHub runner 上验过**（改了 Rust 源码 + 改了 REST 契约面 ⇒ 需一次 push；`docs/API.md` 是生成物，CI 上需重生成并跑漂移门）。
- ⚠️ 未销账：目标验收①**Web 管理端仍是单文件最小版**；`docs/API.md` 是**生成物**；文档覆盖 **REST 面** + **MCP 面** + **SKILL.md 面** + **CI 可移植性面** + **Web 页面 JS 读面** + **第三方客户端消费面（两个官方实现）** 六份文档面全部有门；**「手抄实现细节」残留**已治到十四个面；**「真相层完整性」**与**「跨面同一事实（同一份上限）」**各为一道被门钉住的面。

## 当前状态（2026-10-22 · **T38「person registry 并发写会把真相层打坏」——真抓到并修掉一个**不可自愈**的真相层缺陷（60 并发建档 ⇒ 29/59 行交错垃圾 ⇒ 此后**所有**读面永久 500），并给真相层补上「整行一次写 + 进程内互斥 + 末行在途可跳过」的机制与两道门** · D-055 落地**）

- ✅ **T38-① 缺口定位（PLAN/T37 未销账项③原话）**：「**写侧（`create_person`/`upload_document`）两轮各只调 1 次** ⇒ 未做多轮/并发/大载荷」。事实核对：写侧确实**从未被并发打过** ⇒ 门的覆盖面上有个洞。⇒ 本轮按 PLAN 的方向先做「**并发**」这一维（多轮/大载荷留待后轮）。
- ✅ **T38-② 先干跑再动手（D4「参数生效要有证据」）**：本轮**不先改代码**，而是**先用一次性干跑把缺陷打出来**（起临时 ehr-api ⇒ 60 个并发 `POST /api/v1/persons`）⇒ **当场抓到真缺陷，不是推演**：
  - 真相层 `registry/persons.jsonl`：**59 行里 29 行是交错垃圾**（形如 `{"op":"create",...,"data":{"name":"并发-5",{""birthdayop""::null,create""gen` ⇒ 半行 + 半行拼在一起）；
  - 后果**不是**「返回码错」而是**真相层被写坏**：此后 `GET /api/v1/persons`、`/persons/{pid}/timeline`、审计视图等**所有**读面**永久 500**（实测 `{"error":"...persons.jsonl:7 corrupt registry line: key must be a string..."}`）⇒ **不可自愈**（只有人工去修 JSONL）⇒ 「真相层是唯一真相」这条不变量**当时是假的**。
- ✅ **T38-③ 根因（不是「环境问题」，是**写法**问题）**：`PersonStore::append()` 用 `serde_json::to_writer(&mut f, ev)` 往真相层文件写 ⇒ 那**不是一次写**，而是**几十次小 write syscall**（键、值、逗号逐段吐）⇒ 两个并发 append 的这些小写**交错落盘** ⇒ 行被拼碎。⇒ 关键洞察：**该函数**完全没有加锁**（per-person 的 `vaults::LockToken` 只护 vault 事件日志，**护不到 registry**）⇒ 这是**两个独立失效模式**（见 T38-④）。
- ✅ **T38-④ 结构决策：修法是**两件独立**的事，缺一不可**：
  - ① **整行一次写**：先把整行序列化成 `Vec<u8>`（`to_vec`），再**一次 `write_all`** ⇒ O_APPEND 模式下**一次 write syscall 的内容不会与别家交错**（内核在同一��写入里把整段追加到文件尾）⇒ 行不再破碎，**跨进程**并发也受保护；
  - ② **进程内互斥**：全局一把 `APPEND_LOCK`（**不是**每文件一把，因为 registry 是**全库共享**的那一个文件）⇒ 临界区覆盖「序列化 + 写入」整段 ⇒ 本进程内的并发 append 互斥。
  - ⇒ 纪律（记入 DECISIONS）：**「锁」与「写粒度」是两个不同的失效模式**——只加锁不治写粒度 ⇒ 跨进程仍会写碎；只治写粒度不加锁 ⇒ 本进程内仍可能读到「写了一半」的尾行 ⇒ **两者必须分别取证、分别修**。
- ✅ **T38-⑤ 读侧必须能区分「**在途**」与「**坏了**」**（本轮第二个结构决策）：写侧改成「整行一次写」后，**行内**不再破碎，但**跨进程**并发时读侧仍可能正好读到「写了一半的最后一行」（末尾没有 `\n`）⇒ **「末行不完整」≠「真相层被写坏」** ⇒ 两者必须**分开判**：
  - **末行不完整**（无结尾 `\n`）⇒ 说明有并发写在途 ⇒ **跳过该行**并打 `warn`（可观测，不静默）；
  - **完整行**解析失败 ⇒ 那是**真坏了** ⇒ **必须报错**（绝不「跳过坏行」——否则真相层坏了却装作没事 ⇒ D-050「派生的期望值不得为空」同族的反面：**掩盖比报错更坏**）。
  - ⇒ 新增 `PersonStore::complete_lines()`（**唯一一份**「怎么切行」的实现 ⇒ 读侧 `list()` 与 `events()` 共用 ⇒ D2「一个语义只允许一处实现」）。
- ✅ **T38-⑥ 上游同族核查（确认这个 bug 是**我们独有**的，不是继承来的）**：查 `upstream-medme/packages/core-model/src/log.rs` 的事件日志 append ⇒ 它用 `to_string` + `writeln!`（**整行一次写**）且读侧**容忍** torn 末行 ⇒ **上游没有这个 bug** ⇒ 本项目 registry 层是**独立引入**的 ⇒ 修它不需要动上游（也**不该**动上游，`upstream-medme/` 是只读参考）。
- ✅ **T38-⑦ 机制落地的证据链（缺一即算交付）**：
  - ①**修后复跑同一份干跑**（**同一份命令**，只换二进制）⇒ **60 行全完整、0 损坏、60 个 id 全在、读面 200 且 60 条** ⇒ 修法有效（**不是**换了命令才绿）。
  - ②**新门 `registry-concurrency-e2e.sh`（真进程 + 真 HTTP）**：C1 真相层每行都是完整 JSON（**直接验落盘字节**，不看 API 的说法）· C2 行数 == 并发数（无半条/丢条）· C3 读面仍 200 且条数 == 并发数（**不**因并发写而永久 500）· C4 落盘 id 集合 == 建档返回 id 集合（无重复、无丢）⇒ **fails=0**。
  - ③**新门是「真门」的负向证据（实撞 2 次，每次复原）**：把修法退回旧的 `to_writer` 逐段写 ⇒ **红** `FAIL 30/60 行是交错/损坏的垃圾（真相层被并发写打坏）` + `FAIL 读面不可用（并发写把真相层打坏了？）: <HTTPError 500>` ⇒ **fails=2** ⇒ 复原 ⇒ **fails=0** ⇒ ⇒ **新门精确指认了那一个缺陷**。
  - ④**库级门 `ehr-api/tests/registry_concurrency.rs`（4 条判据）**：`concurrent_creates_leave_registry_intact`（64 线程并发 ⇒ 每行完整 + `list()` 条数正确 + 每个建档 id 都在读面）· `concurrent_http_creates_keep_read_surface_alive`（真 HTTP 32 并发）· `torn_tail_line_is_skipped_not_fatal`（**正例**：末行不完整 ⇒ 跳过它，其余照常）· `corrupt_complete_line_is_fatal`（**反例**：完整行坏 ⇒ **必须报错**）⇒ 后两条是**双向对照**（少任一条，门就没有判别力）⇒ **4 门全绿**。
  - ⑤**库级门的负向证据（实撞 1 次，已复原）**：退回旧写法 ⇒ **红 fails=2**，且失败信息**指名道姓**：`registry must contain only intact JSON lines; 49 of 62 lines are interleaved/corrupt`（**先报「哪一行坏了」再报「行数不对」**——行数只是症状，坏行才是病灶）⇒ 复原 ⇒ **4 门全绿**。
  - ⑥**Rust 侧零破坏**：`cargo test --all-targets --locked` ⇒ **88 门全绿**（原 84 门 + 本轮新 4 门）；`Cargo.toml`/`Cargo.lock` **未改动** ⇒ 零新依赖。
  - ⑦**产物新鲜度**：改了 Rust 源码 ⇒ 重编译 debug+release（**local target 与 shared target 两份**，D-029 纪律）⇒ 同步（`cp .new && mv` 换 inode，绕开 D-049 的 `Text file busy`）⇒ `artifact-freshness-check.sh` **fails=0**（两份产物 sha256 逐字一致：release `80063e47…`）。
  - ⑧**统一入口全量复跑：24 门全绿，总 fails=0**（`run-all-gates.sh` ⇒ A1 PASS + 24 个 `[PASS] … fails=0`；末行「门禁数=24（本次执行 24）总 fails=0」）⇒ 覆盖面净增一道真跑的门（23 → 24）。
  - ⑨**可移植性门覆盖到新载体**：`ci_portability_check.py` 的 `NEEDS_ENV` 增 `registry-concurrency-e2e.sh`（它起真进程 ⇒ 必须经唯一来源取产物路径）⇒ **fails=0**。
- ✅ **T38-⑧ 门自身的两个真 bug（当场抓出并修，**都是「假弱」不是假绿**）**：
  - ①**裸 `wait` 会把门挂死**（首跑即被超时打断）：裸 `wait` 等**所有**后台作业 ⇒ 包括那个**一直活着的 ehr-api 服务** ⇒ 门永远不返回 ⇒ 修法：**只收集 curl 的 PID**，逐个 `wait`（**服务不能被 wait 到**）。
  - ②**并发 `>>` 追加到同一个文件 ⇒ 判据变假弱**：各 curl 的响应会互相交错 ⇒ 拼出来的行不是合法 JSON ⇒ 「建档返回的 id 集合」**只收到 15/60** ⇒ 那条判据**看着在核对、实际大部分没核到**（**假弱**：不报错但失去判别力）⇒ 修法：**一请求一文件**（零交错）⇒ 修后 `created_ids=60/60` ⇒ 判据才**真的**在核对。
  - ⇒ 纪律（记入 DECISIONS）：**一道门自己也可能「不报错但没在核对」** ⇒ 判据的**强度**要单独看（比值/覆盖率），不能只看 `fails=0`。
- ✅ **T38-⑨ 后台卡死检查（D-010）**：**无 cargo/rustc 残留**；门起的临时 ehr-api（`:18696`）由脚本 `trap` 自清 ⇒ 常驻只剩两个 **docker 容器**内的 ehr-api ⇒ **非卡死**；无 done 哨兵残留、无 log 错误行。（另：本轮开工时发现一个 **10-22 20:01 遗留**的临时 ehr-api（`:18971`）——是此前一轮的手工干跑留下的 ⇒ 已收割。）
- 🔄 **下一步**：①**T5 Web 管理端正式化**（目标验收①最后一块，**选型先报方案再动手**）；②**写侧剩余两维仍未做**：「**多轮**」（同一会话连续多次建档/上传）与「**大载荷**」（大文件上传）⇒ 本轮只关掉「并发」这一维；③**MCP 写侧同一条路**：`create_person` 走**同一** `store.create()` ⇒ 本轮修复**同时**覆盖 MCP 面（但 MCP 面**尚无**并发门 ⇒ 可考虑复用本门）；④官方 SDK 的 Java 实现仍未验（按 D-053 新坑①，≥2 个实现这条已达成，但「全部官方实现」尚未）；⑤公网暴露前 `EHR_BIND_ADDR` 改 `0.0.0.0` 前**必须**先设 token（当前 compose 容器日志 `auth=open(tokens=0) ⚠️未配置 token，所有请求放行` —— **仅本地**）；⑥ghcr 包可见性仍 private（OCR 镜像未推 GHCR、未纳入 CI）；⑦`.dcm`/DICOM worker 未做；⑧上游分类缺口仍在；⑨`docs/API.md` 是**生成物**（本轮**未改 REST 接口** ⇒ 无需重生成）；⑩人员域三端点仍 501 且刻意不进 OpenAPI；⑪MCP 侧**仍无 prompts**（已显式钉住「不提供」——两套官方 SDK 上都是机器判据）。
- ⚠️ 未销账：目标验收①**Web 管理端仍是单文件最小版**（编译期内嵌，无构建链）；`docs/API.md` 是**生成物**；文档覆盖 **REST 面** + **MCP 面** + **SKILL.md 面** + **CI 可移植性面** + **Web 页面 JS 读面** + **第三方客户端消费面（两个官方实现）** 六份文档面全部有门；**「手抄实现细节」残留**已治到十四个面；**「真相层完整性」本轮新增为第十五个面**（并发写 ⇒ 真相层会不会被打坏）；**「真相层是唯一真相」这条不变量本轮从「口头声称」升级为「有门钉住」**（D-055）。

## 当前状态（2026-10-22 · **T37「T35/T36 两道第三方互通门在**真 GitHub runner** 上验过」——把「本机绿」升级为「CI 上也绿」，并首次在真 runner 上实测 **npm(npmmirror) / pip(TUNA) 两条依赖通道**（此前只在配方里，未实测）· D-054 落地**）

- ✅ **T37-① 缺口定位（PLAN/T36 未销账项④原话）**：「**T35/T36 两道新门尚未在真 GitHub runner 上验过**（含 **npm 通道** —— npmmirror 已在配方内但**未在真 runner 上实测**）；Python 面的 venv 退化路径同」。事实核对：T35/T36 两道门**已入库**（`mcp-interop-e2e.sh` / `mcp-interop-ts-e2e.sh`，均进 RUN 清单），但**从未被真 runner 执行过** ⇒ 「官方 SDK 能吃」这条结论**目前只有本机证据**（本机有代理 7890、有 TUNA 直连、有已缓存的 venv/node_modules）⇒ **CI 上很可能因通道/缺包而红**，而**本地绿 ≠ CI 绿**正是 D-033/T33 记录过的同族坑（「把门转正却没给 CI 补前置」）。
- ✅ **T37-② 结构决策：本轮**不改任何产品代码**，只做「把已入库的门推到真 runner 上跑」**。理由：本轮的验收目标是**通道与门在真环境下的可复现性**（第三方 SDK 依赖面），而**不是**产品行为 ⇒ 改 Rust 源码会引入**与目标无关的变量**（还会强制重建产物、拉长 runner 时间）⇒ 纪律：**验证类的小步优先选「零代码改动」的那一步**，让失败原因唯一（要么通道红，要么门红）。
- ✅ **T37-③ 干跑在前（本地先绿，再上真 runner）**：本地复跑两道门 ⇒ **Python 面 17 条判据全 PASS、fails=0**（`I1`~`I6`，含 `I5.read_resource_equals_call_tool` 跨面逐字相等、`I6` 两码 `-32005`/`-32001` 为 `McpError`）⇒ **TS 面 16 条判据全 PASS、fails=0**（`T1`~`T6`，含 `T5` 逐字相等、`T6` 两码）⇒ 确认「门本身可跑通」后，才把**同一份 commit**推上去 ⇒ 真 runner 上的红，就只能是**通道/环境**问题，不是**门本身**问题。
- ✅ **T37-④ 真·runner 验证（run `39767572283`，sha `2803f54`）= 全绿 ✅**：13 个 step **全部 success**（`checkout`/装 Rust/装 Node/拉上游/依赖通道/构建 debug+release/`cargo test`/同步产物/构建容器镜像/起 compose 容器/统一门禁入口），末步「门禁失败时打印每道门的原始输出」= **skipped** ⇒ 门全绿时兜底步**正确地不跑**（D-047/D-048 同款对照）。日志实锤：
  - `== A1 登记表完整性 ==` ⇒ **`[PASS] A1 登记表完整（RUN=23 SKIP=1）`**（新门确实进了 RUN 清单，不是漏跑）；
  - `---- mcp-interop-e2e.sh` ⇒ **`[PASS] mcp-interop-e2e.sh fails=0`**（runner 上从零装 TUNA pypi 的 `mcp==1.30.0` ⇒ **TUNA pypi 通道实测可用**；该步耗时 51s）；
  - `---- mcp-interop-ts-e2e.sh` ⇒ **`[PASS] mcp-interop-ts-e2e.sh fails=0`**（runner 上从零装 npmmirror 的 `@modelcontextprotocol/sdk@1.32.1` ⇒ **npmmirror 通道实测可用**；该步耗时 29s）；
  - 末行 ⇒ **`[证据] run-all-gates.sh 门禁数=23（本次执行 23）总 fails=0`** ⇒ 23 门在真 runner 上全绿。
  - **回写 commit 亦在真 runner 上复验**：PLAN/DECISIONS 回写（`a0ab2ae`）⇒ run `39769207268` **同样全绿**（13 步全 success、兜底步 skipped）⇒ **文档回写没有把门禁打红**（`doc-contract`/`api-doc`/`ci-portability` 三门本地也绿）。
- ✅ **T37-⑤ 通道结论升级为「实测」而非「配方」**：D-004~D-007 的通道配方此前**从未在真 runner 上跑过**（PLAN 反复写「未在真 runner 上实测」）⇒ 本轮起，**npmmirror（TS SDK）与 TUNA pypi（Python SDK）两条通道在真 runner 上各实测成功一次**（门自带 fallback：主通道失败退官方 registry/pypi ⇒ 通道单点失效不会把门打红）。
- ✅ **T37-⑥ Rust 侧零破坏**：本轮**未改任何 Rust 源码**、**未改 `Cargo.toml`/`Cargo.lock`** ⇒ 零新依赖；runner 上 `cargo test --all-targets --locked` 步 **success**（含 84 门）。
- ✅ **T37-⑦ 产物新鲜度与同步**：runner 的「同步产物（local target ⇄ shared target）」步 **success** ⇒ `artifact-freshness-check.sh` 在 runner 上绿 ⇒ 两份产物 sha256 逐字一致这条纪律**在真环境成立**。
- ✅ **T37-⑧ 后台卡死检查（D-010）**：**无 cargo/rustc 残留**；本地跑完两道门后临时 ehr-api 由脚本 `trap` 自清（`mcp-interop-e2e.sh` 起 `:18695`、`mcp-interop-ts-e2e.sh` 起 `:18696`，均已收割）⇒ 常驻只剩两个 **docker 容器**内的 ehr-api（`:8443` 端口侧）⇒ **非卡死**；无 done 哨兵残留、无 log 错误行。
- 🔄 **下一步**：①**T5 Web 管理端正式化**（目标验收①最后一块，**选型先报方案再动手**）；②**官方 SDK 的 Java 实现仍未验**（按 D-053 新坑①，≥2 个实现这条已达成，但「全部官方实现」尚未）；③**写侧（`create_person`/`upload_document`）两轮各只调 1 次** ⇒ 未做多轮/并发/大载荷；④公网暴露前 `EHR_BIND_ADDR` 改 `0.0.0.0` 前**必须**先设 token（当前 compose 容器日志 `auth=open(tokens=0) ⚠️未配置 token，所有请求放行` —— **仅本地**）；⑤ghcr 包可见性仍 private（OCR 镜像未推 GHCR、未纳入 CI）；⑥`.dcm`/DICOM worker 未做；⑦上游分类缺口仍在；⑧`docs/API.md` 是**生成物**（本轮**未改 REST 接口** ⇒ 无需重生成）；⑨人员域三端点仍 501 且刻意不进 OpenAPI；⑩MCP 侧**仍无 prompts**（已显式钉住「不提供」——两套官方 SDK 上都是机器判据）。
- ⚠️ 未销账：目标验收①**Web 管理端仍是单文件最小版**（编译期内嵌，无构建链）；`docs/API.md` 是**生成物**；文档覆盖 **REST 面** + **MCP 面** + **SKILL.md 面** + **CI 可移植性面** + **Web 页面 JS 读面** + **第三方客户端消费面（两个官方实现）** 六份面全部有门；**「手抄实现细节」残留**已治到十四个面；**「只被自家 curl 覆盖的协议面」已升级为「必须被官方 SDK 消费才算验收」（且从 1 个实现扩到 2 个官方实现）**（D-052 新坑① + D-053 新坑①）；**T35/T36 两道门的「真 runner 可复现」本轮关闭**（D-054）。

## 当前状态（2026-10-22 · **T36「真·第三方 MCP 客户端互通（TypeScript 官方 SDK 面）」——把「官方 SDK 能吃」从一个实现的结论升级为**两个官方实现的结论**（Python pydantic + TS zod/v4 两个独立解析器），并给 TS 面补一道同形门；实测**两侧判据都抓得住**同一个真缺陷（TS 侧报 `$ZodError`、Python 侧报 `ValidationError`）· D-053 落地**）

- ✅ **T36-① 缺口定位（D-052 未销账项③的原话）**：「**真·第三方 MCP 互通本轮只覆盖 Python SDK**（官方另有 TypeScript/Java 实现 ⇒ 未验）」。事实核对：官方 SDK 有 **两个独立实现**（Python `mcp` / TS `@modelcontextprotocol/sdk`）⇒ **解析器不同**（Python=pydantic v2，TS=zod/v4）⇒ 「服务端形状不合规范」这件事，**一个实现拒收、另一个实现可能放行**是可能的 ⇒ 只验一个实现 ⇒ **不能声称「协议面合规」**，只能说「Python 面合规」。
- ✅ **T36-② 结构决策：TS 面判据与 Python 面**同形**（T1~T6 ↔ I1~I6），便于双面对照**。六条：T1 握手可被官方 zod 模型解析（**显式断言 `prompts` 不被宣告**）· T2 工具数 == 干跑清单派生 · T3 `tools/call` 真跑通（探针**自己造人**⇒ 空库不恒绿）· T4 资源/模板数 == 干跑清单派生 · T5 **跨面不变量在 TS 客户端上复现**（`readResource` 的 text 与 `callTool` 的 `structuredContent` 逐字相等）· T6 **错误语义可判读**（必须 `McpError` + 正确 `code`；**任何其它异常 = 客户端崩溃 ⇒ 红**）。
- ✅ **T36-③ 官方 TS SDK 仍是**门禁侧**依赖，不是产品依赖**（D-016/D-052 同形）⇒ 装到门禁专用目录 `$HOME/.cache/medme-ehr-mcp-ts` ⇒ **不进仓库、不进 package.json、不进产品镜像** ⇒ 版本钉死（`@modelcontextprotocol/sdk@1.32.1`）⇒ 门禁行为可复现 ⇒ 通道 **npmmirror 优先、失败退官方 registry.npmjs.org**。
- ✅ **T36-④ 探针**放在门禁依赖目录里跑**（ESM 依赖解析需要 `node_modules` 在其上级目录）⇒ 仓库里只留一份 `.mjs` 源（`tools/mcp_interop_ts_probe.mjs`）⇒ **仓库不留 `node_modules`**。
- ✅ **T36-⑤ 一处**同形差异**已记明：TS 侧「不提供 prompts」表现为属性缺省（`undefined`）**，Python 侧是 `None` ⇒ 探针里做归一（`?? null`）⇒ **两边判据同义**，不因运行时差异而假红。
- ✅ **T36-⑥ 门是「真门」的负向证据（实撞 1 次，已复原）**：把 `resources/read` 的失败改回 T35 之前的旧形状（`result.isError=true` + `_code`）⇒ 重编译 ⇒ **两面同时红**：TS 面 `FAIL T6.unknown_uri.raises_mcperror: got="$ZodError" want="McpError"`（fails=2），Python 面 `FAIL I6.unknown_uri.raises_mcperror: got='ValidationError' want='McpError'`（fails=2）⇒ 复原 ⇒ 两面各 fails=0 ⇒ **两个解析器各自独立地拒绝了这个形状**（判别力来自第三方权威，不是自家约定）。
- ✅ **T36-⑦ Rust 侧零破坏**：`cargo test --all-targets --locked` ⇒ **84 门全绿**（16 个 test target；mcp.rs 仍 16 门）⇒ **未改任何 Rust 源码**（本轮只动门禁脚本与探针）⇒ `Cargo.toml`/`Cargo.lock` **未改动** ⇒ 零新依赖。
- ✅ **T36-⑧ 统一入口全量复跑：23 门全绿，总 fails=0**（`run-all-gates.sh` ⇒ A1 PASS + 23 个 `[PASS] … fails=0`；末行「门禁数=23（本次执行 23）总 fails=0」）⇒ 覆盖面净增一道真跑的门（22 → 23）。
- ✅ **T36-⑨ 可移植性门覆盖到新载体**：`ci_portability_check.py` 的 `NEEDS_ENV` 增 `mcp-interop-ts-e2e.sh` ⇒ 该门也起真进程 ⇒ 必须经唯一来源取产物路径 ⇒ **fails=0**。
- ✅ **T36-⑩ Python 面的依赖通道也做了加固**（顺手补的**真**风险点）：`python3 -m venv` 在 CI runner 上常因缺 `python3-venv` 失败 ⇒ 门会「装不上依赖 ⇒ 直接红」⇒ 已加**可复现的退化路径**（`--target` 目录 + `PYTHONPATH`）⇒ 依赖面拿不到时**不直接红**，而是走等价可复现路径（D-050 家族）。
- ✅ **T36-⑪ 产物新鲜度门本轮红过一次（门自己的正确行为）**：改了 `src/mcp.rs`（负向注入）⇒ local target 产物比源码旧 ⇒ 门红（fails=2）⇒ 按 CI 配方在 local target 重建 debug+release 并同步（`cp .new && mv` 换 inode）⇒ 转绿 ⇒ **这次红是门自己的正确行为**（D-011 家族的反例）。
- ✅ **T36-⑫ 后台卡死检查（D-010）**：**无 cargo/rustc 残留**；临时 ehr-api 由门禁 `trap` 自清；常驻只剩 docker 容器 ⇒ **非卡死**。
- 🔄 **下一步**：①**T5 Web 管理端正式化**（目标验收①最后一块，**选型先报方案再动手**）；②**真·第三方客户端互通现覆盖 Python + TypeScript 两个官方实现**（**Java 实现仍未验**；**只读面**已覆盖，**写侧**两轮各调 1 次，未做多轮/并发/大载荷）；③**T35/T36 两道新门尚未在真 GitHub runner 上验过**（含 npm 通道 —— npmmirror 已在配方内但**未在真 runner 上实测**）；④公网暴露前 `EHR_BIND_ADDR` 改 `0.0.0.0` 前**必须**先设 token；⑤ghcr 包可见性仍 private（OCR 镜像未推 GHCR、未纳入 CI）；⑥`.dcm`/DICOM worker 未做；⑦上游分类缺口仍在；⑧`docs/API.md` 是**生成物**（本轮**未改 REST 接口** ⇒ 无需重生成）；⑨人员域三端点仍 501 且刻意不进 OpenAPI；⑩MCP 侧**仍无 prompts**（已显式钉住「不提供」——两套官方 SDK 上都是机器判据）。
- ⚠️ 未销账：目标验收①**Web 管理端仍是单文件最小版**（编译期内嵌，无构建链）；`docs/API.md` 是**生成物**；文档覆盖 **REST 面** + **MCP 面** + **SKILL.md 面** + **CI 可移植性面** + **Web 页面 JS 读面** + **第三方客户端消费面（两个官方实现）** 六份面全部有门；**「手抄实现细节」残留**已治到十四个面；**「只被自家 curl 覆盖的协议面」已升级为「必须被官方 SDK 消费才算验收」（且从 1 个实现扩到 2 个官方实现）**（D-052 新坑① + D-053 新坑①）。

## 当前状态（2026-10-22 · **T35「真·第三方 MCP 客户端互通」——用官方 MCP Python SDK 当客户端，当场抓出 resources/read 的失败信封不合规范（官方 SDK 抛 `ValidationError` 崩溃 ⇒ 三个错误码白设计），修后补一道可复跑的第三方互通门 · D-052 落地**）
- ✅ **T35-① 缺口定位（PLAN 连续多轮列着的同一条未销账项）**：「**真·第三方 MCP 客户端互通仍未验**（含 resources 面的真客户端消费）⇒ 优先级上升」。事实核对：本仓库的 `mcp-e2e.sh` / `mcp-resources-e2e.sh` **全部是自家写 curl 调自家服务** ⇒ 只能证明「本实现自洽」，**证不了「官方 SDK 能不能吃」** ⇒ 一个只被自家 curl 门覆盖的协议面，**可能整族不合规范而门禁全绿**（D-047 同族风险，这次在协议面）。
- ✅ **T35-② 结构决策：第三方客户端 = 官方 SDK（不是「另一个 curl」）**。选 **官方 MCP Python SDK**（`mcp==1.30.0`，版本钉死）⇒ 走 `streamablehttp_client` + `ClientSession` ⇒ **形状判定由官方 pydantic 模型做** ⇒ 服务端不合规范时官方模型直接拒收 ⇒ 判别力来自第三方权威，不是自家约定。
- ✅ **T35-③ 本轮真抓到缺陷（非推演）：`resources/read` 的失败信封不合 MCP 规范**。T34 落地 resources 面时失败沿用了 tools/call 的形状（`result.isError=true` + `result._code`）⇒ 实测官方 SDK 消费 ⇒ **抛 `ValidationError`（不是 `McpError`）**：`1 validation error for ReadResourceResult / contents Field required` ⇒ **宿主 agent 拿到的是「客户端崩溃」**，`-32005`/`-32001`/`-32602` 三个错误码**白设计了**。⇒ 修法**不在客户端，在服务端信封**：规范里 `resources/read` 的失败是**协议级错误** ⇒ 改回 JSON-RPC `{"error":{"code","message"}}`（**不带** `isError`）。
- ✅ **T35-④ 两个面的错误信封**故意不同**（各有规范依据，不可统一）：`tools/call` 失败 = **工具执行失败** ⇒ `result.isError=true`；`resources/read` 失败 = **协议级错误** ⇒ `error` 对象 ⇒ 已在 `tool-contracts.md` 写明差异**及其依据**。
- ✅ **T35-⑤ 错误码从 `_code`（非规范字段）搬到 `error.code`（规范字段）后，错误码第一次真的对第三方可见** ⇒ 协议面错误码的验收标准升级为「**能在官方 SDK 里被断言**才算数」。
- ✅ **T35-⑥ 新门 `mcp-interop-e2e.sh` + `tools/mcp_interop_probe.py`**（真进程、真实数据、零 mock），判据六条：**I1** 握手可被官方模型解析（含**显式断言 `prompts is None`**——把「不提供 prompts」从注释钉成机器判据）· **I2** 工具数 == 干跑清单派生 · **I3** `tools/call` 真跑通（探针**自己造人**⇒ 空库不恒绿）· **I4** 资源数/模板数 == 干跑清单派生 · **I5** **跨面不变量在第三方客户端上复现**（`read_resource` 的 text 与 `call_tool` 的 `structuredContent` 逐字相等）· **I6** **错误语义可判读**（必须 `McpError` + 正确 `code`；**任何其它异常 = 客户端崩溃 ⇒ 红**）。
- ✅ **T35-⑦ 官方 SDK 是门禁侧依赖，不是产品依赖**（D-016 同形：构建期零外网/依赖面最小不破）⇒ 不进 `Cargo.toml`/`Cargo.lock`/产品镜像 ⇒ 按需装到**门禁专用 venv**（`$HOME/.cache/medme-ehr-mcp-venv`，不进仓库）⇒ 通道 **TUNA pypi 优先、失败退官方 pypi**、版本钉死 ⇒ 依赖**可离线复现** ⇒ 按 D-048 分层，**该门进 RUN 清单**（不是 SKIP）。
- ✅ **T35-⑧ 既有门同步，且「同步」本身被机器守住**：`mcp-resources-e2e.sh` 的 R4（读 `result._code`）当场红（fails=3）⇒ 改读 `error.code` + **新增形状纪律**「失败响应**不得**带 `isError` 残留」⇒ Rust 侧补 `tests/mcp.rs::mcp_resources_read_failures_use_jsonrpc_error_envelope`（三码 + 不得带 isError + **对照断言**「tools/call 失败**仍**是 isError」）⇒ **两面信封差异双向钉死**。
- ✅ **T35-⑨ 证据链（缺一即算交付）**：
  - ①**干跑在前**：动手前一次性探针实测「官方 SDK 能不能吃」——`initialize` OK、`tools: 13`、`resources: 3`、`templates: 11`、`call_tool`/`read_resource` 均成功 ⇒ 通道在干净 venv **可复现**（`mcp==1.30.0` 安装 rc=0）。
  - ②**真缺陷实测记录**：注入前 `read_resource("ehr://nope")` ⇒ 抛 **`ValidationError`**（异常文本原样记入决策）。
  - ③**修复后新门全绿**：`./mcp-interop-e2e.sh` ⇒ **16 条判据全 PASS、fails=0**；关键证据 `I1.initialize.prompts_not_declared = None`、`I2.tools_count_matches_impl = 13`、`I4.resources=3 / templates=11`、`I5.read_resource_equals_call_tool`、`I6.unknown_uri.code = -32005` / `I6.unknown_person.code = -32001`（**`McpError` 里的规范 `error.code`**）。
  - ④**门是「真门」的负向证据（实撞 1 次，已复原）**：把失败改回 `tool_err(...)` 旧形状 ⇒ 重编译 ⇒ **红** `FAIL I6.unknown_uri.raises_mcperror: got='ValidationError' want='McpError'`（+1 条同族）⇒ **fails=1** ⇒ 复原 ⇒ **fails=0** ⇒ **新判据精确指认了那一条真缺陷**（其余 14 条仍 PASS ⇒ 不是「整门崩了」）。
  - ⑤**Rust 侧零破坏**：`cargo test --all-targets --locked` ⇒ **84 门全绿**（16 个 test target；mcp.rs 15→16 门）；`Cargo.toml`/`Cargo.lock` **未改动** ⇒ **零新依赖**。
  - ⑥**既有门同步后全绿**：`./mcp-resources-e2e.sh` ⇒ **fails=0**。
  - ⑦**统一入口全量复跑：22 门全绿，总 fails=0**（`run-all-gates.sh` ⇒ A1 PASS + 22 个 `[PASS] … fails=0`；末行「门禁数=22（本次执行 22）总 fails=0」）⇒ 覆盖面净增一道真跑的门（21 → 22）。
  - ⑧**可移植性门覆盖到新载体**：`ci_portability_check.py` 的 `NEEDS_ENV` 增 `mcp-interop-e2e.sh` ⇒ 受检集合 **33 → 36 个**（新增新门 + `tools/mcp_interop_probe.py` + 此前未纳入的 `mcp-resources-e2e.sh`）⇒ **fails=0**。
  - ⑨**文档面同步**：`tool-contracts.md` 的 resources 面「失败形状」段落改写为**错误信封 + 规范依据 + 实测后果（官方 SDK 会崩）**，并写明由两道门守住。
  - ⑩**产物新鲜度**：改了 Rust 源码 ⇒ 重编译 debug+release（**local target 与 shared target 两份**，D-029 纪律）⇒ 同步（`cp .new && mv` 换 inode）⇒ `artifact-freshness-check.sh` **fails=0**（两份 sha256 逐字一致）。
  - ⑪**后台卡死检查（D-010）**：**无 cargo/rustc 残留**；临时 ehr-api 由门禁 `trap` 自清；常驻只剩 docker 容器 ⇒ **非卡死**。
- ✅ **T35-⑫ 一条「门红了不是环境问题」的正例（须记录）**：全量复跑时**产物新鲜度门红过一次**（本机此前一直在 shared target 构建 ⇒ local target 产物旧 ⇒ 「产物比源码旧」+「两份 sha256 不一致」）⇒ 按 CI 配方在 local target 重建并同步后转绿 ⇒ **这次红是门自己的正确行为**（D-011 家族的反例：门红了**不是**环境问题，也不是代码坏了）。
- 🔄 **下一步**：①**T5 Web 管理端正式化**（目标验收①最后一块，**选型先报方案再动手**）；②**真·第三方客户端互通本轮只覆盖 Python SDK**（官方另有 TS/Java 实现 ⇒ 未验；**只读面**已覆盖，**写侧**仅各调 1 次，未做多轮/并发/大载荷）；③公网暴露前 `EHR_BIND_ADDR` 改 `0.0.0.0` 前**必须**先设 token；④ghcr 包可见性仍 private（OCR 镜像未推 GHCR、未纳入 CI）；⑤`.dcm`/DICOM worker 未做；⑥上游分类缺口仍在；⑦`docs/API.md` 是**生成物**（本轮**未改 REST 接口** ⇒ 无需重生成）；⑧人员域三端点仍 501 且刻意不进 OpenAPI；⑨**T35 新门尚未在真 GitHub runner 上验过**（含 Rust 源码改动 ⇒ 需一次 push；**CI 上装官方 SDK 需 pip 通道**——TUNA pypi 已在配方内但**未在真 runner 上实测**）；⑩MCP 侧**仍无 prompts**（本轮**显式钉住「不提供」**——`prompts` 能力不宣告，已成机器判据）。
- ⚠️ 未销账：目标验收①**Web 管理端仍是单文件最小版**（编译期内嵌，无构建链）；`docs/API.md` 是**生成物**；文档覆盖 **REST 面** + **MCP 面** + **SKILL.md 面** + **CI 可移植性面** + **Web 页面 JS 读面** + **第三方客户端消费面（本轮新增）** 六份面全部有门；**「手抄实现细节」残留**已治到十三个面；**「只被自家 curl 覆盖的协议面」本轮升级为「必须被官方 SDK 消费才算验收」**（D-052 新坑①）。

## 当前状态（2026-10-22 · **T34「MCP resources 面落地」——第二个协议面补齐（`resources/list` / `templates/list` / `read`），并配一道**跨面不变量**门（`resources/read` 的内容必须与 `tools/call` 的 structuredContent **逐字相等** ⇒ 一个语义只有一处实现）** · D-051 落地**）
- ✅ **T34-① 缺口定位（PLAN 连续多轮列着的同一条未销账项）**：「**MCP 侧仍无 resources/prompts**」。事实核对：此前 `initialize` 只宣告 `capabilities.tools`，`mcp.rs` 里**没有任何** `resources/*` 方法 ⇒ 该面**根本不存在**（不是「文档没写」，是**协议面缺失**）⇒ 只吃 MCP 的 agent 只能「调动作」，拿不到「按 URI 定位的只读上下文」。
- ✅ **T34-② 结构决策：resources = 只读上下文，**不是**「第二套工具」**。URI 直接定位「某个东西本身」（某人档案 / 某人指标 / 审计视图 / 能力清单）⇒ **只读**（ro token 可读，与读侧工具同权限口径）⇒ **不改真相层**。
- ✅ **T34-③ 结构决策（本轮最关键）：**跨面不变量靠「共享 payload builder」保证，不是靠「两边写得像」**。把 tools 面取数+成形的逻辑抽成**一份** builder（`persons_payload` / `timeline_payload` / `labs_payload` / `medications_payload` / `conditions_payload` / `measurements_payload` / `audit_payload` / `capabilities_payload` / `documents_payload` / `document_detail_payload`）⇒ `tools/call` 与 `resources/read` **都调它** ⇒ D2「一个语义只允许一处实现」在**协议面之间**也成立 ⇒ resources 面**不可能**与 REST 面分叉（两者都走同一批既有领域函数 `vaults::labs_aggregate` / `entries::measurements_projection` / `audit::audit_view` / `capabilities::capability_manifest`）⇒ **零新领域逻辑、零新依赖**。
- ✅ **T34-④ 新增干跑面 `--print-mcp-resources`（机器可读 stdout，证据行走 stderr）**：输出 `{count, templates, uris[], template_uris[]}` ⇒ 门禁与文档**从实现取值**（URI 表不手抄），与 T15/T16 同款纪律。
- ✅ **T34-⑤ 错误语义分得开（agent 能据此改行为而不是瞎重试）**：未知资源 URI ⇒ **-32005**（改 URI）；查无此人 ⇒ **-32001**（换人，与工具面同码）；缺 `uri` 参数 ⇒ **-32602**（改参数）⇒ 三个码**互不混用**。
- ✅ **T34-⑥ 新门 `mcp-resources-e2e.sh`（真进程、真实数据、零 mock）**，判据七条：R1 公布==实现（双向，派生）· R2 **跨面不变量**（10 个域各比一次，`resources/read` 的 text 解析后与 `tools/call` 的 structuredContent **逐字相等**）· R3 只读（ro token 也能 list+read）· R4 错误语义分得开 · R5 握手宣告 resources · R6 **每个公布的模板都真能读**（占位符用真值替换 ⇒ 堵「挂名不实现」）· R7 **文档面 == 实现**（`tool-contracts.md` 的 URI 表 vs 干跑清单，堵「文档变假」）⇒ **门已登记进 `run-all-gates.sh` 的 RUN 清单**（A1 完整性仍 PASS，RUN=21 / SKIP=1）。
- ✅ **T34-⑦ 证据链（缺一即算交付）**：
  - ①**新门实测全绿**：`./mcp-resources-e2e.sh` ⇒ **fails=0**；关键证据：`resources_list.uris_match_impl`（3 个具体资源）、`resources_templates.match_impl`（**11** 个模板）、`templates_readable=11/11`、10 个 `face.*` 跨面比较全 PASS、`unknown_uri=-32005` / `unknown_person=-32001` / `missing_uri=-32602` 三码各归其位、`ro_token.resources_read=ok`。
  - ②**门是「真门」的负向证据（本轮实撞 3 次，每次复原）**：①**注入跨面分叉**（在 `resource_contents()` 里给 resources 面多加一个 `__diverge__` 键 ⇒ 模拟「resources 与 tools 形状漂移」）⇒ **红 fails=10 / rc=1**（10 个 `face.*` 全红）⇒ 复原 ⇒ **fails=0**；②**注入文档漂移**（把 `tool-contracts.md` 里的 `ehr://audit` 改成 `ehr://auditX`）⇒ **红 fails=1**「doc.resources_uris_match_impl」（R7 有判别力）⇒ 复原 ⇒ 绿。**注**：第一次注入时我改的是 **builder 内部**（两边都改）⇒ 门**照样绿** ⇒ **这恰好证明「共享 builder」是有效的**（两边不可能分叉）⇒ 真正能触发分叉的注入点只可能落在**面的包装层**（`resource_contents`）⇒ 门就是钉在那里的。
  - ③**Rust 侧零破坏**：`cargo test --all-targets --locked` ⇒ **83 门全绿**（16 个 test target 全 ok）；`Cargo.toml`/`Cargo.lock` **未改动** ⇒ **零新依赖**。
  - ④**统一入口全量复跑：21 门全绿，总 fails=0**（`run-all-gates.sh` ⇒ A1 PASS + 21 个 `[PASS] … fails=0`；末行 `[证据] run-all-gates.sh 门禁数=21（本次执行 21）总 fails=0`）⇒ **覆盖面净增一道真跑的门**（20 → 21）。
  - ⑤**产物新鲜度**：`--print-mcp-resources` 是**新增 CLI 面** ⇒ 需重编译 debug+release ⇒ 同步 shared-target（`cp .new && mv` 换 inode，绕开 D-049 记录的 `Text file busy`）⇒ `artifact-freshness-check.sh` **fails=0**（两份产物 sha256 逐字一致）。
  - ⑥**文档面**：`tool-contracts.md` 新增「resources 面」章节（URI 表 + 等价工具 + 成功/失败形状 + 只读与同源纪律）+ 错误码表补 **-32005** ⇒ 且该文档**由 R7 机器核对**（不再是「手抄实现细节」的第二份）。
  - ⑦**后台卡死检查（D-010）**：**无 cargo/rustc 残留**；临时 ehr-api 由门禁脚本 `trap` 自清；常驻只剩两个 docker 容器 ⇒ **非卡死**。
- 🔄 **下一步**：①**T5 Web 管理端正式化**（目标验收①最后一块，**选型先报方案再动手**）；②**MCP 侧仍无 prompts**（resources 已补 ⇒ prompts 面仍未做；本项目的用法是「查档」不是「填模板」⇒ 倾向**不提供**，须在决策里显式记「不提供」的理由）；③**真·第三方 MCP 客户端互通仍未验**（含 resources 面的真客户端消费）⇒ 优先级上升；④公网暴露前 `EHR_BIND_ADDR` 改 `0.0.0.0` 前**必须**先设 token；⑤ghcr 包可见性仍 private（OCR 镜像未推 GHCR、未纳入 CI）；⑥`.dcm`/DICOM worker 未做；⑦上游分类缺口（认「出院记录」不认「出院小结」）仍在；⑧`docs/API.md` 是**生成物**（本轮**未改 REST 接口** ⇒ 无需重生成）；⑨人员域三端点仍 501 且刻意不进 OpenAPI；⑩**T34 新门尚未在真 GitHub runner 上验过**（本轮改动含 Rust 源码 + 新脚本 ⇒ 需一次 push 触发真 runner）。
- ⚠️ 未销账：目标验收①**Web 管理端仍是单文件最小版**（编译期内嵌，无构建链）；`docs/API.md` 是**生成物**；文档覆盖 **REST 面** + **MCP 面** + **SKILL.md 面** + **CI 可移植性面** + **Web 页面 JS 读面** 五份面全部有门；**「手抄实现细节」残留**已治到 **stub 清单 / REST 路径 / 响应形状 / 文档字段 / 产物新鲜度 / 门禁编排完整性 / MCP 工具契约字段 / MCP resources URI 表（本轮新增）/ SKILL.md 字段承诺 / CI 可移植性 / 「缺依赖不得静默跳过」/ 「派生期望值不得为空」** 十二个面 —— **「MCP 侧无 resources」本轮关闭**（协议面补齐 + 跨面不变量门 + 文档面门）。

## 当前状态（2026-10-22 · **T33「container-smoke 门从 SKIP 转正为 RUN」——重建镜像跑通容器化 e2e，并当场抓出该门自身的一个假绿洞（派生的期望值为空 ⇒ 空==空被判 PASS）** + **agent-e2e 真 agent 门本轮补跑成功（3/3 PASS）** · D-050 落地**）
- ✅ **T33-① 缺口定位（PLAN 连续多轮列着的同一条未销账项）**：「**`container-smoke.sh` 仍**未跑**（须先重建镜像）」。事实核对：运行中的 `deploy-ehr-api-1` 容器用的是 **10-21 的旧镜像**（image `a4edd6b7…`）⇒ 跑它只会红在陈旧产物 ⇒ 该门被登记在 `run-all-gates.sh` 的 **SKIP 清单**（理由写明「需要先重建镜像」）⇒ **这道门从 10-21 起就一直没存在过**。
- ✅ **T33-② 通道与前提先核实（dry-run 在前）**：TUNA sparse 直连 `https://mirrors.tuna.tsinghua.edu.cn/crates.io-index/` ⇒ **HTTP 200**；宿主代理 `127.0.0.1:7890` 在监听 ⇒ 容器 build 期的 `host.docker.internal:7890` 可用；`docker compose config` ⇒ **OK**；`.dockerignore` 白名单已含 `upstream-medme/packages/` ⇒ 构建 context 完整；磁盘余量 124G ⇒ 不会因空间失败。⇒ **参数生效有证据**（D4），不是盲跑。
- ✅ **T33-③ 重建镜像 + 起新容器**：`docker compose -f deploy/docker-compose.yml build ehr-api` ⇒ **3m36s 成功**，新镜像 sha `1cd00ea21147`（旧的 `a4edd6b7…` 已被替换）⇒ 构建期自带 dry-run 探针通过：`[证据] Dockerfile build: manifest_paths=17 openapi_paths=17`、`[证据] ehr-api dry-run exit=0` ⇒ **镜像里的二进制与当前 HEAD 同源**（不是旧产物）。`up -d ehr-api` ⇒ 容器日志 `[证据] ehr-api v0.0.1 | mode=serve | EHR_HOME=/home/ehr | ADDR=0.0.0.0:8443`（D1 证据行与实际 bind 同源）。
- ✅ **T33-④ 门自身有洞，当场抓出并修（关键）**：**首次跑 `container-smoke.sh` ⇒ `fails=1`，红在 `openapi.paths: got= want=ok`**。真因不是服务坏了，而是**门自己有假绿缺陷**：步骤 4/5 用 `docker run -e EHR_HOME=/tmp/h` 从**镜像自身**派生期望路径数，而**容器内 `/tmp/h` 不存在** ⇒ ehr-api 报 `EHR_HOME not accessible` ⇒ **stdout 为空** ⇒ `EXP=""`、`M=""` ⇒ **两个空串相等 ⇒ 步骤 5 被判 PASS（假绿）**；同时步骤 4 拿真实 17 与空串比 ⇒ 假红。⇒ 修法（**不是**改服务端，是改判据）：①`EHR_HOME=/tmp`（容器内**已存在**的目录）；②**派生的期望值为空时必须红**（新增 `image.manifest.paths` 与 `image.dry-run.paths` 两条空值断言）⇒ **纪律升级：任何「派生出来的期望值」为空时必须红，不许拿空值参与比较**（D-044「判据必须能被证伪」+ D-011「门红了先怀疑门」的双向纪律）。修后 ⇒ **`fails=0`，且 `paths=17 expected=17`、`image_paths=17 expected=17`（两边都从镜像派生，非手抄）**。
- ✅ **T33-⑤ 门是「真门」的负向证据**：把 `--print-rest-manifest` 故意改成 `--print-rest-manifestX` ⇒ **红 fails=3**（「派生的期望值为空」+ 步骤4 + 步骤5）⇒ 复原 ⇒ **fails=0** ⇒ **新判据有判别力**（若不修那两条空值断言，此注入会得到「步骤5 空==空 ⇒ PASS」⇒ 门是空的）。
- ✅ **T33-⑥ 门从 SKIP 转正为 RUN（登记完整性仍由 A1 机器核对）**：`run-all-gates.sh` ⇒ `container-smoke.sh` 移入 **RUN 清单**（20 门），SKIP 只剩 `agent-e2e.sh`（**理由仍必填**，A1 判据不变）⇒ `--list` dry-run 打印 `RUN=20 / SKIP=1` 且「[证据] A1 登记表完整」。
- ✅ **T33-⑦ 统一入口全量复跑：20 门全绿，总 fails=0**（`run-all-gates.sh` ⇒ A1 PASS + 20 个 `[PASS] … fails=0`，末行 `[证据] run-all-gates.sh 门禁数=20（本次执行 20）总 fails=0`）⇒ **覆盖面净增一道真跑的门**。
- ✅ **T33-⑧ `agent-e2e.sh` 本轮补跑成功（真·第三方 agent CLI，Copilot）**：退出码 **rc=0**，判定 **3/3 PASS**——「PASS agent 找到目标人」「PASS agent 报出数值 13.5」「PASS agent 给出证据来源」（docref `43b5077d…`）⇒ 此前 PLAN 记录的「exit=124 超时（LiteLLM 间歇 500）」本轮**未复现**（本轮 Duration 1m30s、Tokens ↑93.4k）⇒ 该门**本轮有效**，但仍留在 SKIP（依赖外部引擎 ⇒ 抖动会红在引擎而非代码 ⇒ 按 D-048「用真服务验证的性质 vs 可离线复现的性质必须分层」，**不并入例行入口**，作为**按需补跑**项，理由已写进 SKIP 清单）。
- ✅ **T33-⑨ Rust 侧零破坏**：`cargo test` 门由统一入口覆盖（`ehtest`/`rebuild`/`rebuild-lock` 等全绿）；**未改任何 Rust 源码**、`Cargo.toml`/`Cargo.lock` **未改动** ⇒ **零新依赖**；本轮改动只在 `container-smoke.sh`（判据）与 `run-all-gates.sh`（清单）两个 shell 脚本 ⇒ **不需重编译二进制**，产物新鲜度门仍绿。
- ✅ **T33-⑩ 后台卡死检查（D-010）**：**无 cargo/rustc 残留**；**无 copilot 残留**（补跑结束后进程已收）；临时目录 `/tmp/medme-ehr-agent-test` 由脚本自建自清；常驻只剩 10-21 的陈旧取证进程（`:18999`/`:18991`/`:19031`/`:19032`）、本轮新起的容器（`:8585`）与 10-21 的 OCR 容器 ⇒ **非卡死**。
- ✅ **T33-⑪ 真·runner 验证：run `39721697944`（sha `aaac47e`）= 全绿 ✅**（12 步 success + 兜底步正确 skipped）：①**镜像构建步 success**（runner 上直连 TUNA，**不依赖本机代理 7890**）⇒ `container-smoke` 门的前置在真 runner 上成立；②**起 compose 容器步 success**；③**统一门禁入口步 success** ⇒ **20 门（含新增的 `container-smoke.sh`）在真 runner 上全绿**；④步骤 13「门禁失败时打印每道门的原始输出」= **skipped** ⇒ 门全绿时兜底**正确地不跑**（与 D-047/D-048 同款对照）。
- ✅ **T33-⑫ 途中的一次真红（必须记录，因为它是自己造成的）**：把 `container-smoke.sh` 移入 RUN 清单后的**第一次 push**（sha `33d138f`）⇒ **run `39721077890` 在第 10 步「统一门禁入口」failure** ⇒ 原因正是**runner 上没有镜像**（容器门的前置缺失）⇒ **不是代码坏了，是我把门转正却没给 CI 补前置**。⇒ 修法（不是把门退回 SKIP，**退回 SKIP 等于把刚发现的缺口重新藏起来**）：`ci-gates.yml` 增两步「构建容器镜像（直连 TUNA，build-args 不用本机代理）」+「起 compose 容器（`--no-build`，避免 compose 用带代理的 build-args 触发重建）」⇒ 推 `aaac47e` ⇒ run `39721697944` **全绿** ⇒ **这次红是本轮最有价值的证据**：它证明「把门转正」这件事**在真 runner 上同样必须补齐前置**（本机有代理、runner 没有 ⇒ 本机绿不代表 runner 绿）。
- 🔄 **下一步**：①**T5 Web 管理端正式化**（目标验收①最后一块，**选型先报方案再动手**）；②**MCP 侧仍无 resources/prompts**；③公网暴露前 `EHR_BIND_ADDR` 改 `0.0.0.0` 前**必须**先设 token（当前 `EHR_API_TOKENS` 为空 ⇒ 放行模式，仅本地）；④ghcr 包可见性仍 private（OCR 镜像未推 GHCR、未纳入 CI）；⑤**容器镜像重建耗时 3m36s 且本机网络依赖代理 7890** ⇒ 若要把 `container-smoke` 纳入 CI，需先有 CI 侧镜像构建通道；⑥真·第三方 MCP 客户端互通仍未验；⑦`.dcm`/DICOM worker 未做（能力清单已如实标 `enabled=false`）；⑧上游分类缺口（认「出院记录」不认「出院小结」）仍在；⑨`docs/API.md` 是**生成物**（本轮**未改接口** ⇒ 无需重生成）；⑩人员域三端点仍 501 且刻意不进 OpenAPI；⑪**本轮新增的 `container-smoke.sh` 门尚未在真 GitHub runner 上验过**（runner 上没有本机代理 ⇒ 需要镜像构建通道，见⑤）。
- ⚠️ 未销账：目标验收①**Web 管理端仍是单文件最小版**（编译期内嵌，无构建链）；`docs/API.md` 是**生成物**；文档覆盖 **REST 面** + **MCP 面** + **SKILL.md 面** + **CI 可移植性面**——四份文档/契约面全部有门；**「手抄实现细节」残留**已治到 **stub 清单 / REST 路径 / 响应形状 / 文档字段 / 产物新鲜度 / 门禁编排完整性 / MCP 工具契约字段 / SKILL.md 字段承诺 / CI 可移植性（含 Python 内层 + workflow + `tools/`）/ 「缺依赖不得静默跳过」/ 「派生期望值不得为空（空==空假绿）」** 十一个面 —— **「`container-smoke.sh` 未跑」本轮关闭**（重建镜像 + 门转 RUN + 门自身假绿洞已封）。

## 当前状态（2026-10-22 · **T32「家庭自测趋势」专用视图落地——把「家庭自测值 ≠ 医院指标」这条上游结构约束在 Web 端显式做成一屏（只读已有真端点，零新接口/零新依赖）** + 新判据「页面读的字段必须来自**运行时 OpenAPI 的 schema**」（字段清单与 schema **两边都派生**，不手抄）· D-049 落地**）
- ✅ **T32-① 缺口定位（PLAN 连续多轮列着的同一条未销账项）**：「**「家庭自测趋势」专用视图仍未做**」。它**不是装饰性 UI 缺口**：上游用 `GroupKey::SelfMeasured` 把家庭自测序列与医院序列**结构上分开**（家庭血压区间是「家庭自测上限 135/85」，与门诊 140/90 不可套用）⇒ 把两者画进同一条线 ⇒ **参考区间就是错的**。现有 Web「指标」页只读 `labs`（虽有 `self_measured` 标志，但**没有任何一处把家庭自测单独呈现**）⇒ 消费方极易把家庭值当医院值解读（D-016 家族）。
- ✅ **T32-② 先干跑再动手（D4「参数生效要有证据」）**：动手前起临时 ehr-api，真写 3 条血压自测 ⇒ 逐字段观测到 `measurements` 真响应为 `taken_at`/`docref`/`payload_readable`/`values[].{analyte_key,value,unit,ref_low,ref_high,ref_source,flag}`，且 `labs` 里 `self_measured=true` ⇒ 视图的读法照着**真响应**写，不是照想象写。
- ✅ **T32-③ 实现：只读已有真端点，零后端改动**：`ehr-api/web/index.html` 新增「**自测趋势**」页签 ⇒ 数据源 `GET /persons/{pid}/measurements?kind=self_measurement`（D-028 已实装的写侧回放投影）⇒ **未改任何 Rust 源码**、`Cargo.toml`/`Cargo.lock` **未改动** ⇒ **零新依赖、零 API 契约变更**（⇒ `docs/API.md` 仍是生成物且**本轮无需重生成**）。与「指标」页**刻意分栏**，并把「**不与医院指标混用参考区间**」写进页面。
- ✅ **T32-④ 页面上的两条上游纪律必须保持**（否则 UI 就在造数据）：①**「查不到出处就不给区间」** ⇒ `ref_low`/`ref_high` 是**可选键**，缺则显示 `—`，**不臆造阈值**；显示区间时**带 `ref_source` 出处**；②`payload_readable=false` 的记录**被过滤掉**（结构化载荷读不出来时原文仍在 `text` 里 ⇒ 这一屏只画能读的值，**不猜**）。
- ✅ **T32-⑤ 新判据落到 **schema**，不是「真响应里必须有该键」**（本轮最关键取舍）：按「页面读的每个字段都必须在真响应里出现」来判 ⇒ **合法的可选键**（`ref_low`/`ref_high`/`flag`）会**假红**（D-011 家族「门红了先怀疑门」）⇒ 改判「该字段必须存在于**运行时 OpenAPI 的 schema**」（可选键也在 schema 里）⇒ 既不假红，又仍抓住「页面读了一个服务端根本不存在的字段」。
- ✅ **T32-⑥ 字段清单与 schema **两边都派生，都不手抄**：①「页面读了哪些字段」从**页面源码**正则派生（`d.<f>` 顶层 / `m.<f>` 记录项 / `v.<f>` 值项）；②「服务端有哪些字段」从**运行时 `/openapi.json`** 顺着 `$ref` 走到 `MeasurementsReply → MeasurementOut → MeasurementValueOut`（**不手写 schema 名**）⇒ 页面改字段门自动跟着变、服务端改契约门自动红。判据**还覆盖路径**：页面 `j(\`…\`)` 的路径（`${cur}`→`{pid}`）必须在 OpenAPI `paths` 里**且是真 operation**。
- ✅ **T32-⑦ 门必须**自己造数**（不依赖已有数据）**：`web-e2e.sh` 新增 **W4** ⇒ 现场 `POST /entries/self-measurement` 写一条血压自测 ⇒ 读真响应 ⇒ 断言真响应里**确有** `taken_at`/`docref`/`payload_readable`/`values` 与值项 `analyte_key`/`value`/`unit` ⇒ 空库时该门**恒绿**的老问题被治住（与 T27 同款纪律）。
- ✅ **T32-⑧ 证据链（缺一即算交付）**：
  - ①**新判据实测全绿**：`./web-e2e.sh` ⇒ W1/W2/W3 照旧绿 + **W4 绿** ⇒ `fails=0`；W4 派生输出 = `d=['count','measurements'] m=['docref','payload_readable','taken_at','values'] v=['analyte_key','flag','ref_high','ref_low','unit','value']`、路径 `/api/v1/persons/{pid}/measurements`、`count=1`。
  - ②**门是「真门」的负向证据（本轮实撞 2 次，每次复原）**：①页面 `m.payload_readable` → `m.payload_readableX` ⇒ **红 fails=1**「页面读的字段在 OpenAPI schema 里不存在: ['m.payload_readableX']」；②请求路径 → `.../measurementsX` ⇒ **红 fails=1**「页面请求路径 /api/v1/persons/{pid}/measurementsX 不在 OpenAPI 里」⇒ **字段面与路径面各一次**。
  - ③**Rust 侧零破坏**：`cargo test --all-targets --locked` ⇒ **83 门全绿**（16 个 test target 全 ok，rc=0）；**未改任何 Rust 源码** ⇒ **零新依赖**。
  - ④**统一入口全量复跑：19 门全绿，总 fails=0**（`run-all-gates.sh` ⇒ `门禁数=19（本次执行 19）总 fails=0`）。
  - ⑤**产物新鲜度与同步**：改了 `web/index.html`（`include_str!` ⇒ 必须重编译才进二进制）⇒ 重编译 debug+release 并同步 ⇒ **两份产物 sha256 逐字一致**（release `d5204a26…`、debug `1106db38…`），`artifact-freshness-check.sh` 绿。
  - ⑥**后台卡死检查（D-010）**：**无 cargo/rustc 残留**；本轮起的临时 ehr-api（`:19077`）已收割、临时目录 `/tmp/ehr-dry-*` 已清；常驻只剩 10-21/10-22 的陈旧 `ehr-api`（`:18999`/`:18991`/`:19031`/`:19032`）与两个 docker 容器 ⇒ **非卡死**。
  - ⑦**真·runner 验证（run `39715605855`，sha `8b35f1b`）= 全绿 ✅**：推 `main` ⇒ 真 GitHub runner ⇒ **步骤 1~10 全部 success**（`checkout`/`装 Rust`/`装 Node`/`拉取上游`/`TUNA 通道`/`构建`/`cargo test`/`同步产物`/**`统一门禁入口`**）；日志实锤 `run-all-gates.sh 门禁数=19（本次执行 19）总 fails=0` 且 `web-e2e.sh` = `[PASS] web-e2e.sh fails=0` ⇒ **本轮新增的 W4 判据在真 runner 上也执行且通过**（门禁汇总为机器判定 ⇒ W4 若红则该行必为 fails≠0）；步骤 11「门禁失败时打印每道门的原始输出」= **skipped** ⇒ 门全绿时兜底步骤**正确地不跑**（D-047/D-048 同款对照）。
  - ⑧**本轮踩坑并已修（记录在案）**：**产物被运行中的进程占住 ⇒ 同步失败**。两个 10-21/10-22 的陈旧取证进程**一直持有** `~/medme-shared-target/release/ehr-api` ⇒ 直接 `cp` 覆盖报 **`Text file busy`** ⇒ 差点被当成「环境问题」跳过（而真相是**门禁会继续跑旧产物**，D-035 新坑③的复发）⇒ 处置：**先 `cp` 到 `.new` 再 `mv` 覆盖**（换 inode ⇒ 绕过 busy；老进程继续跑它的旧 inode，不影响本轮门禁测的产物）⇒ **同步步骤从此对「有进程在跑」免疫**。
- 🔄 **下一步**：①**T5 Web 管理端正式化**（目标验收①最后一块，**选型先报方案再动手**）——本轮只补了**一个视图**，Web 端仍是单文件最小版（编译期内嵌、无构建链）；②**MCP 侧仍无 resources/prompts**；③公网暴露前 `EHR_BIND_ADDR` 改 `0.0.0.0` 前**必须**先设 token（当前 `EHR_API_TOKENS` 为空 ⇒ 放行模式，仅本地）；④ghcr 包可见性仍 private（OCR 镜像未推 GHCR、未纳入 CI）；⑤真·第三方 MCP 客户端互通仍未验；⑥`.dcm`/DICOM worker 未做（能力清单已如实标 `enabled=false`）；⑦上游分类缺口（认「出院记录」不认「出院小结」）仍在；⑧`docs/API.md` 是**生成物**（本轮**未改接口** ⇒ 无需重生成）；⑨人员域三端点仍 501 且刻意不进 OpenAPI；⑩`container-smoke.sh` 仍**未跑**（须先重建镜像）；⑪**本轮改动已在真 GitHub runner 上验过**（run `39715605855`，sha `8b35f1b` 全绿 ⇒ 连续第 3 次真 runner 全绿）。
- ⚠️ 未销账：目标验收①**Web 管理端仍是单文件最小版**（编译期内嵌，无构建链）——**「家庭自测趋势」视图已补齐**（本轮），但 Web 端整体仍待正式化；`docs/API.md` 是**生成物**；文档覆盖 **REST 面** + **MCP 面** + **SKILL.md 面** + **CI 可移植性面**——**四份文档/契约面全部有门**，本轮新增第五面：**Web 页面 JS 读面 × 运行时 OpenAPI schema**；**上游分类缺口**仍在；**上游审计口径缺口**已在 D-030 如实标注（**不造事件补齐**）；**OCR 镜像尚未推 GHCR、未纳入 CI**；**「手抄实现细节」残留**已治到 **stub 清单 / REST 路径 / 响应形状 / 文档字段 / 产物新鲜度 / 门禁编排完整性 / MCP 工具契约字段 / SKILL.md 字段承诺 / CI 可移植性（含 Python 内层 + workflow + `tools/`）/ 「缺依赖不得静默跳过」** 十个面 —— **「家庭自测趋势」专用视图本轮关闭**。

## 当前状态（2026-10-27 · **T31「体检报告列布局 labs=0」缺口关闭：抽取路径能救（机器可判）** + **真 PDF → 真 LLM → 回写端到端跑通（labs 0 ⇒ 50 series）** + 可移植性门又补一个洞（Python 行尾注释让整行逃逸）· D-048 落地**）
- ✅ **T31-① 缺口本体先复现（不粉饰）**：真实体检报告 `docs/testres/检查单/lbb/2021_0907.pdf`（**列布局**：项目/结果/单位/参考区间分列）入档 ⇒ **正则路径 labs series=0 / points=0** ⇒ 确认 PLAN 未销账项「体检报告 PDF 列布局 labs=0」是**真缺口**，且**不是**环境差异。
- ✅ **T31-② 缺口关闭方式 = 补一条「可机器判定」的路，而不是放宽正则**：新增门禁 `extraction-e2e.sh`（**零外部依赖**：cargo+curl+python3，不需要 LLM 网关/key/网络）⇒ 证明「**结构化抽取路径能救列布局**」这条性质**可离线复现**。判据五条（缺一即红）：**E1** 缺口复现（正则 labs=0）→ **E2** 回写 extraction 后 labs series≥3 且落在点名的指标上 → **E3** 真相层不被污染（塞一条**原文根本没有**的伪造指标「血糖999」⇒ 逐字闸必须判假、不得进 labs）→ **E4** 持久化（换新进程读同一 `EHR_HOME`，series 数不变 ⇒ 结果落在事件日志+派生索引，不是只在内存）→ **E5** 契约一致（extraction 端点在 OpenAPI 里是**真 operation**，非 stub→501）。
- ✅ **T31-③ 真实端到端跑通（真 PDF + 真 LLM 网关 + 真服务端，不是 fixture）**：`tools/pdf_llm_extract.py`（新增）= 「从 ehr-api 取文本层 → 用**上游同一份** system prompt（`upstream-medme/packages/deid/prompts/extract_v1_system.txt`）与**同一份** `extract_params.json` 调 LLM（OpenAI 兼容 `/chat/completions`）→ 模型输出**原样** PUT 回 `PUT /documents/{sha}/extraction`（服务端跑上游 `deid::verify` 逐字闸）→ 复跑 `GET /persons/{pid}/labs`」。**先 dry-run 后回写**（脚本内置 `--dry-run`）。
  - **dry-run（不回写）证据**：文本层 chars=11370 ⇒ 模型输出 **labs 64 条**（含「白细胞计数 4.7 10^9/L 3.5-9.5」「血红蛋白 125.0 g/L 130-175 L」「红细胞计数 3.73 L」等，**值与参考区间逐项对齐体检单**）⇒ 打印但**不落库**。
  - **回写（真跑）证据**：回执 `labs_count=64, diagnoses_count=9, verified=false, verify_detail={"rejected":2}` ⇒ **逐字闸判假 2 条**（模型编的/原文对不上的）⇒ 落库的是**校验后**的结果 ⇒ **labs 投影 series=0 ⇒ 50 series / 50 points**（含「中性粒细胞百分比 %」「低密度脂蛋白胆固醇 mmol/L」「尿比重」等）。
  - ⇒ **PLAN 未销账项「缺的是『把 PDF 送进 LLM』这一步的端到端脚本」本轮关闭**（此前重复列了两次的同一条缺口）。
- ✅ **T31-④ 可移植性门又补一个洞（实测，非推演）**：`ci_portability_check.py` 原用 tokenize 的 COMMENT token 行号**整行**跳过 ⇒ **代码行尾挂的注释**（`X = "/home/lbb/..."  # 负向注入`）会把**整行**标成「非代码」⇒ 那一行里的机器专属路径/产物路径**逃过所有判据** ⇒ **门是空的**。⇒ 改为**逐行取「该行注释起点列之前的代码部分」**（注释在代码之前⇒整行跳过；注释在代码之后⇒只判代码部分；docstring 行仍按 `ast` 精确排除）。
- ✅ **T31-⑤ 覆盖面补到 `tools/`**：可移植性门的受检集合从 31 → **33 个**（新增 `tools/pdf_llm_extract.py`）⇒ **「一份实现」的载体**又多了一个，必须纳入门；新门 `extraction-e2e.sh` 也登记进 `NEEDS_ENV`（它起真进程 ⇒ 必须经唯一来源 `ci/env.sh`）。
- ✅ **T31-⑥ 证据链（缺一即算交付）**：
  - ①**新门实测全绿**：`./extraction-e2e.sh` ⇒ 8 个 PASS、**fails=0**；关键证据：E1 `series=0`（缺口复现）→ 回执 `labs_count=3`（伪造项被逐字闸清空）→ 投影 `series=3 points=3 names=['总胆固醇','白细胞','空腹血葡萄糖']` → 重启后 `series=3`（E4 持久化）→ OpenAPI 里 extraction 是真 operation。
  - ②**门是「真门」的负向证据**：往 `ci_portability_check.py` 注入**行尾注释**里的 `/home/lbb/medme-shared-target/release/ehr-api` ⇒ **红 fails=2**（「机器专属绝对路径」+「自己拼了产物路径」）⇒ 复原 ⇒ **fails=0** ⇒ **新判据有判别力**（旧实现对同一注入是绿的）。
  - ③**Rust 侧零破坏**：`cargo test --all-targets --locked` **83 门全绿**（`Cargo.toml`/`Cargo.lock` **未改动** ⇒ **零新依赖**；本轮**未改任何 Rust 源码**）。
  - ④**统一入口全量复跑：19 门全绿，总 fails=0**（`run-all-gates.sh` ⇒ `门禁数=19（本次执行 19）总 fails=0`；**新门已登记进 RUN 清单** ⇒ A1 完整性仍 PASS；`extraction-e2e.sh` ⇒ `[PASS] fails=0`）。**SKIP 2 个**：`container-smoke.sh`（需先重建镜像）、`agent-e2e.sh`（需真·第三方 agent CLI）——均写明原因。
  - ⑤**真·端到端（真 PDF + 真网关）**：见 T31-③（0 ⇒ 50 series，逐字闸判假 2 条）。
  - ⑥**后台卡死检查（D-010）**：**无 cargo/rustc 残留**；本轮起止的临时 ehr-api（`:19041`）已收割、临时目录已清；常驻只有两个 10-21 的陈旧 `ehr-api`（`:18999`/`:18991`）、两个 10-21/10-22 的取证进程（`:19031`/`:19032`）与两个 docker 容器 ⇒ **非卡死**。
  - ⑦**私密数据不入库**：`docs/testres/`（真体检单 + LLM key）本就在 `.gitignore` 内 ⇒ 本轮**未**把它加进任何入库文件；LLM key/网关地址**只出现在 shell 环境变量**里，**未写入仓库任何文件**。
- ✅ **T31-⑧ 第三次真 runner 验证：run `39709726552`（sha `5f79842`）= 全绿 ✅**：推 `main` ⇒ 真 GitHub runner ⇒ **10 个步骤全部 success**（`checkout` / `装 Rust 工具链` / **`装 Node`** / `拉取上游` / `TUNA 通道` / `构建 debug+release` / `cargo test` / `同步产物` / **`统一门禁入口`** ⇒ 关键：①**新门 `extraction-e2e.sh` 在真 runner 上绿**（它依赖 `python3`/`curl`/cargo/网络零依赖 ⇒ runner 环境满足）；②**可移植性门在真 runner 上绿**（受检 33 个文件，**含新增的 `tools/pdf_llm_extract.py`**）；③**兜底步骤「门禁失败时打印每道门的原始输出」= `skipped`** ⇒ 这是**门全绿时的正确行为**（不是失败，是没人需要兜底）⇒ 与 D-047 记录的「兜底步骤自己先炸」形成对照：**该跑时跑、不该跑时不跑**）。
- ✅ **T31-⑨ 第四次真 runner 验证：run `39710367533`（sha `5370e4e`，纯文档回填提交）= 全绿 ✅** ⇒ **连续两次真 runner 全绿**（`39709726552` 代码提交 / `39710367533` 文档提交）⇒ CI 这条链路**稳定**（不是一次性侥幸）。
- ⚠️ **本轮踩坑并已修（记录在案）**：**LLM 网关对 11KB prompt + `max_tokens=16384` 直接 504**（模型 `hy3`）⇒ 换小模型 `space-bunny`（`max_tokens` 同值）**一次成功**（prompt_tokens=6811 / completion=2776 / finish_reason=stop）⇒ 教训：**「模型/参数组合」是取证链的一部分，必须在证据里记名**（本轮记了 model、usage、finish_reason）⇒ 不能只记「LLM 抽取成功」。
- 🔄 **下一步**：①**T5 Web 管理端正式化**（目标验收①最后一块，**选型先报方案再动手**）；②**「家庭自测趋势」专用视图仍未做**；③公网暴露前 `EHR_BIND_ADDR` 改 `0.0.0.0` 前**必须**先设 token（当前 OCR 编排 `EHR_API_TOKENS` 为空 ⇒ **放行模式**，仅本地）；④ghcr 包可见性仍 private（OCR 镜像也尚未推 GHCR）；⑤真·第三方 MCP 客户端互通仍未验；⑥`.dcm`/DICOM worker 未做（能力清单已如实标 `enabled=false`）；⑦**上游分类缺口**（`parser::classify` 认「出院记录」不认「出院小结」）仍在；⑧MCP 侧仍**无 resources/prompts**；⑨`docs/API.md` 是**生成物**，改接口须跑 `./api-doc-check.sh --write`；⑩人员域三端点仍 501 且刻意不进 OpenAPI；⑪**OCR 镜像未推 GHCR / 未纳入 CI**；⑫`container-smoke.sh` 仍**未跑**（须先重建镜像）；⑬**T31 新门已在真 GitHub runner 上验过**（run `39709726552` 全绿，含 `extraction-e2e.sh` 与 33 文件的可移植性门）。
- ⚠️ 未销账：目标验收①**Web 管理端仍是单文件最小版**（编译期内嵌，无构建链）；`docs/API.md` 是**生成物**；文档覆盖 **REST 面** + **MCP 面** + **SKILL.md 面** + **CI 可移植性面**——**四份文档/契约面全部有门**；**上游分类缺口**仍在；**上游审计口径缺口**已在 D-030 如实标注（**不造事件补齐**）；**OCR 镜像尚未推 GHCR、未纳入 CI**；**「手抄实现细节」残留**已治到 **stub 清单 / REST 路径 / 响应形状 / 文档字段 / 产物新鲜度 / 门禁编排完整性 / MCP 工具契约字段 / SKILL.md 字段承诺 / CI 可移植性（含 Python 内层 + workflow + `tools/`）/ 「缺依赖不得静默跳过」** 十个面——**「体检报告列布局 labs=0」本轮关闭**（离线可判的门 + 真 PDF→真 LLM→回写 0⇒50）。

## 当前状态（2026-10-27 · **T30「CI 真 runner 首次验证」——第一次真跑就红了：Web 页面 JS 整文件是 SyntaxError，而这个洞被一道「静默 SKIP」的检查盖了几个月（真缺陷已修 + 门禁语义改「缺依赖即红」）· D-047 落地**）
- ✅ **T30-① 本轮唯一目标：把 CI 真的跑一次**（D-046 未销账①原话：「下次 push 到 `main` 应当是第一次真跑」）。已把 T29 提交推上 `main`（sha `f5a85c0`）⇒ 触发真 GitHub runner ⇒ **run `39697214296`**。
- ✅ **T30-② 真跑第一次就红了——红的是真缺陷，不是环境差异**：run `39697214296` 的**前 7 步全 success**（`checkout` / `装 Rust 工具链` / `拉取上游`（`ci/fetch-upstream.sh`）/ `TUNA 通道` / `构建 debug+release --locked` / `cargo test` / `同步产物`）⇒ **第一次拿到真 runner 的证据**（此前全是本机模拟）；第 8 步 `统一门禁入口` **failure** ⇒ `[FAIL] web-e2e.sh 退出码=1 fails=1 / FAIL W2 js syntax` ⇒ 总 fails=1，**17/18 门在真 runner 上绿**。
- ✅ **T30-③ 真缺陷：Web 页面 JS 整文件是 SyntaxError ⇒ Web 管理端在浏览器里一行都跑不起来**：`ehr-api/web/index.html` 的 `j()` 里 `const t`（token）与 `const t`（响应文本）**同作用域重复声明** ⇒ 整个 `<script>` 解析失败。**为什么几个月没被发现**：`web-e2e.sh` 的 W2 用 `node --check` 校验 JS 语法，而**本机没有 node** ⇒ 原实现走 `else` 分支 `echo "SKIP W2 js syntax"` ⇒ **W2 在本机从未真正执行** ⇒ W1（HTML 200/字段）W3（业务流/字段交叉核对）全绿 ⇒ **门禁全绿而产品是坏的**。⇒ 装 node v20.18.0（TUNA 通道）后本机 `node --check` 直接报 `SyntaxError: Identifier 't' has already been declared`（第 22 行）⇒ 修复：响应文本改名 `txt` ⇒ `node --check` **rc=0**。
- ✅ **T30-④ 门禁语义收紧：缺依赖不再「静默 SKIP」，改为红**（D-042 决策②的收紧版）：W2 的 else 分支由 `SKIP` 改为 `bad "W2 js syntax 未执行（node 不存在）"` + 打印修复路径 ⇒ **「因为缺依赖而假装通过」的检查从门禁体系里消失**。
- ✅ **T30-⑤ CI 必须自己装 node，否则 W2 在 runner 上也会被跳过**：workflow 新增步骤「装 Node（Web 门禁 W2 的 JS 语法检查需要它）」，走 **TUNA nodejs-release**（D-007 同族），v20.18.0，`node --version` 打印留证 ⇒ **真 runner 上该步骤 success**（run `39698485007`）。
- ✅ **T30-⑥ 顺手修掉 workflow 里一处真 bug（CI 日志抓到的）**：「门禁失败时打印每道门的原始输出」步骤原写 `ls -la "$CARGO_TARGET_DIR"/*/ehr-api` 但**没 source `ci/env.sh`** ⇒ `set -euo pipefail` 下 `CARGO_TARGET_DIR: unbound variable` ⇒ **兜底步骤自己先炸** ⇒ 门禁失败时**拿不到任何产物信息**。⇒ 改为先 `. ./ci/env.sh`（与 D-045 决策③「产物路径单一来源」一致，且满足 D-046 判据 C）。
- ✅ **T30-⑦ 证据链（缺一即算交付）**：
  - ①**真 runner 首次验证（run 39697214296，sha f5a85c0）**：前 7 步 success、第 8 步 failure 且失败原因**精确到判据**（`FAIL W2 js syntax`）⇒ **CI 不是摆设**（它抓到了本机抓不到的缺陷）。
  - ②**缺陷是实测定位**：node v20.18.0 + `node --check` ⇒ `SyntaxError` 第 22 行 ⇒ 指向 `j()` ⇒ 改名 `txt` ⇒ rc=0。
  - ③**门是真门的负向证据**：把 `txt` 故意改回 `t` ⇒ 重编译 release ⇒ `./web-e2e.sh` ⇒ **`fails=1`**（W2 红）⇒ 复原 ⇒ **`fails=0`** ⇒ W2 有判别力。
  - ④**本机统一入口全量复跑：18 门全绿，总 fails=0**（`run-all-gates.sh` ⇒ A1 PASS + 18 个 `[PASS] … fails=0`；SKIP 2 个同前）。**覆盖面扩大**：`web-e2e.sh` 由「SKIP W2」变成「**PASS W2**」⇒ 一道从未执行的检查现在真执行。
  - ⑤**Rust 侧零破坏**：`cargo test --all-targets --locked` **83 门全绿**；`Cargo.toml`/`Cargo.lock` **未改动** ⇒ **零新依赖**（`web/index.html` 经 `include_str!` 内嵌 ⇒ 只影响重编译，不改逻辑）。
  - ⑥**后台卡死检查（D-010）**：**无 cargo/rustc 残留**；常驻只有两个 10-21 的陈旧 `ehr-api` 进程（`:18999`/`:18991`）与两个 docker 容器 ⇒ **非卡死**；node 装在 `~/.local/node-v20.18.0-linux-x64`（非临时进程）。
  - ⑦**第二次真跑（run 39698485007，sha 7b0cfe6）= 全绿 ✅**：真 runner 复跑 ⇒ **run 结论 success**；日志逐条：`装 Node（v20.18.0）` success、`A1 登记表完整（RUN=18 SKIP=2）`、**18 个 `[PASS] … fails=0`**、汇总 `门禁数=18（本次执行 18）总 fails=0` ⇒ **「统一入口接入 CI」在真 runner 上闭环**（第一次红 ⇒ 定位真缺陷 ⇒ 修 ⇒ 复跑绿）。
- ⚠️ **本轮踩坑并已修（记录在案）**：**「本机缺依赖 ⇒ 门静默跳过」是比「门红」更坏的失败模式**（W2 跳过 ⇒ 整文件 SyntaxError 的 Web 端活了几个月）；以及**「门禁失败时的兜底步骤自己失败 ⇒ 你在最需要证据的时候没有证据」**（兜底步骤因 `CARGO_TARGET_DIR` 未绑定而炸掉）。⇒ 纪律升级两条：**① 任何检查项缺依赖时必须红或「不存在」，不允许「静默通过」；依赖必须写进 CI 安装步骤**；**② 失败兜底路径必须与主路径共享同一份事实来源**。
- 🔄 **下一步**：①**T5 Web 管理端正式化**（目标验收①最后一块，**选型先报方案再动手**）；②「体检报告 PDF 列布局 labs=0」缺口：extraction 管线已就位，缺的是「把 PDF 送进 LLM」这一步的**端到端脚本**；③「体检报告 PDF 列布局 labs=0」缺口：extraction 管线已就位，缺的是「把 PDF 送进 LLM」这一步的**端到端脚本**；④「家庭自测趋势」专用视图仍未做；⑤公网暴露前 `EHR_BIND_ADDR` 改 `0.0.0.0` 前**必须**先设 token（当前 OCR 编排 `EHR_API_TOKENS` 为空 ⇒ **放行模式**，仅本地）；⑥ghcr 包可见性仍 private（OCR 镜像也尚未推 GHCR）；⑦真·第三方 MCP 客户端互通仍未验；⑧`.dcm`/DICOM worker 未做（能力清单已如实标 `enabled=false`）；⑨**上游分类缺口**（`parser::classify` 认「出院记录」不认「出院小结」）仍在；⑩MCP 侧仍**无 resources/prompts**；⑪`docs/API.md` 是**生成物**，改接口须跑 `./api-doc-check.sh --write`；⑫人员域三端点仍 501 且刻意不进 OpenAPI；⑬**OCR 镜像未推 GHCR / 未纳入 CI**；⑭`container-smoke.sh` 仍**未跑**（须先重建镜像）。
- ⚠️ 未销账：目标验收①**Web 管理端仍是单文件最小版**（编译期内嵌，无构建链）；**Web 端刚被 CI 抓出「整文件 SyntaxError」⇒ 说明「本机门禁全绿」曾长期不等于「页面能用」**（本轮起 W2 在本机与 CI 都真执行）；`docs/API.md` 是**生成物**；文档覆盖 **REST 面** + **MCP 面** + **SKILL.md 面** + **CI 可移植性面**——**四份文档/契约面全部有门**；**上游分类缺口**仍在；**上游审计口径缺口**已在 D-030 如实标注（**不造事件补齐**）；**OCR 镜像尚未推 GHCR、未纳入 CI**；**「手抄实现细节」残留**已治到 **stub 清单 / REST 路径 / 响应形状 / 文档字段 / 产物新鲜度 / 门禁编排完整性 / MCP 工具契约字段 / SKILL.md 字段承诺 / CI 可移植性（含 Python 内层 + workflow）/ 「缺依赖不得静默跳过」** 十个面——**「统一入口接入 CI」已落地并完成真 runner 闭环**（第一次真跑红 ⇒ 抓出真缺陷 ⇒ 修 ⇒ 第二次真跑绿）。
## 当前状态（2026-10-27 · **T29「可移植性门覆盖到 Python 内层实现与 CI workflow」——T28 那道门**自己有个洞**：Python 内层实现完全在覆盖之外，实测注入机器专属路径仍绿；洞里藏着 3 类真缺陷（已修）+ 判据 C 原本判「注释里提到 source」也算数（已修）· D-046 落地**）
- ✅ **T29-① 接手工作树的第一件事仍是「让它编译并跑一遍」（D-039 新坑② 纪律沿用）**：`cd ehr-api && cargo test --all-targets --locked` ⇒ **83 门全绿** ⇒ 确认**工作树可编译可跑**。（本轮**未改任何 Rust 代码** ⇒ `Cargo.toml`/`Cargo.lock` **未改动** ⇒ **零新依赖**。）
- ✅ **T29-② 缺口定位（D-045 未销账项⑮「`ci-portability-check.sh` 判据**未覆盖 Python 内层实现**（当前只查 `*.sh`）」的正面处置）**：T28 那道门**只扫 `*.sh`** ⇒ **每道门的 Python 内层实现完全在覆盖之外** ⇒ 而本仓库**每道门都是「.sh 包装 + .py 内层」**（`artifact_freshness` / `doc_contract` / `mcp_contract` / `skill_doc_field`）⇒ **覆盖面比看上去小一半**。
- ✅ **T29-③ 门的洞是实测出来的，不是推演**：往 `skill_doc_field_check.py` 注入 `/home/lbb/medme-shared-target/release/ehr-api` ⇒ **门照样绿（fails=0, rc=0）** ⇒ 门有洞。⇒ 本轮把覆盖面从 **`*.sh` 扩到 `*.sh` + `*.py` + `ci/*` + `skills/**/ehr.sh` + `.github/workflows/*.yml`**（受检文件 **23 → 31 个**）。
- ✅ **T29-④ 洞里藏着的真缺陷 3 类（本轮实测修掉，不是推演）**：
  - **① `skill_doc_field_check.py` 自带兜底默认值**：`os.environ.get("EHR_BIN", "~/medme-shared-target/release/ehr-api")` ⇒ 那是**绕过唯一来源的第二份实现** ⇒ 在 CI 上若注入漏了，就会**静默**跑那份默认路径上的旧产物，而门是绿的（D-035 新坑③「先问它跑的是哪一份产物」）⇒ 改为「**没注入就明确失败**」（实测 `EHR_BIN` 未注入 ⇒ rc=1 + 明确报错）。
  - **② `artifact_freshness_check.py` 另写一份默认根**：`os.environ.get("CARGO_TARGET_DIR", "~/medme-shared-target")` ⇒ 同族 ⇒ 改为由 wrapper 注入 `EHR_SHARED_TARGET_DIR`/`EHR_LOCAL_TARGET_DIR`（后者**新增**为 `ci/env.sh` 的契约项，因为新鲜度门要比「local vs shared」两份产物）⇒ 未注入 ⇒ rc=1。
  - **③ CI workflow 里也写死了一份默认根**：`.github/workflows/ci-gates.yml` 的「同步产物」步骤原写 `SHARED="${CARGO_TARGET_DIR:-$HOME/medme-shared-target}"` ⇒ **workflow 也是「一份实现」的载体** ⇒ 改为 `. ./ci/env.sh` + `SHARED="$CARGO_TARGET_DIR"` + `LOCAL_TARGET="$EHR_LOCAL_TARGET_DIR"`。
- ✅ **T29-⑤ 新增两条判据（判据 D/E）**：**D 产物根目录不得有第二份实现**（除唯一来源 `ci/env.sh` 外，任何文件都不得写死 `medme-shared-target` 这个默认根）；**E 产物路径变量不得自带兜底默认值**（`.py` 里 `os.environ.get("EHR_BIN", <默认路径>)` ⇒ 红 ⇒ 必须是「没注入就明确失败」）。
- ✅ **T29-⑥ 顺带修掉判据 C 本身的一个洞（T28 遗留）**：判据 C 原判「文件里出现过 `ci/env.sh` 这个字符串」⇒ **注释里提一句也算数** ⇒ 负向实测：把 `artifact-freshness-check.sh` 的 source 行删掉（只留注释里的「唯一来源（ci/env.sh）」）⇒ **门照样绿** ⇒ 判据 C 是空的 ⇒ 改为**只看真正的 source 语句**（非注释行 + 以 `.`/`source` 开头）⇒ 现已红。
- ✅ **T29-⑦ 顺带修掉判据 B 的一个假红（门红了先怀疑门，D-011 家族）**：判据 B 原判「token 含 `debug/ehr-api` 就红」⇒ 会把**变量派生**的 `$LOCAL_TARGET/debug/ehr-api`（来自唯一来源）也判红 ⇒ 改为「**字面量**产物路径才红」（token 里无变量引用才算）⇒ 变量派生放行。
- ⚠️ **T29-⑧ Python 的注释判定必须用 `ast`/`tokenize`，不能套 shell 的 `#` 规则**：T28 的 `is_comment` 认「整行以 `#` 开头」⇒ 对 Python 的**模块 docstring**（`"""..."""`）无效 ⇒ docstring 里的历史说明（`/home/lbb/...`）会被误判 ⇒ 门**假红** ⇒ 本轮用 `ast` 精确取 docstring 节点 + `tokenize` 取 `COMMENT` ⇒ 只排除「真·非代码」。
- ✅ **T29-⑨ 证据链（缺一即算交付）**：
  - ①**`cargo test --all-targets --locked` 83 门全绿**（本轮**未改任何 Rust 代码**，`Cargo.toml`/`Cargo.lock` **未改动** ⇒ **零新依赖**；`git diff --name-only ehr-api/` = **0 个文件**）。
  - ②**统一入口一次跑完全部门禁：18 个脚本全绿，总 fails=0**（`run-all-gates.sh` ⇒ A1 PASS + 18 个 `[PASS] … fails=0`）。**SKIP 2 个**：`container-smoke.sh`（需先重建镜像）、`agent-e2e.sh`（需真·第三方 agent CLI）——**均写明原因**。
  - ③**门是「真门」的负向证据（本轮实撞 4 次，每次复原）**：①往 Python 内层注入机器专属路径 ⇒ **原 T28 门绿**、**新门红 fails=3**（判据 B/D/E）⇒ **洞已封**；②往 `artifact_freshness_check.py` 注入兜底默认值 ⇒ 红 fails=2；③删 `artifact-freshness-check.sh` 的 source 行（注释仍在）⇒ **原判据 C 绿**、**新判据 C 红**；④把 `skill-rest-check.sh` 的 source 行改成注释 ⇒ **原判据 C 绿**、**新判据 C 红**；⑤（**门自身语义修正的留证**）在 workflow 里再写一份默认根 ⇒ 红 `FAIL .github/workflows/ci-gates.yml:93 写死了产物根目录` ⇒ 复原后 fails=0。
  - ④**CI 在干净 checkout 上跑通的证据**：`git ls-files` 导出到 `/tmp/ci-sim3`（模拟 runner，无 `upstream-medme/`）⇒ `ci/env.sh` 正确跟随 `CARGO_TARGET_DIR`（`REL=/tmp/ci-sim3-target/release/ehr-api`、`LOCAL=/tmp/ci-sim3/ehr-api/target`）⇒ 可移植性门在干净 checkout 上 **fails=0**（31 个受检文件）。
  - ⑤**参数生效要有证据**：`ci/env.sh` 新增契约项 `EHR_LOCAL_TARGET_DIR` ⇒ **实测** `ROOT`/`LOCAL`/`REL`/`SHARED` 四个变量在「不同 cwd + 覆盖 `CARGO_TARGET_DIR`」两种情形下都正确（`/tmp/ci-sim3` 一轮）。
  - ⑥**后台卡死检查（D-010）**：**无 cargo/rustc 残留**；本轮收割了 1 个上一轮遗留的**临时取证进程**（`:18990`，跑 ~1.1 小时）；常驻只剩两个 **10-21 的陈旧** `ehr-api` 进程（`:18999`/`:18991`）与两个 docker 容器（`:8585`/`:8587`）⇒ **非卡死**。
  - ⑦**零新依赖**：本轮**未改任何 Rust 代码** ⇒ 零 Cargo 依赖；只改了 1 个门禁的内层实现 + 唯一来源 + workflow + 判据本身。
- ⚠️ **本轮踩坑并已修（记录在案）**：**判据本身也要能被质疑**——本轮先让新门**假红**了一次（把变量派生的 `$LOCAL_TARGET/debug/ehr-api` 判红）⇒ 处置：改判据语义（「字面量 vs 变量派生」）⇒ 再复跑确认绿。**教训沿用 D-011 家族「门红了先怀疑门」**：新写判据的第一版**几乎必然有语义偏差** ⇒ 必须靠**制造缺陷**来确认它红、靠**正常写法**来确认它不红。
- 🔄 **下一步**：①**CI 尚未在真 GitHub runner 上跑过**（本轮仍是**本地干净 checkout 逐步实跑**模拟 ⇒ 待下一次 push 到 `main` 做第一次真 runner 验证）；②**T5 Web 管理端正式化**（目标验收①最后一块，**选型先报方案再动手**）；③「体检报告 PDF 列布局 labs=0」缺口：extraction 管线已就位，缺的是「把 PDF 送进 LLM」这一步的**端到端脚本**；④「家庭自测趋势」专用视图仍未做；⑤公网暴露前 `EHR_BIND_ADDR` 改 `0.0.0.0` 前**必须**先设 token（当前 OCR 编排 `EHR_API_TOKENS` 为空 ⇒ **放行模式**，仅本地）；⑥ghcr 包可见性仍 private（OCR 镜像也尚未推 GHCR）；⑦真·第三方 MCP 客户端互通仍未验；⑧`.dcm`/DICOM worker 未做（能力清单已如实标 `enabled=false`）；⑨**上游分类缺口**（`parser::classify` 认「出院记录」不认「出院小结」）仍在；⑩MCP 侧仍**无 resources/prompts**；⑪`docs/API.md` 是**生成物**，改接口须跑 `./api-doc-check.sh --write`；⑫人员域三端点仍 501 且刻意不进 OpenAPI；⑬**OCR 镜像未推 GHCR / 未纳入 CI**；⑭`container-smoke.sh` 仍**未跑**（须先重建镜像）；⑮**CI 真 runner 验证**（同①）。
- ⚠️ 未销账：目标验收①**Web 管理端仍是单文件最小版**（编译期内嵌，无构建链）；`docs/API.md` 是**生成物**；文档覆盖 **REST 面** + **MCP 面** + **SKILL.md 面**——**三份文档的面全部有门**；**上游分类缺口**仍在；**上游审计口径缺口**已在 D-030 如实标注（**不造事件补齐**）；**OCR 镜像尚未推 GHCR、未纳入 CI**；**「手抄实现细节」残留**已治到 **stub 清单 / REST 路径 / 响应形状 / 文档字段 / 产物新鲜度 / 门禁编排完整性 / MCP 工具契约字段 / SKILL.md 字段承诺 / CI 可移植性（**本轮扩到 Python 内层 + workflow**）** 十个面——**「统一入口接入 CI」本轮已落地**（workflow 已写、已在干净 checkout 实跑验证），**待下一次 push 到 `main` 做第一次真 runner 验证**。

## 当前状态（2026-10-27 · **T28「统一入口接入 CI」——「门禁在别人机器上跑不起来」这件事本身要有门：审计实测出 3 类真 portability 缺陷并修掉（其中 1 类是**门禁长期测旧产物**）+ 产物路径收敛为唯一来源 · D-045 落地**）
- ✅ **T28-① 接手工作树的第一件事仍是「让它编译并跑一遍」（D-039 新坑② 纪律沿用）**：`cd ehr-api && cargo test --all-targets` ⇒ **83 门全绿**（`mcp` 15 / `entries` 6 / `rest_manifest` 5 / `response_shape` 4 / `auth` 7 / `extraction` 5 / `meds` 2 / `conds` 4 / `audit` 3 / `rebuild` 3 / `capabilities` 3 / `apidoc` 3 / `lib` 21 / `contract` 1 / `sha_gate` 1）⇒ 确认**工作树可编译可跑**。（本轮**未改任何 Rust 代码** ⇒ `Cargo.toml`/`Cargo.lock` **未改动** ⇒ **零新依赖**。）
- ✅ **T28-② 缺口定位（PLAN 未销账项①的正面处置）**：「统一入口 `run-all-gates.sh` **尚未接入 CI**（仓库无 CI 配置 ⇒ 「统一入口」仍只是**本地约定**）」。**本轮没有「直接加个 workflow 就完事」**——本项目的历史教训（D-035~D-045）是「**说出来的事与机器判定的事必须一致**」⇒ 「统一入口能进 CI」这件事本身，先要回答一个前置问题：**这些脚本在别人的机器上跑得起来吗？** ⇒ 先做**可移植性审计**，审计**实测出 3 类真缺陷**（不是推演）。
- ✅ **T28-③ 新门 `ci-portability-check.sh`（+ 内层 `ci_portability_check.py`）——把「门禁能不能在别人机器上跑」变成机器判据**：判据**三条独立方向**：①**无机器专属绝对路径**（`/home/<用户名>/` 这类绑定到某台机器的路径 ⇒ 命中即红；**放行** `$HOME/...`/`/tmp/...`，**容器内路径**（`/home/ehr/`）不算——它是镜像内固定路径，跨机器一致）；②**产物路径单一来源**（门禁脚本里不许自己拼 `.../debug/ehr-api`/`.../release/ehr-api` ⇒ 必须用 `$EHR_BIN`/`$EHR_DBG_BIN`/`$EHR_REL_BIN`）；③**起真进程的门禁都 source 唯一来源**（漏 source ⇒ 拿不到统一路径 ⇒ 红）。
  - **为什么必须有这道门**：直接把 `run-all-gates.sh` 塞进 workflow 是不负责任的——**它一旦接上 CI，第一次 push 就会红**，而那时红的是「CI 环境缺东西」而不是「代码有问题」⇒ 判据必须先落成**机器可判的门**，让「跑不起来」在**提交前**就红。
- ✅ **T28-④ 本轮实测修掉的真缺陷 3 类（全部是审计跑出来的，不是推演）**：
  - **① 硬编码机器专属路径 6 处**：5 个脚本写死 `/home/lbb/medme-shared-target`（`entries-e2e`/`audit-e2e`/`api-doc-check`/`conds-e2e`/`meds-e2e`）+ `web-e2e.sh` 写死 `/home/lbb/medme-ehr/upstream-medme/...` ⇒ **CI runner 上必然跑不起来**。
  - **② 产物路径有**两份**实现**：debug 侧走 `$CARGO_TARGET_DIR/debug/...`，release 侧**写死** `$HOME/medme-shared-target/release/...` ⇒ 改一处忘另一处 ⇒ 门禁静默跑旧产物（D-035 新坑③）⇒ **收敛为唯一来源 `ci/env.sh`**（`CARGO_TARGET_DIR` → `EHR_DBG_BIN`/`EHR_REL_BIN`/`EHR_BIN`），**16 个门禁脚本一律 source 它**。
  - **③ 门禁测旧产物 2 处（最严重，比「CI 跑不起来」更危险）**：**`web-e2e.sh`** 原先**只**打 `base`（默认 `:8585` = docker compose 起的服务）⇒ 而那是**上一次 `up --build` 时的镜像** ⇒ 本门长期测**几周前的旧产物**，而**门禁照样是绿的**——**D-035 新坑③在「Web 端」这个面上复发**（其余 16 道门都起自己的真进程，只有这道门指向外部服务）⇒ **默认改为自起临时 ehr-api（跑当前产物）**，外部服务改为**显式 opt-in**（`base=`）并打印实际 base 留证据；**`auth-e2e.sh`** 用**相对路径** `./target/debug/ehr-api` 而 `cargo build` 写进 `$CARGO_TARGET_DIR` ⇒ 两者不同时它启动的是**另一份（可能很旧的）产物** ⇒ 改为 `$EHR_DBG_BIN`。
  - ⚠️ **为什么这类缺陷最危险**：它**不会让 CI 红**，它让 CI **绿得没有意义** ⇒ 纪律升级：**任何门禁若指向外部服务（容器/远端/既有进程），必须显式声明「那份服务由谁保证是当前的」**；否则就**自己起进程**。
- ✅ **T28-⑤ 上游源码拉取共用一份实现（`ci/fetch-upstream.sh`）**：原先 `ci-image.yml` 内联了一份拉取逻辑；本轮 `ci-gates.yml` 需要同一份 ⇒ 两份会各自漂移（「镜像能构建但门禁跑不了」或反之）⇒ 抽成脚本，**两个 workflow 都调它**。**通道照 D-004~D-007**：直连 codeload 优先，失败退 `ghfast.top`，两者都失败 ⇒ 退出非零（不静默跳过）。**护栏**：检测到 `upstream-medme/.git` ⇒ **拒绝覆盖**（不弄脏开发者的浅克隆）；**幂等**：已是同一 commit ⇒ 跳过下载。
- ✅ **T28-⑥ 新增 CI workflow `.github/workflows/ci-gates.yml`——「统一入口」从此不是本地约定**：触发 `push main` / `pull_request` / `workflow_dispatch`；步骤 = 装工具链 → `ci/fetch-upstream.sh` → TUNA 通道 → `cargo build --locked`（debug+release）→ `cargo test --all-targets --locked` → 同步产物（两份 sha256 逐字一致）→ **跑 `run-all-gates.sh`** ⇒ 「门禁全绿」成为**每次 push 自动判定**的事实。**只跑门禁、不推镜像**（镜像发布仍是独立的 `ci-image.yml`，走 tag 触发）⇒ 两者职责不混。
- ✅ **T28-⑦ 门是「真门」的负向证据（本轮实撞 3 次，每次复原）**：①给 `entries-e2e.sh` 注入硬编码 `/home/lbb/...` ⇒ 红 2 条（机器专属路径 + 自己拼产物路径）；②删掉 `meds-e2e.sh` 的 source 行 ⇒ 红 `FAIL meds-e2e.sh 起了真进程却没 source ci/env.sh`；③把 `skill-contract-check.sh` 的 `BIN` 改回 `$HOME/medme-shared-target/release/ehr-api` ⇒ 红 `自己拼了产物路径` ⇒ 复原后 fails=0。
- ✅ **T28-⑧ 证据链（缺一即不算交付）**：
  - ①**`cargo test --all-targets` 83 门全绿**（本轮**未改任何 Rust 代码**，**零新依赖**）。
  - ②**统一入口一次跑完全部门禁：18 个脚本全绿，总 fails=0**（`run-all-gates.sh` ⇒ A1 PASS + 18 个 `[PASS] … fails=0`）；**新门已登记进 RUN 清单** ⇒ A1 完整性仍 PASS。**SKIP 2 个**：`container-smoke.sh`（需先重建镜像）、`agent-e2e.sh`（需真·第三方 agent CLI）——**均写明原因**。
  - ③**CI 在真环境里跑通的证据（不是「应该能跑」）**：用 `git ls-files` 导出**干净 checkout**（**无 `upstream-medme/`**，模拟 runner）到 `/tmp/ci-sim2`，按 `ci-gates.yml` 的**每一步**实跑：①`ci/fetch-upstream.sh` ⇒ 直连 codeload 成功（5s / 58MB）⇒ 拉齐 `packages/` + `examples/sample-records/`；②TUNA 通道 + `cargo build --locked` ⇒ **debug 产物编译成功**（零新依赖）；③`ci/env.sh` 在 `CARGO_TARGET_DIR` 被覆盖时正确跟随。
  - ④**后台卡死检查（D-010）**：**无 cargo/rustc 残留**；本轮清理了上一轮遗留的 **4 个临时取证进程**（`:18777`/`:18778`/`:18799`/`:18811`，各跑 ~2 小时）；常驻只剩两个 **10-21 的陈旧** `ehr-api` 进程（`:18999`/`:18991`）与两个 docker 容器（`:8585`/`:8587`）⇒ **非卡死**。
  - ⑤**零新依赖**：本轮**未改任何 Rust 代码**（`Cargo.toml`/`Cargo.lock` **未改动**）⇒ 零 Cargo 依赖；只新增 2 个 CI 辅助脚本 + 1 个门禁脚本及其内层实现 + 1 个 workflow。
- ⚠️ **本轮踩坑并已修（记录在案）**：`ci/fetch-upstream.sh` **第一版**没有「拒绝覆盖 git 浅克隆」的护栏 ⇒ 我在开发机上实跑它 ⇒ 它把 `upstream-medme/packages/` 重拷了一遍 ⇒ **内容逐字未变**（`git status` 0 行）但 **mtime 全部刷新** ⇒ **T24 新鲜度门立刻红**（`产物比源码旧`）⇒ 处置：①脚本加护栏；②把上游文件 mtime 还原为该 commit 的提交时间 ⇒ 门恢复绿。**教训**：「不破坏现有状态」不是新脚本的加分项，而是**准入条件**——尤其当它写进**别人（CI 与开发机）共用的目录**时。
- 🔄 **下一步**：①**CI 尚未在真 GitHub runner 上跑过**（本轮是**本地干净 checkout 逐步实跑**模拟 ⇒ 通道/脚本/产物路径已验证，但 runner 的实际环境差异（磁盘/权限/网络）仍未验；**下次 push 到 `main` 应当是第一次真跑**）；②**T5 Web 管理端正式化**（目标验收①最后一块，**选型先报方案再动手**）；③「体检报告 PDF 列布局 labs=0」缺口：extraction 管线已就位，缺的是「把 PDF 送进 LLM」这一步的**端到端脚本**；④「家庭自测趋势」专用视图仍未做；⑤公网暴露前 `EHR_BIND_ADDR` 改 `0.0.0.0` 前**必须**先设 token（当前 OCR 编排 `EHR_API_TOKENS` 为空 ⇒ **放行模式**，仅本地）；⑥ghcr 包可见性仍 private（OCR 镜像也尚未推 GHCR）；⑦真·第三方 MCP 客户端互通仍未验；⑧`.dcm`/DICOM worker 未做（能力清单已如实标 `enabled=false`）；⑨**上游分类缺口**（`parser::classify` 认「出院记录」不认「出院小结」）仍在；⑩MCP 侧仍**无 resources/prompts**；⑪`docs/API.md` 是**生成物**，改接口须跑 `./api-doc-check.sh --write`；⑫人员域三端点仍 501 且刻意不进 OpenAPI；⑬**OCR 镜像未推 GHCR / 未纳入 CI**；⑭`container-smoke.sh` 仍**未跑**（须先重建镜像）；⑮`ci-portability-check.sh` 判据**未覆盖 Python 内层实现**（当前只查 `*.sh`）。
- ⚠️ 未销账：目标验收①**Web 管理端仍是单文件最小版**（编译期内嵌，无构建链）；`docs/API.md` 是**生成物**；文档覆盖 **REST 面**（`rest-contracts.md`）+ **MCP 面**（`tool-contracts.md`）+ **SKILL.md 面**（「字段承诺表」）——**三份文档的面全部有门**；**上游分类缺口**仍在；**上游审计口径缺口**已在 D-030 如实标注（**不造事件补齐**）；**OCR 镜像尚未推 GHCR、未纳入 CI**；**「手抄实现细节」残留**已治到 **stub 清单 / REST 路径 / 响应形状 / 文档字段 / 产物新鲜度 / 门禁编排完整性 / MCP 工具契约字段 / SKILL.md 字段承诺 / CI 可移植性** 九个面——**「统一入口接入 CI」本轮已落地**（workflow 已写、已在干净 checkout 实跑验证），**待下一次 push 到 `main` 做第一次真 runner 验证**。

## 当前状态（2026-10-27 · **T27「SKILL.md 字段面」纳入机器核对——agent 第一手说明里的字段承诺也必须与真响应一致，2 处真漂移已修 · D-044 落地**）
- ✅ **T27-① 接手工作树的第一件事仍是「让它编译并跑一遍」（D-039 新坑② 纪律沿用）**：`cd ehr-api && cargo test --all-targets` ⇒ **83 门全绿**（`mcp` 15 / `entries` 6 / `rest_manifest` 5 / `response_shape` 4 / `auth` 7 / `extraction` 5 / `meds` 2 / `conds` 4 / `audit` 3 / `rebuild` 3 / `capabilities` 3 / `apidoc` 3 / `lib` 21 / `contract` 1 / `sha_gate` 1）⇒ 确认**工作树可编译可跑**。（本轮**未改任何 Rust 代码** ⇒ `Cargo.toml`/`Cargo.lock` **未改动** ⇒ **零新依赖**。）
- ✅ **T27-② 缺口定位（D-043 未销账①的正面处置）**：D-040 把 **REST 面**（`rest-contracts.md`）、D-043 把 **MCP 面**（`tool-contracts.md`）纳入了机器核对；**`SKILL.md`——agent 真正读到的第一手说明——的字段承诺至今无门**。既有门 `skill-contract-check.sh` 只核**工具数/路径数/工具名**；`skill-rest-check.sh` 核的是**它自己手写的期望字段表**（又是一份手抄清单，且**不来自 SKILL.md**）⇒ **SKILL.md 里的字段名会悄悄变假** ⇒ agent 照它取字段 → 取不到 → 数据全空（D-016 家族的病，主角换成 SKILL.md）。
- ✅ **T27-③ 新门 `skill-doc-field-check.sh`（+ 内层 `skill_doc_field_check.py`）——把「SKILL.md 承诺的字段」纳入机器核对**：判据**五条独立方向**：①**表格正向（逐命令）**：SKILL.md 新增的「**字段承诺表**」为某命令承诺的每个字段名，必须出现在**该命令自己**的真响应里；②**表格反向（逐命令）**：该命令真响应的**契约级键**（顶层 + 数组元素一层 + 元素内对象一层）必须被表格提及；③**散文 ↔ 表格一致（逐命令）**：命令速查（散文）里提到的字段必须在表格同一命令行里（跨命令引用**允许**，但那行必须**点名 owner 命令**，否则 agent 无处可寻）；④**命令面真实（双向）**：SKILL.md 提到的每个 `ehr.sh <命令>` 必须 helper 真支持（真源 = `ehr.sh --help`），helper 支持的每个命令都必须被 SKILL.md 提到；⑤**数量真实**：工具数与「读侧 N + 写侧 M」== 实现派生（`--print-mcp-tools`）。
  - **为什么必须「逐命令」**（D-043 决策②的纪律迁移）：把多个响应拼一起查 ⇒ 字段在**任意一个**命令里出现过就算过 ⇒ 「A 命令承诺 B 命令才有的字段」照样绿 ⇒ **门在测错对象**。
  - **门自己造数**（1 人 + 3 份不同类型原件 + 3 类手动条目 + 1 次 extraction 回写）⇒ `ocr_meta`/`extraction`/`verify_detail`/`values[]` 这类「有内容才出现」的键才存在 ⇒ 空库跑恒绿的问题被治住。
  - **枚举值 vs 字段名由判据裁决**（D-011 家族纪律）：`status:ok` 的 `ok`、`source=registry` 的 `registry` 是**值** ⇒ 只有当该名字**不是任何响应的键、却等于某个响应的标量值**时才排除（打印 `INFO … 枚举值(非字段,已排除)`），**不靠正则猜**。
  - **非 JSON 输出单独分流**：`ehr.sh original` 回的是原始字节流 ⇒ 不参与字段级判据（否则门会拿 JSON 判据去套二进制）。
- ✅ **T27-④ 门是「真门」的负向证据（本轮实撞 5 次，每次复原）**：①表格给 `documents` 注入只属 `labs` 的 `series_count` ⇒ 红 `FAIL documents 表格承诺但真响应没有的字段: ['series_count']`；②表格删掉 `persons` 的 `id_note` ⇒ 红 `FAIL persons 真响应有但表格未提及的字段: ['id_note']`；③表格漏掉 `labs` 的 `total_points` ⇒ 红 `FAIL labs 真响应有但表格未提及的字段: ['total_points']`；④工具数改 13→12 ⇒ 红 `FAIL SKILL.md 工具数=12 ≠ 实现派生 13`；⑤把 `documents` 行的跨命令引用去掉 owner 点名 ⇒ 红 `FAIL documents 散文提到但表格未列、且该行未点名拥有它的命令: ['docref']` ⇒ 复原后 fails=0。
  - ⚠️ **门自身的第一版有洞，已修**（D-043 新坑②「门红了有两种可能」的同款纪律）：**首版只做「承诺存在」单方向、且把散文当承诺面** ⇒ 负向①「A 命令承诺 B 命令的字段」与负向②「漏字段」**都漏过**（实测两次绿）⇒ 处置：**引入机器可解析的「字段承诺表」**（散文降级为一致性核对面）⇒ 负向 5/5 全红。
- ✅ **T27-⑤ 本轮实测修掉的真漂移 2 处**（`SKILL.md` 落后于实现）：①`documents` 行原写「`content_hash` = 原件 sha256 = `docref`」——**`docref` 根本不是 `documents` 响应的键**（它只存在于 `labs`/`medications`/`measurements`）⇒ agent 照文档去 `documents` 取 `docref` ⇒ **取不到**；②「字段承诺表」里 `detail` 的 `extraction.raw` 被写成必有键——**真响应只在 `parseable=false` 时才有 `raw`** ⇒ 属「文档过度承诺」⇒ 已标为**可选键**。
- ✅ **T27-⑥ 证据链（缺一即不算交付）**：
  - ①**`cargo test --all-targets` 83 门全绿**（本轮**未改任何 Rust 代码**，**零新依赖**）。
  - ②**统一入口一次跑完全部门禁：17 个脚本全绿，总 fails=0**（`run-all-gates.sh` ⇒ A1 PASS + 17 个 `[PASS] … fails=0`）；**新门已登记进 RUN 清单** ⇒ A1 完整性仍 PASS。**SKIP 2 个**：`container-smoke.sh`（运行中容器是旧镜像）、`agent-e2e.sh`（需真·第三方 agent CLI）——**均写明原因**。
  - ③**后台卡死检查（D-010）**：**无 cargo/rustc 残留**；常驻只有两个 **10-21 的陈旧** `ehr-api` 进程（`:18999`/`:18991`）、两个 docker 容器（`:8585`/`:8587`）+ 上一轮遗留的 4 个临时取证进程 ⇒ **非卡死**（本轮探针进程已清理）。
  - ④**零新依赖**：本轮只新增**一个门禁脚本 + 其内层 Python 实现**（无 Rust 改动、零 Cargo 依赖）＋ 文档与清单回写。
- 🔄 **下一步**：①**统一入口尚未接入 CI**（仓库无 CI 配置 ⇒ 「统一入口」仍只是**本地约定**；这是当前文档面**最后一块无门的地基**）；②**T5 Web 管理端正式化**（目标验收①最后一块，**选型先报方案再动手**）；③「体检报告 PDF 列布局 labs=0」缺口：extraction 管线已就位，缺的是「把 PDF 送进 LLM」这一步的**端到端脚本**；④「家庭自测趋势」专用视图仍未做；⑤公网暴露前 `EHR_BIND_ADDR` 改 `0.0.0.0` 前**必须**先设 token（当前 OCR 编排 `EHR_API_TOKENS` 为空 ⇒ **放行模式**，仅本地）；⑥ghcr 包可见性仍 private（OCR 镜像也尚未推 GHCR）；⑦真·第三方 MCP 客户端互通仍未验；⑧`.dcm`/DICOM worker 未做（能力清单已如实标 `enabled=false`）；⑨**上游分类缺口**（`parser::classify` 认「出院记录」不认「出院小结」）仍在；⑩MCP 侧仍**无 resources/prompts**；⑪`docs/API.md` 是**生成物**，改接口须跑 `./api-doc-check.sh --write`；⑫人员域三端点仍 501 且刻意不进 OpenAPI；⑬**OCR 镜像未推 GHCR / 未纳入 CI**；⑭`container-smoke.sh` 仍**未跑**（须先重建镜像）。
- ⚠️ 未销账：目标验收①**Web 管理端仍是单文件最小版**（编译期内嵌，无构建链）；`docs/API.md` 是**生成物**；文档覆盖 **REST 面**（`rest-contracts.md`）+ **MCP 面**（`tool-contracts.md`）+ **SKILL.md 面**（本轮新增「字段承诺表」）——**三份文档的面全部有门**；**上游分类缺口**仍在；**上游审计口径缺口**已在 D-030 如实标注（**不造事件补齐**）；**OCR 镜像尚未推 GHCR、未纳入 CI**；**「手抄实现细节」残留**已治到 **stub 清单 / REST 路径 / 响应形状 / 文档字段 / 产物新鲜度 / 门禁编排完整性 / MCP 工具契约字段 / SKILL.md 字段承诺** 八个面——**统一入口仍未接入 CI**。

## 当前状态（2026-10-26 · **T26「MCP 工具契约字段面」纳入机器核对——`tool-contracts.md` 落后于实现 10 处已修，判据必须「逐工具」 · D-043 落地**）
- ✅ **T26-① 接手工作树的第一件事仍是「让它编译并跑一遍」（D-039 新坑② 纪律沿用）**：`cd ehr-api && cargo test --all-targets` ⇒ **83 门全绿**（`mcp` 15 / `entries` 6 / `rest_manifest` 5 / `response_shape` 4 / `auth` 7 / `extraction` 5 / `meds` 2 / `conds` 4 / `audit` 3 / `rebuild` 3 / `capabilities` 3 / `apidoc` 3 / `lib` 21 / `contract` 1 / `sha_gate` 1）⇒ 确认**工作树可编译可跑**，再谈别的。（本轮**未改任何 Rust 代码** ⇒ `Cargo.toml`/`Cargo.lock` **未改动** ⇒ **零新依赖**。）
- ✅ **T26-② 缺口定位（D-041 未销账①）**：T23（D-040）只把 **REST 面**（`rest-contracts.md`）纳入机器核对；**MCP 面**（`tool-contracts.md` 的「structuredContent 关键字段」列）仍是**人读文档里的手抄实现细节** ⇒ 实现改了它**不会红** ⇒ agent 照文档取字段 → 取不到 → 数据全空（D-016 家族）。`skill-contract-check.sh` 只核**工具数/路径数/工具名**，**不核字段** ⇒ 缺口真实存在。
- ✅ **T26-③ 新门 `mcp-contract-check.sh`（+ 内层 `mcp_contract_check.py`）——把「MCP 契约文档」也纳入机器核对**：判据**三条独立方向**：①**正向（逐工具）**：文档承诺的每个字段名必须出现在**该工具自己**的真响应里；②**反向（逐工具）**：该工具真响应的键（顶层 + 数组元素一层 + 元素内对象一层）必须被文档提及；③**工具覆盖**：实现派生的 13 个工具都必须有文档行。门**自己造数**（1 人 + 3 份不同类型原件 + 3 类手动条目）⇒ `ocr_meta`/`extraction`/`values[]`/`event_package` 这类「有内容才出现」的键才存在 ⇒ 空库跑恒绿的问题被治住。
  - **为什么判据必须「逐工具」**：若把各工具响应拼一起查，字段只要在**任意一个**工具里出现过就算过 ⇒ 「A 工具承诺 B 工具才有的字段」照样绿 ⇒ **门在测错对象**（D-037 同族）。**负向证明**：把只属 `get_medications` 的 `atc`/`docref` 注入 `get_capabilities` ⇒ 门红。
  - **枚举值与字段名必须能区分**（D-011 家族纪律）：`status`(恒 `active`)、`source`(`registry`/`vault`) 这类**枚举值**不是字段名 ⇒ 只有当它**不是真响应的键、却等于真响应的标量值**时才排除（`INFO … 枚举值(非字段,已排除)`），**不靠正则猜**。
- ✅ **T26-④ 门是「真门」的负向证据（本轮实撞 3 次，每次复原）**：①**退回已提交版 `tool-contracts.md`** ⇒ 门红 **fails=10**（逐条与修复一一对应）⇒ 复原 fails=0；②**错工具注入**（`atc`/`docref` 写进 `get_capabilities`）⇒ 红 `FAIL get_capabilities 文档承诺但真响应没有的字段: ['atc','docref']` ⇒ 复原绿；③（**门自身解析缺陷的修正记录**）首版漏 `xxx[]` 容器名且把枚举值当字段 ⇒ 假红 16 条 ⇒ 修正解析 ⇒ 真实剩余 10 条 ⇒ 修文档后 0 条。
- ✅ **T26-⑤ 本轮实测修掉的真漂移 10 处**（`tool-contracts.md` 落后于实现）：`list_persons` 承诺 `person_id`（真键名 `id`）且漏 `created_at`/`updated_at`；`get_timeline` 漏 `document_id`/`doc_date_end`；`search_records` 漏 `document_id`；`get_labs` 承诺 `count`（真字段 `series_count`/`total_points`）且漏 `analyte_key`/`loinc`/`self_measured`/`ref_high`/`ref_high_canonical`/`values_converted`/`documents_scanned`；`get_medications` 漏 `atc`/`drug_key`/`latest_dose`/`latest_raw_name`/`unverified`；`get_measurements` 承诺 `ref_low`（可缺省）且漏 `title`/`event_package`；`list_documents` 漏 `document_id`/`mime_type`/`text_indexed`/`title`/`doc_date_end`。⇒ **7 个工具的字段承诺是假的/不完整的**，其中 4 处是「**漏字段**」（agent 照文档取不到 ⇒ 数据全空）——**这正是这道门要拦的失败模式，实测发生**。
- ✅ **T26-⑥ 证据链（缺一即不算交付）**：
  - ①**`cargo test --all-targets` 83 门全绿**（本轮**未改任何 Rust 代码**，**零新依赖**）。
  - ②**统一入口一次跑完全部门禁：16 个脚本全绿，总 fails=0**（`run-all-gates.sh` ⇒ A1 PASS + 16 个 `[PASS] … fails=0`）；**新门已登记进 RUN 清单** ⇒ A1 完整性仍 PASS。**SKIP 2 个**：`container-smoke.sh`（运行中容器是旧镜像）、`agent-e2e.sh`（需真·第三方 agent CLI）——**均写明原因**。
  - ③**后台卡死检查（D-010）**：**无 cargo/rustc 残留**；常驻只有两个 **10-21 的陈旧** `ehr-api` 进程（`:18999`/`:18991`）与两个 docker 容器（`:8585`/`:8587`）⇒ **非卡死**。
  - ④**零新依赖**：本轮只新增**一个门禁脚本 + 其内层 Python 实现**（无 Rust 改动）＋ 文档与清单回写。
- 🔄 **下一步**：①**`SKILL.md` 的字段描述面**仍未逐条核（本门核 `tool-contracts.md`）；②**把 `run-all-gates.sh` 纳入 CI**（当前仓库无 CI 配置 ⇒ 「统一入口」仍只是本地约定）；③**T5 Web 管理端正式化**（目标验收①最后一块，**选型先报方案再动手**）；④「体检报告 PDF 列布局 labs=0」缺口：extraction 管线已就位，缺的是「把 PDF 送进 LLM」这一步的**端到端脚本**；⑤「家庭自测趋势」专用视图仍未做；⑥公网暴露前 `EHR_BIND_ADDR` 改 `0.0.0.0` 前**必须**先设 token（当前 OCR 编排 `EHR_API_TOKENS` 为空 ⇒ **放行模式**，仅本地）；⑦ghcr 包可见性仍 private（OCR 镜像也尚未推 GHCR）；⑧真·第三方 MCP 客户端互通仍未验；⑨`.dcm`/DICOM worker 未做（能力清单已如实标 `enabled=false`）；⑩**上游分类缺口**（`parser::classify` 认「出院记录」不认「出院小结」）仍在；⑪MCP 侧仍**无 resources/prompts**；⑫`docs/API.md` 是**生成物**，改接口须跑 `./api-doc-check.sh --write`；⑬人员域三端点仍 501 且刻意不进 OpenAPI；⑭**OCR 镜像未推 GHCR / 未纳入 CI**；⑮`container-smoke.sh` 仍**未跑**（须先重建镜像）。
- ⚠️ 未销账：目标验收①**Web 管理端仍是单文件最小版**（编译期内嵌，无构建链）；`docs/API.md` 是**生成物**；文档覆盖 **REST 面**（`rest-contracts.md`）+ **MCP 面**（`tool-contracts.md`，本轮补齐）；**上游分类缺口**仍在；**上游审计口径缺口**已在 D-030 如实标注（**不造事件补齐**）；**OCR 镜像尚未推 GHCR、未纳入 CI**；**「手抄实现细节」残留**已治到 **stub 清单 / REST 路径 / 响应形状 / 文档字段 / 产物新鲜度 / 门禁编排完整性 / MCP 工具契约字段** 七个面——**`SKILL.md` 字段描述面**仍无门；**统一入口已存在但未接入 CI**。



### 历史状态（2026-10-26 · **T25「一键跑全部门禁」统一入口——漏跑一个脚本 = 那道门不存在，且「新增门禁忘了登记」由机器抓 · D-042 落地**）
- ✅ **T25-① 接手工作树的第一件事仍是「让它编译并跑一遍」（D-039 新坑② 纪律沿用）**：`cd ehr-api && cargo test --all-targets` ⇒ **83 门全绿**（`mcp` 15 / `entries` 6 / `rest_manifest` 5 / `response_shape` 4 / `auth` 7 / `extraction` 5 / `meds` 2 / `conds` 4 / `audit` 3 / `rebuild` 3 / `capabilities` 3 / `apidoc` 3 / `lib` 21 / `contract` 1 / `sha_gate` 1）。
- ✅ **T25-② 缺口定位（D-041 未销账②）**：仓库根有 **16 个门禁脚本**（`artifact-freshness` / `doc-contract` / `api-doc` / `skill-contract` / `skill-rest` / `mcp` / `auth` / `entries` / `conds` / `meds` / `audit` / `ehtest` / `rebuild` / `rebuild-lock` / `web-e2e` / `container-smoke` / `agent-e2e`），**全靠人工逐个跑** ⇒ ①**一次「我跑了全部」无法证明**（⇒ 那道门这一轮可能根本不存在）；②**清单本身也会漂移**（新增一个门禁脚本却忘了加进任何清单 ⇒ 新门永远不跑，**且没人知道**）——这正是 D-037「清单/注释会骗人」在**编排层**的复发。
- ✅ **T25-③ 新入口 `run-all-gates.sh`——把「我跑了全部」变成机器判据**：判据**三条独立方向**：①**A1 登记表完整性**（仓库根每个 `*.sh` 门禁脚本必须**要么在 RUN 清单、要么在 SKIP 清单且写明跳过原因** ⇒ 新增门禁忘了登记 ⇒ 立刻红）；②**A2 每道门真跑且真绿**（退出码为 0 **且**输出里 `fails=N` 必须为 0 ⇒ 两个都要看：有的脚本非零退出不改计数、有的脚本计数非零却仍 exit 0）；③**A3 汇总可见**（逐门 PASS/FAIL + 总 fails ⇒ 任一红整体非零）。
  - **为什么 A1 必须是「文件系统派生 vs 清单」核对**：登记完整性若靠**人记得**（写在注释里），它就是 D-037 同族的假门 ⇒ 判据必须是**目录里的真实文件集合**与清单**逐条相等**。
  - **为什么 SKIP 也必须写原因**：允许「跳过」但要求写明理由 ⇒ 「跳过」变成**有账可查的决定**而不是隐藏。
  - **A2 为什么退出码和 fails 计数都要看**：二者是**两个不同事实**（D-041 决策③「判据形式要匹配被测事实的性质」的复用）——只看退出码会漏掉「计数红但 exit 0」的脚本，只看计数会漏掉「早退非零但没打印计数」的脚本。
- ✅ **T25-④ 门是「真门」的负向证据（本轮实撞 3 次，每次复原）**：①新增一个未登记的 `zz-fake-gate.sh` ⇒ **A1 红**（`[FAIL] A1 门禁脚本未登记：zz-fake-gate.sh ⇒ … 这道门这一轮不会被跑`，rc=1）⇒ 复原 rc=0；②把一个「打印 `fails=1` 但 exit 0」的假绿脚本登记进 RUN ⇒ **A2 红**（`[FAIL] zz-broken-gate.sh 退出码=0 但 fails=1 ≠ 0`）⇒ 复原；③把一个 `exit 1` 的脚本登记进 RUN ⇒ **A2 红**（`退出码=1` + 回显 `FAIL something bad`）⇒ 复原 ⇒ **A2 对「假绿」与「真红」两种失败都敏感**。
- ✅ **证据链（缺一即不算交付）**：
  - ①**`cargo test --all-targets` 83 门全绿**（本轮**未改任何 Rust 代码**；`ehr-api/Cargo.toml` / `Cargo.lock` **未改动** ⇒ **零新依赖**）。
  - ②**统一入口一次跑完全部门禁：15 个脚本全绿，总 fails=0**（`run-all-gates.sh` ⇒ A1 PASS + 15 个 `[PASS] … fails=0`）。**SKIP 2 个**：`container-smoke.sh`（运行中容器是 10-21 旧镜像 ⇒ 须先重建镜像才有意义）、`agent-e2e.sh`（需真·第三方 agent CLI + 外部服务态）——**两个都写明了原因**。
  - ③**A1 完整性自证**：`./run-all-gates.sh --list` ⇒ `A1 登记表完整（RUN=15 SKIP=2）`，且根目录 `*.sh` 全数覆盖（`ls *.sh` = 18 个，含入口自身与两个 SKIP）。
  - ④**后台卡死检查（D-010）**：**无 cargo/rustc 残留**；常驻只有两个 **10-21 的陈旧** `ehr-api` 进程（`:18999` / `:18991`，跑的是 10-21 编译的旧产物 ⇒ **非卡死**）与两个 docker 容器（`:8585` 默认 12 小时前 / `:8587` OCR 8 小时前）⇒ **非卡死**。
  - ⑤**零新依赖**：本轮只新增**一个门禁入口脚本**（无内层 Python 实现）＋ PLAN/DECISIONS 的回写，**未动任何 Rust 代码**。
- 🔄 **下一步**：①**`SKILL.md` / `tool-contracts.md` 的字段面尚未纳入 D-040 那道门**（现成思路可复用；`skill-contract-check.sh` 只核工具数/路径，**不核工具契约字段**）；②**把 `run-all-gates.sh` 纳入 CI**（当前它只是本地入口；**没有 CI 配置文件** ⇒ 「统一入口」仍只是本地约定）；③**T5 Web 管理端正式化**（目标验收①最后一块，**选型先报方案再动手**）；④「体检报告 PDF 列布局 labs=0」缺口：extraction 管线已就位，缺的是「把 PDF 送进 LLM」这一步的**端到端脚本**；⑤「家庭自测趋势」专用视图仍未做；⑥公网暴露前 `EHR_BIND_ADDR` 改 `0.0.0.0` 前**必须**先设 token（当前 OCR 编排 `EHR_API_TOKENS` 为空 ⇒ **放行模式**，仅本地）；⑦ghcr 包可见性仍 private（OCR 镜像也尚未推 GHCR）；⑧真·第三方 MCP 客户端互通仍未验；⑨`.dcm`/DICOM worker 未做（能力清单已如实标 `enabled=false`）；⑩**上游分类缺口**（`parser::classify` 认「出院记录」不认「出院小结」）仍在；⑪MCP 侧仍**无 resources/prompts**；⑫`docs/API.md` 是**生成物**，改接口须跑 `./api-doc-check.sh --write`；⑬人员域三端点仍 501 且刻意不进 OpenAPI；⑭**OCR 镜像未推 GHCR / 未纳入 CI**；⑮`container-smoke.sh` 仍**未跑**（须先重建镜像）。
- ⚠️ 未销账：目标验收①**Web 管理端仍是单文件最小版**（编译期内嵌，无构建链）；`docs/API.md` 是**生成物**；文档覆盖 **REST 面**，`/mcp` 的工具契约在 `skills/medme-ehr/references/tool-contracts.md`；**上游分类缺口**仍在；**上游审计口径缺口**已在 D-030 如实标注（**不造事件补齐**）；**OCR 镜像尚未推 GHCR、未纳入 CI**；**「手抄实现细节」残留**已治到 **stub 清单 / REST 路径 / 响应形状 / 文档字段 / 产物新鲜度 / 门禁编排完整性** 六个面——**「MCP 工具契约字段面」仍无门**；**统一入口已存在但未接入 CI**。

### 历史状态（2026-10-26 · **T24「产物新鲜度」纳入机器核对——门禁跑的那份二进制必须不比源码旧，两份产物必须逐字一致 · D-041 落地**）

- ✅ **T24-① 接手工作树的第一件事仍是「让它编译并跑一遍」**（D-039 新坑② 纪律沿用）：`cd ehr-api && cargo test --all-targets` ⇒ **83 门全绿**（`mcp` 15 / `entries` 6 / `extraction` 5 / `rest_manifest` 5 / `response_shape` 4 / `auth` 7 / `conds` 4 …）⇒ 确认**工作树可编译可跑**，再谈别的。
- ✅ **T24-② 缺口定位（D-040 未销账②）**：`ehr-api/target/{debug,release}/ehr-api`（本地构建目录）与 `~/medme-shared-target/{debug,release}/ehr-api`（**多数 e2e 脚本实际跑的那份**：`EHR_BIN` / `CARGO_TARGET_DIR` 的默认值都指向后者）⇒ **两份产物靠人工 `cp` 同步，没有任何门** ⇒ 改完 Rust 代码忘了同步 ⇒ **门禁静默跑旧产物、门是绿的、测的却是上一轮代码**（D-035 新坑③「先问它跑的是哪一份产物」的正面处置）。
- ✅ **T24-③ 新门 `artifact-freshness-check.sh`（+ 内层 `artifact_freshness_check.py`）——把「在跑哪一份产物」变成机器判据**：判据**两条独立方向**：①**新鲜度**：产物 mtime 必须**不早于**最新源码 mtime（源码集合 = 本仓 `ehr-api/src` + `ehr-api/tests` + `Cargo.toml` + `Cargo.lock` **以及上游只读依赖 `upstream-medme/packages` 的全部 `.rs`/`.toml`**——上游改了同样会让产物过期）；②**两份一致**：同一 profile 下 local 与 shared 产物必须 **sha256 逐字相等**（= 没忘了同步、也没只同步一半）。⇒ 门**打印产物路径 + 构建时间 + sha256 + 源码最新时间**，让人一眼看出在跑哪一份。
  - **为什么用 mtime 而不是内容哈希**：Rust 产物里**没有嵌入源码哈希**，而 cargo 本身也用 mtime 做增量判定 ⇒ 与 cargo 语义一致（1s 容差，防文件系统时间戳精度）。
  - **为什么 sha256 判据必须独立存在**：mtime 只能判「新旧」，判不了「**同一份**」——把 shared 产物覆盖成另一份**新**的二进制，mtime 仍是最新的 ⇒ 只有内容哈希能抓到。
- ✅ **T24-④ 门是「真门」的负向证据（本轮实撞 2 次，每次复原）**：①`touch ehr-api/src/lib.rs`（让源码比产物新）⇒ 门 **rc=1**，逐条红 `FAIL debug/local 产物比源码旧（产物=2026-10-21 23:14:04 源码=2026-10-22 02:03:18）⇒ 门禁会静默跑旧产物`（debug/local / debug/shared / release/local / release/shared **四条全红**）⇒ 复原 mtime ⇒ **rc=0**；②往 `~/medme-shared-target/release/ehr-api` 追加 1 字节（模拟「忘了同步」）⇒ 门 **rc=1**，红 `FAIL release 两份产物 sha256 不一致 ⇒ 忘了同步（local=… shared=…）` ⇒ 复原 ⇒ **rc=0**。
- ✅ **证据链（缺一即不算交付）**：
  - ①**`cargo test --all-targets` 83 门全绿**（本轮**未改任何 Rust 代码**；`ehr-api/Cargo.toml` / `Cargo.lock` **未改动** ⇒ **零新依赖**）。
  - ②**新门绿态输出可验**：`源码最新 mtime = 2026-10-21 23:14:01 (ehr-api/src/lib.rs)`；`release/shared` = `/home/lbb/medme-shared-target/release/ehr-api`，`sha256=bf2c42be…deb6d7`，与 `release/local` **逐字相等** ⇒ **本轮确实没有「跑旧产物」的隐患**（当前源码最新 10-21 23:14，产物 10-22 01:01/01:02）。
  - ③**门禁全量复跑 15 个脚本 fails=0**：`artifact-freshness-check.sh`（**新**）/ `doc-contract-check.sh` / `api-doc-check.sh` / `skill-contract-check.sh` / `skill-rest-check.sh` / `mcp-e2e.sh` / `auth-e2e.sh` / `entries-e2e.sh` / `conds-e2e.sh` / `meds-e2e.sh` / `audit-e2e.sh` / `ehtest.sh` / `rebuild-e2e.sh` / `rebuild-lock-e2e.sh` / `web-e2e.sh`。
  - ④**后台卡死检查（D-010）**：**无 cargo/rustc 残留**；常驻只有两个 **10-21 的陈旧** `ehr-api` 进程（`:18999` / `:18991`）与两个 docker 容器（`:8585` 默认 / `:8587` OCR）⇒ **非卡死**；本轮**无需同步产物**（门已证明两份一致且不比源码旧）。
  - ⑤**零新依赖**：本轮只新增**一个门禁脚本**及其内层实现（+ 本文件/决策记录的回写），**未动任何 Rust 代码**。
- 🔄 **下一步**：①**`SKILL.md` / `tool-contracts.md` 的字段面尚未纳入 D-040 那道门**（该门只核 `rest-contracts.md`）⇒ 现成思路可复用；②**新鲜度门本身也应被纳入 CI/统一入口**（当前 15 个脚本是手工逐个跑，**没有「一键跑全部门禁」的入口** ⇒ 漏跑一个脚本就等于没那道门）；③**T5 Web 管理端正式化**（目标验收①最后一块，**选型先报方案再动手**）；④「体检报告 PDF 列布局 labs=0」缺口：extraction 管线已就位，缺的是「把 PDF 送进 LLM」这一步的**端到端脚本**；⑤「家庭自测趋势」专用视图仍未做；⑥公网暴露前 `EHR_BIND_ADDR` 改 `0.0.0.0` 前**必须**先设 token（当前 OCR 编排 `EHR_API_TOKENS` 为空 ⇒ **放行模式**，仅本地）；⑦ghcr 包可见性仍 private（OCR 镜像也尚未推 GHCR）；⑧真·第三方 MCP 客户端互通仍未验；⑨`.dcm`/DICOM worker 未做（能力清单已如实标 `enabled=false`）；⑩**上游分类缺口**（`parser::classify` 认「出院记录」不认「出院小结」）仍在；⑪MCP 侧仍**无 resources/prompts**；⑫`docs/API.md` 是**生成物**，改接口须跑 `./api-doc-check.sh --write`；⑬人员域三端点仍 501 且刻意不进 OpenAPI；⑭**OCR 镜像未推 GHCR / 未纳入 CI**；⑮`container-smoke.sh` 本轮**未跑**（运行中的容器是旧镜像 ⇒ 须先重建镜像才有意义）。
- ⚠️ 未销账：目标验收①**Web 管理端仍是单文件最小版**（编译期内嵌，无构建链）；`docs/API.md` 是**生成物**；文档覆盖 **REST 面**，`/mcp` 的工具契约在 `skills/medme-ehr/references/tool-contracts.md`；**上游分类缺口**仍在；**上游审计口径缺口**已在 D-030 如实标注（**不造事件补齐**）；**OCR 镜像尚未推 GHCR、未纳入 CI**；**「手抄实现细节」残留**已治到 **stub 清单 / REST 路径 / 响应形状 / 文档字段 / 产物新鲜度** 五个面——**「MCP 工具契约字段面」仍无门**；**「一键跑全部门禁」的统一入口仍无**。

### 历史状态（2026-10-26 · **T23「文档面」也纳入机器核对——`rest-contracts.md` 的字段承诺必须与真响应一致，9 处真漂移已修 · D-040 落地**）

- ✅ **T23-① 接手工作树的第一件事是「让它编译并跑一遍」（D-039 新坑② 的正面执行）**：工作树里躺着**上一轮遗留的 T23 草稿**（`doc-contract-check.sh` + `doc_contract_check.py` 两个**未跟踪**文件 + `rest-contracts.md` 的**已修改**但**未提交** diff）⇒ 按纪律**先跑**：`cargo test --all-targets` ⇒ **83 门全绿**（`response_shape` 4 / `rest_manifest` 5 / `mcp` 15 / `extraction` 5 / `entries` 6 …）⇒ 确认**工作树可编译、可跑**，再谈别的（**不把上一轮的绿灯误当成本轮现状**）。
- ✅ **T23-② 新门 `doc-contract-check.sh`（+ 内层 `doc_contract_check.py`）——把「文档面」从「人读文档里的手抄实现细节」变成机器判据**：①**它自己起真进程**（临时 `EHR_HOME` + `trap cleanup`）、**真造数据**（建档 / 上传 3 份不同类型原件：检验·处方·诊断 / 写 3 类手动条目：自测·笔记·动作事件 / PUT 一次 extraction）⇒ **不是解析源码、不是手抄端点清单**；②判据**三条独立方向**：**正向**（文档承诺的字段名必须真出现在真响应任意层级）＋**反向**（真响应的键必须被文档提及）＋**端点覆盖**（OpenAPI 每个 operation 都有一行）；③**枚举值 vs 子字段名不靠猜**——括号里的名字**只有在 OpenAPI 属性表里出现**才按子字段名必查，否则按枚举值跳过（**D-011 家族纪律：门不能有自己的隐含猜测**）；④**自由 JSON 载荷内部跳过**（`payload`/`text`/`raw`/`json`/`snippet`/`detail` ⇒ 用户数据不是服务端契约）。
- ✅ **T23-③ 门实测抓到并修掉 9 处真漂移（这才是本轮的交付）**：把 `rest-contracts.md` **退回已提交版本** ⇒ 门 **fails=9**，逐条清单：①`GET .../conditions` 承诺 `onset`（实际可缺省）且**漏写真键名 `raw_text`**（文档写「诊断原文」）；②`GET .../measurements` 承诺 `ref_low`（可缺省）；③`GET .../medications` 承诺 `start`/`end`（可缺省）；④`POST .../documents` 承诺 `doc_type`（无索引行时缺省）；⑤`GET .../documents` **漏写 `doc_date_end`**；⑥`GET .../documents/{sha}` **漏写 `ocr_meta` 的 3 个子字段 + `doc_date_end` + `encounter_id`**；⑦`GET .../labs` **漏写 `self_measured` / `ref_high` / `ref_high_canonical`**；⑧`GET .../search` **漏写 `document_id`**。⇒ **8 个端点的字段承诺是假的或不完整的，其中 3 处是「漏字段」** ⇒ **agent 照文档取字段会取不到 ⇒ 数据全空**（D-016 家族）——**这个失败模式在本项目真实发生过**。
- ✅ **T23-④ 门是「真门」的负向证据（本轮实撞 4 次，每次复原）**：①文档凭空加 `zzz_fake_field` ⇒ 红 `文档过度承诺`（fails=2）；②删掉 `GET /audit` 那一行 ⇒ 红 `doc_missing_endpoints: [('GET','/audit')]`（fails=2）；③从 `GET .../documents` 承诺列里删掉 `text_indexed` ⇒ 红 `doc_missing_fields: ['text_indexed']`（fails=1）；④**退回已提交版文档** ⇒ 红 fails=9（=③那批漂移）⇒ 复原修正版 ⇒ **fails=0**。
- ✅ **证据链（缺一即不算交付）**：
  - ①**`cargo test --all-targets` 83 门全绿**（本轮**未改任何 Rust 代码**；`Cargo.toml` / `Cargo.lock` **未改动** ⇒ **零新依赖**）。
  - ②**新门 38 个 PASS**：19 个 `doc_fields_present`（19 个 operation 的承诺字段数 2～24 不等）／19 个 `doc_covers_response`／`doc_covers_endpoints (19 个 operation 全有文档行)`／`doc_has_no_unknown_endpoints`。
  - ③**门禁全量复跑 14 个脚本 fails=0**：`doc-contract-check.sh`（新）/ `api-doc-check.sh` / `skill-contract-check.sh` / `skill-rest-check.sh` / `mcp-e2e.sh` / `auth-e2e.sh` / `entries-e2e.sh` / `conds-e2e.sh` / `meds-e2e.sh` / `audit-e2e.sh` / `ehtest.sh` / `rebuild-e2e.sh` / `rebuild-lock-e2e.sh` / `web-e2e.sh`。
  - ④**后台卡死检查（D-010）**：**无 cargo/rustc 残留**；常驻只有两个 **10-21 的陈旧** `ehr-api` 进程（`:18999` / `:18991`，跑的是 10-21 编译的旧产物 ⇒ **非卡死**）与两个 docker 容器（`:8585` 默认 10 小时前 / `:8587` OCR 6 小时前）⇒ **本轮已把新 release 产物同步到 `~/medme-shared-target/release/ehr-api`**（否则多数 e2e 脚本会**静默跑旧产物**——D-035 新坑③）。
  - ⑤**零新依赖**：本轮只改**一份文档**（`rest-contracts.md`）＋新增**一个门禁脚本**及其内层实现。
- 🔄 **下一步**：①**`SKILL.md` / `tool-contracts.md` 的字段面尚未纳入这道门**（本门只核 `rest-contracts.md`）；②**「产物新鲜度」没有门**（`ehr-api/target/release/ehr-api` 与 `~/medme-shared-target/release/ehr-api` 两份靠人工同步 ⇒ 建议加「门禁跑的是哪一份产物」的新鲜度门，打印产物路径+构建时间）；③**T5 Web 管理端正式化**（目标验收①最后一块，**选型先报方案再动手**）；④「体检报告 PDF 列布局 labs=0」缺口：extraction 管线已就位，缺的是「把 PDF 送进 LLM」这一步的**端到端脚本**；⑤「家庭自测趋势」专用视图仍未做；⑥公网暴露前 `EHR_BIND_ADDR` 改 `0.0.0.0` 前**必须**先设 token（当前 OCR 编排 `EHR_API_TOKENS` 为空 ⇒ **放行模式**，仅本地）；⑦ghcr 包可见性仍 private（OCR 镜像也尚未推 GHCR）；⑧真·第三方 MCP 客户端互通仍未验；⑨`.dcm`/DICOM worker 未做（能力清单已如实标 `enabled=false`）；⑩**上游分类缺口**（`parser::classify` 认「出院记录」不认「出院小结」）仍在；⑪MCP 侧仍**无 resources/prompts**；⑫`docs/API.md` 是**生成物**，改接口须跑 `./api-doc-check.sh --write`；⑬人员域三端点仍 501 且刻意不进 OpenAPI；⑭**OCR 镜像未推 GHCR / 未纳入 CI**（建议先按需本地构建）；⑮`container-smoke.sh` 本轮**未跑**（运行中的容器是旧镜像 ⇒ 须先重建镜像才有意义）。
- ⚠️ 未销账：目标验收①**Web 管理端仍是单文件最小版**（编译期内嵌，无构建链）；`docs/API.md` 是**生成物**；文档覆盖 **REST 面**，`/mcp` 的工具契约在 `skills/medme-ehr/references/tool-contracts.md`；**上游分类缺口**仍在；**上游审计口径缺口**已在 D-030 如实标注（**不造事件补齐**）；**OCR 镜像尚未推 GHCR、未纳入 CI**；**「手抄实现细节」残留**已治到 **stub 清单 / REST 路径 / 响应形状 / 文档字段** 四个面——**「产物新鲜度」与「MCP 工具契约字段面」仍无门**。

### 历史状态（2026-10-26 · **T22「形状门自己也在测错对象」——覆盖全部端点从注释承诺变成机器判据 + 第 5 个「文档在骗人」的缺陷已修 · D-039 落地**）

- ✅ **T22-① 接手工作树的第一件事：让它编译并跑一遍（而不是先读计划）**：本轮 `cargo test` **直接编译失败**（`error[E0608] cannot index into a value of type CapabilityManifest` / `error[E0277] can't compare Value with CapabilityManifest`）——`tests/capabilities.rs` 仍在用旧的 `Value` 索引写法，而 `capability_manifest()` 已被 D-038 改成**显式建模的 struct** ⇒ **上一轮记在 PLAN 上的「79 门全绿」是在这次改动落地前取得的，改动落地后没有重跑** ⇒ **工作树此前根本没有跑过新代码**。修法：`accepted_exts` 改读 struct 字段、HTTP 层比较改为「两者都序列化成 `Value` 再比」（仍是逐字相等判据，只是不依赖不存在的 trait）。**新纪律**：接手工作树的第一件事是「先让它编译并跑一遍」，否则会把**上一轮的绿灯**误当成本轮现状。
- ✅ **T22-② 抓到「**自称覆盖全部端点、实则只覆盖 12/19**」的又一道假门（D-037 同族病、在自己新写的门上复发）**：①`ehr-api/tests/response_shape.rs` 的文件头注释写着「**覆盖全部端点（不手抄清单）——清单从 OpenAPI 派生 ⇒ 加端点自动纳入守卫**」；②但代码里是**一份手写的 12 条 `probes` 数组** ⇒ **19 个 operation 只覆盖了 12 个**；③**7 个写侧 operation 从未被断言**（`POST /persons` / `POST .../documents` / `POST .../entries/{self-measurement,note,profile-event}` / `PUT .../extraction` / `GET .../original`）⇒ 而**门是绿的、注释是自信的**，只有**数一数**才发现漏了 7 个；④⇒ **「宣称覆盖全部」与「实际覆盖」分叉**，而判据的有效性依赖**一句不可执行的注释**。
- ✅ **T22-③ 把「覆盖率」本身变成机器判据（D-039 决策①）**：新增门 `every_openapi_operation_is_covered_by_a_probe` —— **从运行时 OpenAPI 派生的 operation 集合 ⊆ 探针覆盖集合，且两集合逐条相等** ⇒ **加了新端点却忘了加探针 ⇒ 这一门立刻红** ⇒ 「自动纳入守卫」这句话**第一次真的成立**。**为什么是「集合包含 + 逐条相等」而不是「数量 ≥ N」**（D-009 判据形状第七次应验）：数量对了但端点名错了照样是漏；且「≥ 19」在 19→20 时**不会红** ⇒ 抓不到「忘加探针」这个真正的失败模式。
- ✅ **T22-④ 顺带抓到并修掉第 5 个「文档在骗人」的缺陷：`GET .../original` 的 Content-Type 声明是假话**：①真响应的 `Content-Type` 是**动态的**（取自原件入库时嗅探/存储的 mime：上传 `.txt` ⇒ `text/plain`、`.pdf` ⇒ `application/pdf`；只有索引行缺失时才回落 `application/octet-stream`）；②OpenAPI 却**写死** `application/octet-stream` ⇒ **对绝大多数文件都不成立**；③**处置**：声明改为**通配 `*/*`**（OpenAPI 3.1 的 content map 允许通配键）⇒ 如实表达「任意二进制流，具体类型由原件 mime 决定」，客户端按 `*/*` 处理（存字节）**总是安全**；④**为什么不用 `application/octet-stream`**：那是**对大多数文件不成立的具体断言**，比通配更糟（通配至少不撒谎）；⑤**utoipa 的 `content_type` 是单值属性、没有「动态」的表达力** ⇒ 遇到「真值是动态的」时**先问「声明能不能放宽」**，而不是挑一个最常见的值写死。
- ✅ **T22-⑤ 形状门按 content-type 分流（不能拿 JSON 判据套二进制端点）**：判据分三支 —— ①声明 `application/json` ⇒ **必须有 schema**（缺陷④的门）且真响应符合；②声明 `*/*`（通配）⇒ **覆盖真值**（恒真），但**必须**确认真体**解析不出 JSON**（content-type 不能撒谎）；③声明具体非 JSON 类型 ⇒ 真响应 content-type 必须**逐字相等**。**判据形状 = 「声明必须覆盖真值」**（D-039 决策⑤：通配天然覆盖任意具体类型 ⇒ 门判的是「覆盖」这个**语义**，不是「字符串相等」这个形式）。
- ✅ **证据链（缺一即不算交付）**：
  - ①**`cargo test --all-targets` 83 门全绿**（原 79 + T22 新增 `every_openapi_operation_is_covered_by_a_probe` **1 门** + D-038 的 `response_shape.rs` **3 门**已归位；另修 `tests/capabilities.rs` **2 处编译错**）。
  - ②**本轮抓到并修掉的真缺陷 5 个**：D-038 的 ①②③④（timeline/search/documents 的**裸数组声明 vs 真回对象**、healthz/capabilities 的**没声明 body**）+ 本轮新增的 ⑤（`/original` 的 content-type 假话）。其中 ④⑤ 是**本轮新门抓到的**——D-038 那 3 门因只覆盖 12/19 个 operation，**根本碰不到 `/original`**。
  - ③**覆盖率门是「真门」的负向证据（实撞一次）**：从探针表**删掉** `PUT .../extraction` 的登记 ⇒ 该门 **FAILED**，报 `OpenAPI 里有 operation 没有探针 ⇒ 它们从未被形状门断言过：["PUT /api/v1/persons/{pid}/documents/{sha}/extraction"]` ⇒ 复原后全绿。**这正是「上一版漏掉 7 个写侧 operation」那个失败模式的精确复现**。
  - ④**content-type 门是「真门」的负向证据（实撞一次）**：把 `/original` 的声明**改回** `application/octet-stream`（恢复那句假话）⇒ 形状门 **FAILED**，报 `真响应 Content-Type='text/plain' 与 OpenAPI 声明的 'application/octet-stream' **不一致**` ⇒ 复原 `*/*` 后全绿。
  - ⑤**门禁全量复跑 13 个脚本 fails=0**：`api-doc-check.sh`（A5 `stub endpoints=0`）/ `skill-contract-check.sh` / `skill-rest-check.sh` / `mcp-e2e.sh` / `auth-e2e.sh` / `entries-e2e.sh` / `conds-e2e.sh` / `meds-e2e.sh` / `audit-e2e.sh` / `ehtest.sh` / `rebuild-e2e.sh` / `rebuild-lock-e2e.sh` / `web-e2e.sh`。
  - ⑥**`docs/API.md` 重新生成**（**40305 bytes**）并与生成物**逐字一致**（A3）；A4 `openapi operations=19 / missing_in_doc=0`；A5 `stub endpoints=0`。
  - ⑦**零新依赖**：`ehr-api/Cargo.toml` / `Cargo.lock` **未改动**（本轮只改测试 + 一处 OpenAPI 声明）。
  - ⑧**后台卡死检查（D-010）**：**无 cargo/rustc 残留**；唯一常驻是 `~/medme-shared-target/release/ehr-api --addr=127.0.0.1:18999`（10-21 的**陈旧**二进制）与两个 docker 容器（:8585 默认 / :8587 OCR）⇒ **非卡死**，但那个 :18999 进程是**陈旧产物**（按 D-035 新坑③「先问它跑的是哪一份产物」⇒ 它是 10-21 编译的，不含本轮任何改动）。
- 🔄 **下一步**：①**「手抄实现细节」残留**已推进到**门禁面**（抓到 `response_shape.rs` 自称覆盖全部实则 12/19），但**文档面**（`rest-contracts.md` / `SKILL.md` 里是否还有**过期或错的形状描述**，尤其 `GET /persons` 声明为裸数组 `Vec<Person>` 这类）**未逐条核**；②**T5 Web 管理端正式化**（目标验收①最后一块，**选型先报方案再动手**）；③「体检报告 PDF 列布局 labs=0」缺口：extraction 管线已就位，缺的是「把 PDF 送进 LLM」这一步的**端到端脚本**；④「家庭自测趋势」专用视图仍未做；⑤公网暴露前 `EHR_BIND_ADDR` 改 `0.0.0.0` 前**必须**先设 token（当前 OCR 编排 `EHR_API_TOKENS` 为空 ⇒ **放行模式**，仅本地）；⑥ghcr 包可见性仍 private（OCR 镜像也尚未推 GHCR）；⑦真·第三方 MCP 客户端互通仍未验；⑧`.dcm`/DICOM worker 未做（能力清单已如实标 `enabled=false`）；⑨**上游分类缺口**（`parser::classify` 认「出院记录」不认「出院小结」）仍在；⑩MCP 侧仍**无 resources/prompts**；⑪`docs/API.md` 是**生成物**，改接口须跑 `./api-doc-check.sh --write`；⑫人员域三端点仍 501 且刻意不进 OpenAPI；⑬**OCR 镜像未推 GHCR / 未纳入 CI**（建议先按需本地构建）。
- ⚠️ 未销账：目标验收①**Web 管理端仍是单文件最小版**（编译期内嵌，无构建链）；目标验收②的 6 个 stub 端点**已全部实装**；`docs/API.md` 是**生成物**；文档覆盖 **REST 面**，`/mcp` 的工具契约在 `skills/medme-ehr/references/tool-contracts.md`；**上游分类缺口**仍在；**上游审计口径缺口**已在 D-030 如实标注（**不造事件补齐**）；**OCR 镜像尚未推 GHCR、未纳入 CI**；**「手抄实现细节」残留未系统扫**（本轮治了门禁面，**文档面未扫**）；**`GET /persons` 仍是裸数组声明**（真回也是数组 ⇒ **不漂移**，但与其它端点的信封风格不一致，待统一评估）。

### 历史状态（2026-10-26 · **T20「stub 判据」手写正则→实现派生清单 + 查错表的假门已修 · D-037 落地**）

- ✅ **T20-① 修掉一道「一直在查错表」的假门**（本轮起点是一个真红：`audit-e2e.sh` M5 `FAIL M5 stub 计数不是 1`）：①`audit-e2e.sh` / `entries-e2e.sh` / `conds-e2e.sh` / `meds-e2e.sh` 四个脚本各自用**手写正则解析 `lib.rs` 源码**来判断「端点是否仍被宣称 stub→501」；②`extraction` 实装后 `CONTRACT` 变**空表**（`&[];`）⇒ 该正则的「`\n];`」匹配失败 ⇒ **非贪婪 `.*?` 回溯到下一张表**（`MOUNTED_UNDOCUMENTED`）⇒ 脚本读到的是**人员域三端点**而非 CONTRACT ⇒ **四道门实际在查错的表**；③它们一直绿**不是因为判据对，而是因为空表恰好不含这些端点**。
- ✅ **T20-② 判据改为「实现派生 + 逐条公布」**（D-037）：①`rest_manifest()` 新增字段 **`stub_paths`**（**逐条**列出仍被宣称 501 的 operation）——**为什么不是只给个数**：门禁要回答的是「**某一个具体端点**还在不在 stub 清单里」⇒ 判据形状必须是**集合**不是标量（D-009 判据形状原则）；②四个脚本改读 **`--print-rest-manifest` 的 `stub_paths`**，**删掉「手写正则解析源码」**（同一事实一份实现，D2）；③`audit-e2e.sh` M5 的「应剩 1 个」写死快照一并去掉，改为「`CONTRACT` 条目数 **==** 派生清单 `stubs` 数」（D-032/T15 同族修复）。
- ✅ **T20-③ 反向保险（单测层）**：新增 `rest_manifest_stub_paths_match_contract_table` —— 断言 ①`stub_paths` **逐条等于** `CONTRACT` 表；②`stubs` 数 **==** `stub_paths` 长度；③**五个已实装端点绝不出现在 `stub_paths`**（audit / extraction / medications / conditions / measurements）⇒ 「实装了但 OpenAPI 仍宣称 501」在**单测层**立刻红（D-025 纪律）。
- ✅ **T20-④ 负向证据（门是真的）**：临时把 `medications` 挂回 `CONTRACT` ⇒ `meds-e2e.sh` **FAIL M5**（fails=2）+ `rest_manifest` 测试 **FAILED**（`implemented endpoint still claimed as stub`）+ 派生清单 `stubs=1 / stub_paths=["GET /api/v1/persons/{pid}/medications"]` ⇒ **参数真进判据**；复原后全绿。
- ✅ **T20-⑤ 工作树里上一轮的 extraction 硬化已验证为绿**（本轮顺带核销）：`add_extraction` 落库的是**校验后**的 extraction（编造字段进不了真相层）、`mode` 不猜（拼错 ⇒ 400）、**无文本层拒收**（不「空对空恒过」）、错误**类型化**（`ExtractionError` enum ⇒ handler 一处映射状态码，不再按错误字符串分类）；`skill-rest-check.sh` 新增 4 门（`extraction_verified_true` / `extraction_content_hash_echoes_sha` / `extraction_forged_field_rejected` / `extraction_bad_mode_400`）。
- ✅ **证据链（缺一即不算交付）**：
  - ①**`cargo test --all-targets` 79 门全绿**（原 78 + T20 新增 `rest_manifest_stub_paths_match_contract_table` **1 门**）。
  - ②**门禁全量复跑 13 个脚本 fails=0**：`api-doc-check.sh`（A5 `stub endpoints=0`）/ `skill-contract-check.sh` / `skill-rest-check.sh` / `mcp-e2e.sh` / `auth-e2e.sh` / `entries-e2e.sh` / `conds-e2e.sh` / `meds-e2e.sh` / `audit-e2e.sh` / `ehtest.sh` / `rebuild-e2e.sh` / `rebuild-lock-e2e.sh` / `web-e2e.sh`。
  - ③**派生清单本身可验**：`--print-rest-manifest` ⇒ `stubs=0` / `stub_paths=[]`（**空表也对**，不是「空则跳过」）。
  - ④**负向证据**：见 T20-④（真红两次 + 复原绿）。
  - ⑤**零新依赖**：`ehr-api/Cargo.toml` / `Cargo.lock` **未改动**。
- 🔄 **下一步**：①**「手抄实现细节」残留需系统扫**（本轮只治了「stub 判据」这一类；`container-smoke.sh` / `tests/mcp.rs` 等处的数字快照未逐个核查）；②**T5 Web 管理端正式化**（目标验收①最后一块，**选型先报方案再动手**）；③**「体检报告 PDF 列布局 labs=0」缺口**：extraction 管线已就位，缺的是「把 PDF 送进 LLM」这一步的端到端脚本；④「家庭自测趋势」专用视图仍未做；⑤公网暴露前 `EHR_BIND_ADDR` 改 `0.0.0.0` 前**必须**先设 token（当前 OCR 编排 `EHR_API_TOKENS` 为空 ⇒ **放行模式**，仅本地）；⑥ghcr 包可见性仍 private（OCR 镜像也尚未推 GHCR）；⑦真·第三方 MCP 客户端互通仍未验；⑧`.dcm`/DICOM worker 未做（能力清单已如实标 `enabled=false`）；⑨**上游分类缺口**（`parser::classify` 认「出院记录」不认「出院小结」）仍在；⑩MCP 侧仍**无 resources/prompts** 能力；⑪`docs/API.md` 是**生成物**，改接口须跑 `./api-doc-check.sh --write`；⑫人员域三端点仍 501 且刻意不进 OpenAPI；⑬**OCR 镜像未推 GHCR / 未纳入 CI**（构建需 glibc≥2.38 + 代理 + ModelScope，**建议先按需本地构建**）。
- ⚠️ 未销账：目标验收①**Web 管理端仍是单文件最小版**（编译期内嵌，无构建链）；目标验收②的 6 个 stub 端点**已全部实装**（最后一个 `extraction` 本轮核销）；`docs/API.md` 是**生成物**；文档覆盖 **REST 面**，`/mcp` 的工具契约在 `skills/medme-ehr/references/tool-contracts.md`；**上游分类缺口**仍在；**上游审计口径缺口**已在 D-030 如实标注（**不造事件补齐**）；**OCR 镜像尚未推 GHCR、未纳入 CI**；**「手抄实现细节」残留未系统扫**。

### 历史状态（2026-10-26 · **T19「OCR 做成可部署镜像」+「真照片识别闭环实拍」· D-036 落地**）

- ✅ **T19-① OCR 变体做成「独立可部署镜像」**（D-035 未销账②销账）**：①新增 `deploy/Dockerfile.ocr`（**独立** Dockerfile 而非给默认 Dockerfile 加 feature 分支——理由写进文件头：ORT 1.28 要求 **glibc ≥ 2.38** ⇒ 基础镜像必须换 `rust:1-slim-trixie` / `debian:trixie-slim`；且**默认镜像必须保持 bookworm + 零 OCR 依赖**，否则宿主 e2e 门禁整体报废）；②新增 `deploy/docker-compose.ocr.yml`（**项目名 `medme-ehr-ocr` 隔离**、端口 8587、独立 image tag `medme-ehr/medme-ehr-ocr:latest`、独立 volume `ehr-home-ocr` ⇒ 与默认编排互不覆盖）；③**PP-OCRv5 三个模型在构建期从 ModelScope 取并烤进镜像**（`OAR_HOME=/opt/oar-models`），**sha256 硬编码核对**（三个文件各一个 sidecar，内容 = 上游 registry 的 sha256）⇒ 篡改/半截文件被 `[FAIL]` 打回，且**首启不联网、不写用户目录**；④**构建期自带 dry-run 门**：`--print-capabilities` 必须出 `ocr_available=true`，否则构建失败（基础镜像选错时立刻暴露，不等到部署后）；⑤**构建期/运行期通道**按 D-004~D-007 走（crates 走 TUNA sparse+static、ORT 静态库与模型走代理、apt 走 TUNA 镜像）。
- ✅ **T19-②「真识别闭环」实拍证据（D-035 未销账①销账 —— 最后一格补上）**：在**运行中的 OCR 容器**（`medme-ehr-ocr-ehr-api-1`，127.0.0.1:8587）上传**真实检查单照片** `docs/testres/检查单/zmf/20250922_血常规.jpg` ⇒ 回执 **`text_indexed=true`**、**`stored_raw_only=false`**、`messages=["ocr=ppocr-v5(onnx) via pipeline::ingest"]`；文档详情 `text` 层 **1781 字**、`ocr_meta={pages_with_text:[1], model_version:"ppocr-v5", backend:"onnx", min_confidence:0.9834076}`；`GET /labs` ⇒ **series_count=23 / total_points=23**（如 `neut_pct=53.97%`、`neut_count=2.61 10^9/L`、`mono_pct=7.43%`），**每点带 `docref` 指回原件 sha256** ⇒ 「拍照 → 识别 → 文本层入库 → 指标聚合 → 溯源」**全链路闭合**。
- ✅ **T19-③ 默认构建面零污染**（D-034 决策③的前提未被自己破坏）：OCR 走**独立构建目录 + 独立镜像**，默认 `cargo tree` 仍**不含 ort/oar/onnx**、宿主 `ldd` 仍无 onnx/ort ⇒ 宿主 e2e 门禁体系**未受影响**（本轮全量复跑即证）。
- ✅ **T19-④ 提交物卫生**（D5 纪律）：`deploy/ort/`（101M ORT 静态库）、`deploy/oar-models/`（21M 权重）、`ehr-api/target-ocr/`（1.4G 构建目录）**全部 gitignore**（构建期可重取 + sha256 已硬编码核对 ⇒ 可复现，无需把二进制/权重提交进 git）；**`docs/testres/` 仍不入库**（含真实体检单与 LLM key）。
- ✅ **证据链（缺一即不算交付）**：
  - ①**镜像内 dry-run**：`docker run --rm medme-ehr/medme-ehr-ocr:latest --print-capabilities` ⇒ stderr `ocr_available=true | accepted=7 | rejected=1`；stdout `ocr_engine="ppocr-v5(onnx) via pipeline::ingest"`、`accepted_exts` = txt/pdf/**png/jpg/jpeg/tiff/tif**、`rejected_exts` = **仅 dcm**。
  - ②**live 容器与 dry-run 逐字一致**：`GET 127.0.0.1:8587/api/v1/capabilities` ⇒ `ocr_available=True` / accepted 7 项 / rejected=['dcm'] ⇒ 容器跑的就是那份产物。
  - ③**真照片实拍**（见 T19-②）⇒ **`text_indexed=true` / `stored_raw_only=false`**，非 `stored_raw_only` 的「假成功」。
  - ④**`cargo test --all-targets` 73 门全绿**（含 T18 新增的 `mcp_capabilities_match_rest_capabilities_endpoint`）。
  - ⑤**门禁全量复跑 fails=0**：`mcp-e2e.sh` / `skill-contract-check.sh` / `skill-rest-check.sh` / `api-doc-check.sh` / `ehtest.sh` / `auth-e2e.sh` / `entries-e2e.sh` / `conds-e2e.sh` / `meds-e2e.sh` / `audit-e2e.sh` / `rebuild-e2e.sh` / `rebuild-lock-e2e.sh` / `web-e2e.sh` —— **13 个脚本全绿**。
  - ⑥**门禁是「真门」的负向证据（本轮两次实撞）**：`ehtest.sh`/`rebuild-e2e.sh` 的 **G0 门「端口必须无监听」**在本轮**先手工起了服务再跑脚本**时红了（`GATE FAIL G0 port 8979 free before run`）⇒ 证明 G0 真会拦「陈旧实例串台」；**正解 = 让脚本自己起服务**（它有 `SRV_PID` 与 `trap cleanup`），不手工预占端口。
  - ⑦**MCP/REST 同源门**（T18 机制照抄生效）：`mcp-e2e.sh` 的 `mcp_capabilities == rest_capabilities (verbatim)` 门**在默认构建（无 OCR）下**照样绿 ⇒ 判据是「两面同源」，不是「清单里有 OCR」。
- 🔄 **下一步**：①**stub 端点仍剩 1 个**：`PUT /documents/{sha}/extraction`（**新功能** ⇒ 需单独设计字段级 evidence 锚点；上游 `deid::verify` 要求 evidence 是**原文逐字子串**）——**且它是「体检报告 PDF 列布局 labs=0」缺口的自然落点**（PLAN 真实场景报告缺口①倾向走 LLM 结构化抽取 ⇒ 两条缺口可合并设计）；②**T5 Web 管理端正式化**（目标验收①最后一块，**选型先报方案再动手**）；③「家庭自测趋势」专用视图仍未做；④公网暴露前 `EHR_BIND_ADDR` 改 `0.0.0.0` 前**必须**先设 token（当前 OCR 编排 `EHR_API_TOKENS` 为空 ⇒ **放行模式**，仅本地）；⑤ghcr 包可见性仍 private（OCR 镜像也尚未推 GHCR）；⑥真·第三方 MCP 客户端互通仍未验；⑦`.dcm`/DICOM worker 未做（能力清单已如实标 `enabled=false`）；⑧**上游分类缺口**（`parser::classify` 认「出院记录」不认「出院小结」）仍在；⑨MCP 侧仍**无 resources/prompts** 能力；⑩**`docs/API.md` 是生成物**，改接口须跑 `./api-doc-check.sh --write`；⑪人员域三端点仍 501 且刻意不进 OpenAPI；⑫**OCR 镜像未推 GHCR / 未纳入 CI**（构建需 glibc≥2.38 + 代理 + ModelScope，CI 成本高，**建议先按需本地构建**）。
- ⚠️ 未销账：目标验收①**Web 管理端仍是单文件最小版**（编译期内嵌，无构建链）；目标验收②的 6 个 stub 端点已实装 5 个，**剩 `extraction` 一个**；`docs/API.md` 是**生成物**；文档覆盖 **REST 面**，`/mcp` 的工具契约在 `skills/medme-ehr/references/tool-contracts.md`；**上游分类缺口**仍在；**上游审计口径缺口**已在 D-030 如实标注（**不造事件补齐**）；**OCR 镜像尚未推 GHCR、未纳入 CI**。

### 历史状态（2026-10-26 · **T18「MCP 侧补 capabilities 工具」+「OCR 容器产物首次跑通」· D-035 落地**）

- ✅ **T18-① MCP 侧补 `get_capabilities` 工具（D-034 未销账⑫销账）**：①**复用同一份** `capabilities::capability_manifest()`（**不另写判据**）⇒ MCP 与 REST 两面清单**逐字相同**；②工具**读侧**、无参；MCP 工具数 **12 → 13**（读侧 10 → **11**），由 `--print-mcp-tools` 派生、门禁核对；③skill 层同步：`SKILL.md` 命令表加 `capabilities ↔ get_capabilities` 一行、工具数改 13、`references/tool-contracts.md` 加契约行 + 标题数字改「共 13 / 读侧 11」。
- ✅ **T18-② OCR 容器产物首次跑通（D-034 未销账②推进）**：①**构建路线** = 「`debian:trixie`(glibc 2.41) 容器内构建 + `CARGO_TARGET_DIR=target-ocr/` 独立目录 + `--offline` 复用宿主 `~/.cache/ort.pyke.io` 预编译 ORT 缓存」⇒ **默认构建目录零污染**（默认构建仍「无 ort/oar/onnx」，D-034 的门禁前提未被自己破坏）；②**产物存在**：`ehr-api/target-ocr/release/ehr-api` = **55,883,352 字节**（含静态链入的 ONNX Runtime）；③**能力清单如实变化**（容器内 `--print-capabilities`）：`ocr_available=true`、`ocr_engine="ppocr-v5(onnx) via pipeline::ingest"`、`accepted_exts` = txt/pdf/**png/jpg/jpeg/tiff/tif**、`rejected_exts` = **仅 dcm** ⇒ 「链接进引擎」与「清单随之变化」两件事都被证；④**宿主跑不了是预期行为**（`ldd` ⇒ `version 'GLIBC_2.38' not found`）⇒ D-034「**链接成功 ≠ 二进制能在宿主跑**」再次实测成立 ⇒ 该产物**只能**在 glibc ≥ 2.38 容器内运行。
- ✅ **证据链（缺一即不算交付）**：
  - ①**`cargo test --all-targets` 73 门全绿**（原 72 + 新 `tests/mcp.rs::mcp_capabilities_match_rest_capabilities_endpoint` **1 门**）：MCP 清单 == REST 清单（**逐字**）；`ocr_available` 是 bool；每条**未启用**路由都带 `reject_reason`；每个 `rejected_exts` 元素都归属某条带原因的未启用路由。
  - ②**`skill-contract-check.sh` fails=0**：`doc_tool_count`=13 / `doc_read_count`=11 / `doc_write_count`=2 / `skill_tool_count`=13 ⇒ T15/T16 的「**文档数字由实现派生清单核对**」机制在新增工具时**自动生效**（文档不更新就红）。
  - ③**`mcp-e2e.sh` fails=0**（**新增 3 门**：`mcp_capabilities == rest_capabilities (verbatim)` / `ocr_available_is_bool` / `exts_are_arrays`）。
  - ④**旧门禁全绿未被破坏**：`ehtest.sh` / `skill-rest-check.sh` / `api-doc-check.sh` / `auth-e2e.sh` / `entries-e2e.sh` / `conds-e2e.sh` / `meds-e2e.sh` / `audit-e2e.sh` / `rebuild-e2e.sh` / `rebuild-lock-e2e.sh` / `web-e2e.sh` **全 fails=0**。
  - ⑤**新参数/新工具真进判据**：`--print-mcp-tools` 干跑 ⇒ `count=13 / read=11 / write=2`，`get_capabilities` 在 `names[]` 里；live `tools/call get_capabilities` 的 `structuredContent` 与 `GET /api/v1/capabilities` **逐字相等**（e2e 门证）。
  - ⑥**零新依赖**（默认构建面）：本轮**未改** `Cargo.toml`/`Cargo.lock`；`Cargo.lock` 含 `ort` 等 optional 依赖是 D-034 已登记的事实（**锁文件含全部 feature 的解析结果**，非本轮新增）。
- 🔄 **下一步**：①**OCR 产物的「真识别闭环」还差最后一格**——容器内上传真检查单 jpg ⇒ 回执 `text_indexed=true`（非 `stored_raw_only`）的实拍证据（模型权重需 `$OAR_HOME`，运行期从 ModelScope 下载，容器内尚未验证）；②**OCR 产物尚未做成可部署镜像**（`deploy/Dockerfile` build 段仍是默认 feature ⇒ 需加 `--features ocr` 变体 + 基础镜像换 `debian:trixie`）；③**stub 端点仍剩 1 个**：`PUT /documents/{sha}/extraction`（**新功能** ⇒ 需单独设计字段级 evidence 锚点）；④**T5 Web 管理端正式化**（目标验收①最后一块，**选型先报方案再动手**）；⑤「家庭自测趋势」专用视图仍未做；⑥公网暴露前 `EHR_BIND_ADDR` 改 `0.0.0.0` 前**必须**先设 token；⑦ghcr 包可见性仍 private；⑧真·第三方 MCP 客户端互通仍未验；⑨`.dcm`/DICOM worker 未做（能力清单已如实标 `enabled=false`）；⑩**上游分类缺口**（`parser::classify` 认「出院记录」不认「出院小结」）仍在；⑪MCP 侧仍**无 resources/prompts** 能力；⑫**`docs/API.md` 是生成物**，改接口须跑 `./api-doc-check.sh --write`；⑬人员域三端点仍 501 且刻意不进 OpenAPI；⑭**体检报告 PDF 列布局 labs 抽取为 0**（真实场景报告缺口①）仍未做——倾向走 LLM 结构化抽取（extraction 管线），与 stub 端点 `extraction` 可合并设计。
- ⚠️ 未销账：目标验收①**Web 管理端仍是单文件最小版**（编译期内嵌，无构建链）；目标验收②的 6 个 stub 端点已实装 5 个，**剩 `extraction` 一个**；**OCR 已产出二进制但未做成镜像、真识别闭环未实拍**；`docs/API.md` 是**生成物**；文档覆盖 **REST 面**，`/mcp` 的工具契约在 `skills/medme-ehr/references/tool-contracts.md`；**上游分类缺口**仍在；**上游审计口径缺口**已在 D-030 如实标注（**不造事件补齐**）。

### 历史状态（2026-10-26 · **T17「ingest 能力面」三处硬编码→一处派生（`--print-capabilities`）· D-034 落地：OCR 走可选 feature，默认构建如实拒图片**）

- ✅ **T17 交付（消除「本构建能吃哪些文件类型」的三份实现 · D-034 落地）**：①**真源 = 实现**（`ehr_api::capabilities::route_for_ext()` —— 扩展名→路由的判据只此一处），**能力清单从它派生**（`capability_manifest()`，不是手抄表）；新增干跑开关 **`ehr-api --print-capabilities`**（stdout = 机器可读 JSON：`ocr_available`/`ocr_engine`/`routes[]`/`accepted_exts[]`/`rejected_exts[]`；stderr = `[证据]` 行，dry-run 不落状态）⇒ **门禁与文档从实现取值**；②**同一事实此前三份实现**：`vaults::ingest_upload` 的 if/ensure 判据、OpenAPI 里 501 的 description 文案、门禁/文档里手写的「ocr worker 未做」⇒ 现在**改一处**（实现），其余**从它取值**；③**新增 HTTP 端点 `GET /api/v1/capabilities`**（D-034 决策④）⇒ agent/客户端**自己**能判断「我的文件能不能传」，不必猜、不必看日志；④**skill 层同步暴露**：`ehr.sh capabilities` 命令 + `SKILL.md` 命令表 1 行 + `references/rest-contracts.md` 契约行 + 501 语义改写为「先查 `rejected_exts` 的 `reject_reason`」；⑤**OCR 改为可选 feature `ocr`（默认 off）**——见下「OCR 决策」。
- ✅ **OCR 决策（实测硬证据，不是推测）**：上游 `oar-ocr` 链的 **ONNX Runtime 1.28 预编译静态库要求 glibc ≥ 2.38**（C++23 ABI 符号 `__isoc23_strtol` / `std::__cxx11::basic_string<...>::_M_replace_cold`）。宿主 Ubuntu 22.04 = **glibc 2.35 / libstdc++ 11** ⇒ 链接期报 undefined symbol。**先前把它误诊为「缺 libstdc++」并加 `-C link-arg=-lstdc++`（D-034 修订）——那是错的**：宿主 `nm -D libstdc++.so.6 | grep -c _M_replace_cold` = **0**，加 `-lstdc++` 只能链到同一个缺符号的库，治不了。⇒ **OCR 走 feature `ocr`（默认 off）**：默认构建 = 零 OCR 依赖（`ldd` 无 onnx/ort）、宿主可跑、**既有门禁体系不受影响**；需要 OCR 的产物在 **glibc ≥ 2.38 的容器**（实测 debian:trixie = glibc 2.41 / g++ 14.2）里 `--features ocr` 构建并在该容器内运行。
- ✅ **诚实边界（D5，重要）**：①`ocr_available=true` 只说明**链了引擎**，**不保证模型已就绪**（PP-OCRv5 权重是运行期下载到 `$OAR_HOME` 的）⇒ 真失败时回执带 `stored_raw_only=true` + messages，**不假装识别成功**；②默认构建拒图片时**错误信息指明怎么开**（含 `--features ocr` 与 glibc 要求）⇒「接不了」不等于「坏了」；③**DICOM 路由已知但本服务未接线** ⇒ 仍报「needs dicom worker」，**不假装支持**。
- ✅ **证据链（缺一即不算交付）**：
  - ①**`cargo test --all-targets` 72 门全绿**（原 66 + 新 `capabilities::tests` **3 门** + 新 `tests/capabilities.rs` **3 门**）：①**清单与实际行为逐条一致**（不是「清单自洽」）：`ingest_upload` 真传 png/jpg/jpeg/tiff/tif/dcm/exe 七个扩展名，**清单说能吃的真被接受、说接不了的真被拒**；②**默认构建如实拒图片**（无 ocr feature 时 jpg ⇒ 拒，且错误含 `no OCR engine` 与 `--features ocr`）；③**HTTP 端点与库级清单逐字相等**；④**未知扩展名不进能力清单**（不宣称支持）。
  - ②**`skill-rest-check.sh` fails=0**（**新增 6 门** `capabilities_*`）：helper 的 `capabilities` 命令真能调；`ocr_available` 是 bool；`accepted_exts`/`rejected_exts` 都是数组；**txt 真在 accepted 里且真能上传**（`content_hash` 有值）；**jpg 真被拒且错误信息含 `no OCR engine` + `--features ocr`**（⇒ 清单不是自说自话）；命令面对齐门由 16 → **17 个**。
  - ③**`skill-contract-check.sh` fails=0**（32 门）：`doc_rest_paths`=17 / `doc_rest_operations`=19 / `skill_rest_paths`=17 ⇒ **文档数字由实现派生清单核对**（T16 机制照抄生效）。
  - ④**`ehtest.sh` fails=0**（含 A0/F2 派生清单门）、**`mcp-e2e.sh` fails=0**（36 门）、`auth-e2e.sh` / `entries-e2e.sh` / `conds-e2e.sh` / `meds-e2e.sh` / `audit-e2e.sh` / `rebuild-e2e.sh` / `rebuild-lock-e2e.sh` / `web-e2e.sh` **全 fails=0**。
  - ⑤**`api-doc-check.sh` fails=0**（8 门）：`docs/API.md` 重新生成（**35632 → 36398 bytes**）与生成物**逐字一致**；A4 覆盖 **operations=19**（新端点进了文档）、A5 stub 端点数仍 = 1。
  - ⑥**门禁是「真门」的负向证据**：把 `rest-contracts.md` 的「17 路径」**故意改成「16 路径」** ⇒ 首跑 **`FAIL doc_rest_paths: got=16 want=17` / fails=1** ⇒ 复原后 **fails=0**。
  - ⑦**新参数真进判据**：`--print-capabilities` ⇒ stdout 出 JSON + stderr `mode=dry-run(print-capabilities)` + `accepted=2 rejected=6`；**不传它** ⇒ 走 serve 模式；live `GET /api/v1/capabilities` 的 JSON 与 `--print-capabilities` **逐字一致**。
  - ⑧**依赖增量如实标注（不谎报「零新依赖」）**：**默认构建的依赖图未变**（`cargo tree` 默认 = 210 包，**不含 ort/oar/onnx**；`ldd` 无 onnx/ort）——但 `Cargo.lock` **新增了 optional 依赖**（`ort` 等，锁文件含全部 feature 的解析结果）⇒ `Cargo.lock` **确有改动**，这与 T15/T16 的「零新依赖」**不同**，如实登记。
- 🔄 **下一步**：①**stub 端点仍剩 1 个**：`PUT /documents/{sha}/extraction`（**新功能** ⇒ 需单独设计字段级 evidence 锚点）；②**OCR 容器产物尚未产出**（`--features ocr` 的镜像在 glibc ≥ 2.38 基础镜像里构建；此前尝试因 **docker build 内 `host.docker.internal` 代理不可解析**而失败 ⇒ 改走「直连 + TUNA 通道」重试）；③**T5 Web 管理端正式化**（目标验收①最后一块，**选型先报方案再动手**）；④「家庭自测趋势」专用视图仍未做；⑤公网暴露前 `EHR_BIND_ADDR` 改 `0.0.0.0` 前**必须**先设 token；⑥ghcr 包可见性仍 private；⑦真·第三方 MCP 客户端互通仍未验；⑧`.dcm`/DICOM worker 未做（能力清单已如实标 `enabled=false`）；⑨**上游分类缺口**（`parser::classify` 认「出院记录」不认「出院小结」）仍在；⑩MCP 侧仍**无 resources/prompts** 能力；⑪**`docs/API.md` 是生成物**，改接口须跑 `./api-doc-check.sh --write`（手改会被 A3 打回）；⑫人员域三端点（GET/PATCH/DELETE `/persons/{pid}`）仍 501 且刻意不进 OpenAPI——何时实装需单独设计；⑬**体检报告 PDF 列布局 labs 抽取为 0**（真实场景报告缺口①）仍未做——倾向走 LLM 结构化抽取（extraction 管线），与 stub 端点 `extraction` 可合并设计。
- ⚠️ 未销账：目标验收①**Web 管理端仍是单文件最小版**（`ehr-api/web/index.html` 编译期内嵌，无构建链）；目标验收②的 6 个 stub 端点已实装 5 个，**剩 `extraction` 一个**；**OCR 尚无产物**（能力清单会如实说「本构建无 OCR 引擎」，不假装）；`docs/API.md` 是**生成物**；文档覆盖的是 **REST 面**，`/mcp` 的工具契约仍在 `skills/medme-ehr/references/tool-contracts.md`；**上游分类缺口**仍在；**上游审计口径缺口**（`audit_log()` 不展示手动条目的合成导入）已在 D-030 如实标注（**不造事件补齐**）。

### 历史状态（2026-10-26 · **T16「REST 路径面」五处硬编码→一处派生（`--print-rest-manifest`）· D-032 未销账⑩已销账**）

- ✅ **T16 交付（消除「REST 有多少条路径」的五处硬编码漂移 · D-032 未销账⑩销账）**：①**真源 = 实现**（`ehr_api::rest_manifest()`，由运行时 OpenAPI 派生），新增干跑开关 **`ehr-api --print-rest-manifest`**（stdout = 机器可读 JSON：paths/operations/stubs/stub_mark/paths_list/mounted_undocumented；stderr = `[证据]` 行，dry-run 不落状态）⇒ 门禁与文档**从实现取值**；②**「16 路径 / 18 operation」此前硬编码在 5 处**（`ehtest.sh` / `mcp-e2e.sh` / `container-smoke.sh` / `tests/mcp.rs` / 两份 skill 文档）——**同一事实五份实现**，加一条路由要同步改五处，漏一处就红且**看不出是「漏改」还是「真坏了」**；现在**改一处**（实现），文档数字**由门禁核对**（照抄 D-032 决策③：核对 > 消除）；③**stub 判定也只留一处**（`ehr_api::STUB_MARK` 取代 `apidoc.rs` 里手抄的「stub→501」字面量），并把标记字符串本身也**公布**在清单里 ⇒ 连门禁脚本都不用再手抄它；④**已挂路由但刻意不进 OpenAPI**的人员域三端点（GET/PATCH/DELETE `/persons/{pid}`）也抽成共享表 `MOUNTED_UNDOCUMENTED`（原在 `tests/contract.rs` 手抄一份）⇒ 同一事实两份实现消除；⑤**`tests/contract.rs` 的守卫从「手抄 8 条端点」改为「从运行时 OpenAPI 派生全部 operation」**——原先那份手抄表**漏掉的端点完全不被守卫**（守卫看起来是绿的，实则漏了新端点）。
- ✅ **证据链（缺一即不算交付）**：
  - ①**`cargo test --all-targets` 66 门全绿**（原 62 + 新 `tests/rest_manifest.rs` **4 门**）：清单的 paths/operations **必须等于**运行时 OpenAPI 实际数（不是另一个手抄快照）；`paths_list` **逐条等于** OpenAPI 的 `METHOD path` 集合（名字与集合都对，不只数量）；`stubs` **== CONTRACT 表长度**（stub 端点数只有一个来源）；`mounted_undocumented` **== 共享表**。
  - ②**`ehtest.sh` fails=0**（含**新门 A0**：`--print-rest-manifest` exit=0、**新门 F2**：live openapi 路径**名字序列**逐条 == 派生清单——不只数量，「加了路由但名字写错」也能抓到；A2/F1 由「≥16 快照」改为「== 派生值」）。
  - ③**`mcp-e2e.sh` fails=0**（36 门）：`openapi_paths == derived manifest (16, not hardcoded)` + **新门 6b** `openapi_stubs == derived manifest (1)`（stub 计数也只有一个来源；标记字符串从清单取，脚本不再手抄「stub→501」）。
  - ④**`skill-contract-check.sh` fails=0**（32 门）：**新增 3 门** `doc_rest_paths` / `doc_rest_operations` / `skill_rest_paths`——两份 skill 文档里写的路径数由**实现派生清单**核对。
  - ⑤**门禁是「真门」的负向证据**：把 `SKILL.md` 的「16 路径」**故意改成「17 路径」** ⇒ 首跑 **`FAIL skill_rest_paths: got=17 want=16` / fails=1** ⇒ 复原后 **fails=0**（⇒ 门禁真会因漂移而红，不是「怎么都绿」）。
  - ⑥**旧门禁全绿未被破坏**：`ehtest.sh`(34+) / `mcp-e2e.sh`(36) / `auth-e2e.sh` / `audit-e2e.sh` / `entries-e2e.sh` / `conds-e2e.sh` / `meds-e2e.sh` / `rebuild-e2e.sh` / `rebuild-lock-e2e.sh` / `skill-rest-check.sh` / `skill-contract-check.sh`(32) / `api-doc-check.sh`(8) / `web-e2e.sh`（起真服务复验，fails=0）**全 fails=0**。
  - ⑦**零新依赖**：`ehr-api/Cargo.lock` 与 `Cargo.toml` **未改动**（依赖包数 **204** 不变，可验事实）。
  - ⑧**dry-run 优先**：先跑库级 `cargo test --all-targets`（不起服务、不落状态）与 `--print-rest-manifest` 干跑渲染（`paths=16 operations=18 stubs=1`），再起真进程跑 e2e 与旧门禁复验。
  - ⑨**参数/状态生效有证据**：`--print-rest-manifest` 是**新参数**⇒ ①传它 ⇒ stdout 出现 JSON 且 `[证据]` 行 `mode=dry-run(print-rest-manifest)`；②**不传它** ⇒ 走 serve 模式（参数真进判据，不是摆设）；③清单的 `paths_list` 与 live `/openapi.json` 的**名字序列逐字相等**（F2 门）；④**故意让文档漂移 ⇒ 门禁红**（见⑤）。
- 🔄 **下一步**：①**stub 端点仍剩 1 个**：`PUT /documents/{sha}/extraction`（**新功能** ⇒ 需单独设计字段级 evidence 锚点）；②**T5 Web 管理端正式化**（目标验收①最后一块，**选型先报方案再动手**）；③「家庭自测趋势」专用视图仍未做；④公网暴露前 `EHR_BIND_ADDR` 改 `0.0.0.0` 前**必须**先设 token；⑤ghcr 包可见性仍 private；⑥真·第三方 MCP 客户端互通仍未验；⑦`.dcm`/OCR worker 未做；⑧**上游分类缺口**（`parser::classify` 认「出院记录」不认「出院小结」）仍在；⑨MCP 侧仍**无 resources/prompts** 能力；⑩**REST 面的路径数/operation 数已销账**（本轮）；⑪**`docs/API.md` 里的路径清单仍是生成时快照**（`api-doc-check.sh --write` 重新生成才更新，**手改会被 A3 打回**）；⑫**`ehr-api` 的人员域三端点（GET/PATCH/DELETE `/persons/{pid}`）仍是 501 且刻意不进 OpenAPI**——何时实装需单独设计。
- ⚠️ 未销账：目标验收①**Web 管理端仍是单文件最小版**（`ehr-api/web/index.html` 编译期内嵌，无构建链）；目标验收②的 6 个 stub 端点已实装 5 个，**剩 `extraction` 一个**；`docs/API.md` 是**生成物**，改接口必须跑 `./api-doc-check.sh --write`，手改会被 A3 门禁打回；文档覆盖的是 **REST 面**，`/mcp` 的工具契约仍在 `skills/medme-ehr/references/tool-contracts.md`（该文档的工具数**已由门禁与实现核对**，见 T15）；**上游分类缺口**仍在（D-027 未销账⑧）；**上游审计口径缺口**（`audit_log()` 不展示手动条目的合成导入）已在 D-030 如实标注（**不造事件补齐**）。


### 历史状态（2026-10-26 · **T15「MCP 工具数」三处硬编码→一处派生（`--print-mcp-tools`）· D-031 新坑②已销账**）

- ✅ **T15 交付（消除「MCP 有多少个工具」的三处硬编码漂移 · D-031 未销账⑨已销账）**：①**工具清单的单一事实来源 = 实现**（`mcp::tool_schemas()`），派生两个出口：`mcp::tool_names()`（名字清单）+ `mcp::tool_manifest()`（机器可读 JSON：`count/read/write/names/read_names/write_names`）；②**新增干跑开关 `ehr-api --print-mcp-tools`**（stdout = 机器可读 JSON，stderr = `[证据]` 行，dry-run 不落状态）⇒ 门禁与测试**从实现取值**；③**写侧工具名单也只留一处**（`mcp::WRITE_TOOLS`），**鉴权分权改读它**（`Scope::Ro::allows_tool` 原先在 `auth.rs` 里另写了一份 `matches!` —— 同一事实两份实现，正是本轮要消灭的东西；且**上次加 `get_audit` 时分权表没同步，纯粹因为它恰好是读侧工具**——鉴权是安全面，不能靠「恰好」）；④**文档数字保留但由门禁核对**（`tool-contracts.md` 标题「共 12 / 读侧 10 / 写侧 2」与 `SKILL.md`「12 个工具」是人读上下文，**不删除**，改由 4 门核对：文档写「12」而实现是 13 ⇒ 门禁红）；⑤**「加一个工具」的改动面从四处降为两处**（实现 + 契约文档），漏改立刻红在「文档与实现不一致」上，而不是红在某个无关断言上。
- ✅ **证据链（缺一即不算交付）**：
  - ①**`mcp-e2e.sh` 真进程 36 门 PASS / fails=0**（原 34 + 新 2）：`tools_count` 的**期望值不再写死**（来自 `--print-mcp-tools`）；新增 **`tools_names_match_impl`**（`tools/list` 公布的**名字序列逐字等于**实现派生清单 ⇒ 不只数量，顺序与名字也对）；[证据]行显式 `tools_count=12 (derived from --print-mcp-tools, not hardcoded)`。
  - ②**`skill-contract-check.sh` 真进程 29 门 PASS / fails=0**（原 27 + 新 4）：`tools_declared_in_skill` 的**工具名清单也改为从实现派生**（原为手写 12 名——新增工具若忘了加进那个 for 循环，门禁**根本发现不了**新工具没被 SKILL.md 提到）；新增 `doc_tool_count` / `skill_tool_count` / `doc_read_count` / `doc_write_count`。
  - ③**门禁是「真门」的负向证据**：把 `tool-contracts.md` 标题的「共 12」**故意改成「共 13」** ⇒ 首跑 **`FAIL doc_tool_count: got=13 want=12` / fails=1** ⇒ 复原后 **fails=0**（门禁真会因漂移而红，不是「怎么都绿」）。
  - ④**`cargo test --all-targets` 全绿**：新增 `tests/mcp.rs::write_tool_list_and_scope_split_agree` **1 门**（写侧名单 ↔ scope 分权**完全**一致——不是抽样：ro 拒的恰好是写侧全集、放的恰好是读侧全集；且 `WRITE_TOOLS` 每个成员都真被公布，防「名单写了工具却没实现」；manifest 的 count/read/write 自洽）+ **删掉** `assert_eq!(tools.len(), 12)`（硬编码快照）改为断言「公布清单逐字等于 `tool_names()`」+ 旧门禁**未破** ⇒ 合计 **62 门全绿**（lib 18 / apidoc 3 / audit 3 / auth 7 / conds 4 / contract 1 / entries 6 / **mcp 14** / meds 2 / rebuild 3 / sha_gate 1）。
  - ⑤**旧 e2e 门禁全绿未被破坏**：`ehtest.sh`(34) / `mcp-e2e.sh`(36) / `auth-e2e.sh` / `audit-e2e.sh` / `entries-e2e.sh` / `conds-e2e.sh` / `meds-e2e.sh` / `rebuild-e2e.sh` / `rebuild-lock-e2e.sh` / `skill-rest-check.sh` / `skill-contract-check.sh`(29) / `api-doc-check.sh`(8) / `web-e2e.sh`（起真服务复验，fails=0）**全 fails=0**。
  - ⑥**零新依赖**：`ehr-api/Cargo.lock` 与 `Cargo.toml` **未改动**（依赖包数 **204** 不变，可验事实）。
  - ⑦**dry-run 优先**：先跑库级 `cargo test --all-targets`（不起服务、不落状态）与 `--print-mcp-tools` 干跑渲染（`tools=12 (read=10 write=2)`），再起真进程跑 mcp-e2e 与旧门禁复验。
  - ⑧**参数/状态生效有证据**：`--print-mcp-tools` 是**新参数**⇒ ①传它 ⇒ stdout 出现 JSON 且 `[证据]` 行 `mode=dry-run(print-mcp-tools)`；②**不传它** ⇒ 走 serve 模式（参数真进判据，不是摆设）；③该输出与 `tools/list` 名字序列**逐字相等**；④**故意让文档漂移 ⇒ 门禁红**（见③）。
- 🔄 **下一步**：①**stub 端点仍剩 1 个**：`PUT /documents/{sha}/extraction`（**新功能** ⇒ 需单独设计字段级 evidence 锚点）；②**T5 Web 管理端正式化**（目标验收①最后一块，**选型先报方案再动手**）；③「家庭自测趋势」专用视图仍未做；④公网暴露前 `EHR_BIND_ADDR` 改 `0.0.0.0` 前**必须**先设 token；⑤ghcr 包可见性仍 private；⑥真·第三方 MCP 客户端互通仍未验；⑦`.dcm`/OCR worker 未做；⑧**上游分类缺口**（`parser::classify` 认「出院记录」不认「出院小结」）仍在；⑨MCP 侧仍**无 resources/prompts** 能力；⑩**REST 面的路径数（16）仍是硬编码在 `api-doc-check.sh`/`ehtest.sh` 里**——本轮只治了 MCP 工具面，REST 路径面同病未治（可照本轮做法：OpenAPI 已是运行时真源，缺一个派生比对）。
- ⚠️ 未销账：目标验收①**Web 管理端仍是单文件最小版**（`ehr-api/web/index.html` 编译期内嵌，无构建链）；目标验收②的 6 个 stub 端点已实装 5 个，**剩 `extraction` 一个**；`docs/API.md` 是**生成物**，改接口必须跑 `./api-doc-check.sh --write`，手改会被 A3 门禁打回；文档覆盖的是 **REST 面**，`/mcp` 的工具契约仍在 `skills/medme-ehr/references/tool-contracts.md`（该文档的工具数**已由门禁与实现核对**，见 T15）；**上游分类缺口**仍在（D-027 未销账⑧）；**上游审计口径缺口**（`audit_log()` 不展示手动条目的合成导入）已在 D-030 如实标注（**不造事件补齐**）。

### 历史状态（2026-10-26 · **T14 MCP 侧补齐审计工具 `get_audit`（D-031）· MCP 工具数 11→12**）

- ✅ **T14 交付（MCP 侧补齐审计工具 `get_audit` · MCP 工具数 11→12 · 读侧 9→10）**：①**复用同一 `audit_view()`，不另写一份判据**（D-030 未销账②的原文要求）——MCP 工具**直接调用** REST handler 调的那个 `ehr_api::audit::audit_view()`，**零新判据、零新解析路径**；硬证据不是「两边都有数据」（那可以靠两套实现蒙混），而是 **MCP 返回的审计回执与 REST 返回的审计回执逐字相等**（`persons_scanned`/`count`/`entries[]` 全量相等）；②**审计是全局视图 ⇒ MCP 侧不做「人是否存在」的存在性检查**：问「某人有哪些审计条目」答「0 条」是合法答案（与 REST 同口径）⇒ **不返回 -32001**（否则同语义在两面分叉，agent 无法区分「查无此人」与「此人无记录」）；③**参数可选、别名只有一个**：`person`/`op` 都可选（缺省 = 全部人 / 全部动作），`person_id` 是 `person` 的**入参别名**（不是第二份判据）；④**口径分层在 MCP 面同样如实暴露**：`entries[].source` 区分 `registry`/`vault`；registry 侧无内容哈希 ⇒ `sha256` **键缺省**（不编）；vault 侧导入条目的 `sha256` **逐字等于**原件 sha256；上游 `audit_log()` 不展示手动条目的合成导入 ⇒ 笔记在 vault 侧**看不到行**（**不造事件补齐**）。
- ✅ **证据链（缺一即不算交付）**：
  - ①**`mcp-e2e.sh` 真进程 34 门 PASS / fails=0**：`tools_count` **11→12**（数字变化即客观证据）；**M1 两层真相层都在**（`sources == [registry, vault]`）；**M2 同源硬证据**（MCP `count` == REST `count` == **5**）；**M3 参数真进判据**（`person` 过滤 ⇒ 条目 `person_id` 全匹配；`op=CREATE`（大写）⇒ 1 条且全为 `create` ⇒ 大小写不敏感）；**M4 未知人 ⇒ 0 条**（不是错误码）。
  - ②**`cargo test --all-targets` 全绿**：新增 `tests/mcp.rs::mcp_audit_matches_rest_audit_view` **1 门**（MCP 与 REST 回执**逐字相等** + 两层都在 + vault sha256 == 原件 sha256 + registry **无** sha256 键 + person/op 过滤真生效 + 未知人 0 条）+ 旧门禁未破 ⇒ 合计 **61 门全绿**（lib 18 / apidoc 3 / audit 3 / auth 7 / conds 4 / contract 1 / entries 6 / **mcp 13** / meds 2 / rebuild 3 / sha_gate 1）。
  - ③**旧 e2e 门禁全绿未被破坏**：`ehtest.sh`(34) / `mcp-e2e.sh`(34) / `auth-e2e.sh` / `audit-e2e.sh` / `entries-e2e.sh` / `conds-e2e.sh` / `meds-e2e.sh` / `rebuild-e2e.sh` / `rebuild-lock-e2e.sh` / `skill-rest-check.sh` / `skill-contract-check.sh`（扩到 **12 工具 + 8 条新断言**）/ `api-doc-check.sh`(8) / `web-e2e.sh`（起真服务复验）**全 fails=0**。
  - ④**零新依赖**：`ehr-api/Cargo.lock`/`Cargo.toml` **未改动**（依赖包数 **204** 不变，可验事实）。
  - ⑤**dry-run 优先**：先跑库级 `cargo test --all-targets`（不起服务、不落状态）与 release 构建，再起真进程跑 mcp-e2e 与旧门禁复验。
  - ⑥**参数/状态生效有证据**：`tools_count` **11→12**；MCP 审计 `count` **== REST `count` == 5**；`op=CREATE` ⇒ **1**；未知人 ⇒ **0**。
  - ⑦**skill 层同步暴露**：`SKILL.md`（工具数 11→12 + REST↔MCP 对照表补 `audit` 行）+ `references/tool-contracts.md`（`get_audit` 契约行 + 「审计口径」小节）+ `skill-contract-check.sh` 扩到 12 工具 + 2 条新断言 ⇒ **agent 侧真的能调**。
- 🔄 **下一步**：①**stub 端点只剩 1 个**：`PUT /documents/{sha}/extraction`（**新功能** ⇒ 需单独设计字段级 evidence 锚点）——但**建议下一个里程碑仍是 T5 Web 管理端正式化**（目标验收①最后一块，选型先报方案再动手）；②**MCP 工具数在门禁里硬编码三处**（D-031 新坑②）——应改为从 `tools/list` 动态取值；③「家庭自测趋势」专用视图仍未做；④公网暴露前 `EHR_BIND_ADDR` 改 `0.0.0.0` 前**必须**先设 token；⑤ghcr 包可见性仍 private；⑥真·第三方 MCP 客户端互通仍未验；⑦`.dcm`/OCR worker 未做；⑧**上游分类缺口**（`parser::classify` 认「出院记录」不认「出院小结」）仍在。


- ✅ **T13 交付（`GET /api/v1/audit` 从 stub→501 毕业为真实现 · stub 端点 2→1）**：①**口径分层、不假装只有一层**——本服务有**两层真相层**，审计要回答的是「**发生过什么**」而不是「现在是什么」：`source=registry`（person registry 的 append-only **原始事件**，create/update/delete，**不折叠**——折叠会把 update 历史抹平、把 delete 墓碑藏掉）+ `source=vault`（上游内核事件日志）；②**vault 侧复用上游判据，不另写一份**：直接调用上游 `Vault::audit_log()`（上游自己的投影：导入/导出/分享/删除；`DocumentAdded`/`OcrAdded`/`ExtractionAdded` 是内部派生写入，上游刻意跳过）⇒ **本层不另写「哪些事件算审计条目」的判据**（D2：一个语义只允许一处实现），硬证据不是「两边都有数据」而是 **vault 侧条目与上游 `audit_log()` 逐条相等**（动作名/时刻/内容哈希）；③**registry 侧不臆造内容哈希**（registry 事件没有内容哈希 ⇒ `sha256` 缺省，不编）；④**参数真进判据**：`?person=`/`?op=` 是真过滤（`op` 大小写不敏感；vault 侧用上游中文动作名 `导入`/`导出`/`分享`/`删除`，registry 侧用 `create`/`update`/`delete`——**两种命名并存是如实的**，不擅自归一上游口径）；⑤**一条诚实的边界写进文档**（不粉饰）：上游 `audit_log()` **不展示**手动条目的合成导入 ⇒ 「写了一条自测/笔记」在 `source=vault` 里**看不到行**，只能在该人的 timeline/documents 里看到——**不造一条「写入」事件来补齐**（那是编造）。
- ✅ **证据链（缺一即不算交付）**：
  - ①**`audit-e2e.sh`（新增门禁）真进程 12 门 PASS / fails=0**：`/audit` → **200**；**M1 两层真相层都在**（registry=2 条 create + vault=1 条导入，count=3）；导入条目的 `sha256` **逐字等于**上传原件的 sha256（内容哈希留痕，可核验）；**M2 参数真进判据**（`all=3` → `?person=A`=2 / `?op=create`=2 / `?op=导入`=1 / 未知人=**0**）；**M3 OpenAPI 里是真 operation + typed schema**（`AuditReply`/`AuditEntryOut` 含 `source` 口径字段）；**M4 audit 不在 CONTRACT 表**；**M5 CONTRACT 表里剩 1 个 stub**（`extraction`）。
  - ②**`cargo test --all-targets` 全绿**：新增 `tests/audit.rs` **3 门**（库级：两层真相层都在 + **vault 侧与上游 `audit_log()` 逐条相等** + registry 侧不折叠 + 无内容哈希不臆造 / 过滤真生效（含大小写不敏感、别人 id ⇒ 0 条）/ HTTP 级：200 + person 过滤 + 不在 CONTRACT 表）+ 旧门禁**未破** ⇒ 合计 **60 门全绿**（lib 18 / apidoc 3 / audit 3 / auth 7 / conds 4 / contract 1 / entries 6 / mcp 12 / meds 2 / rebuild 3 / sha_gate 1）。
  - ③**旧 e2e 门禁全绿未被破坏**：`ehtest.sh`(34 门) / `mcp-e2e.sh` / `auth-e2e.sh` / `audit-e2e.sh` / `entries-e2e.sh` / `conds-e2e.sh` / `meds-e2e.sh` / `rebuild-e2e.sh` / `rebuild-lock-e2e.sh` / `skill-rest-check.sh` / `skill-contract-check.sh` / `api-doc-check.sh` / `web-e2e.sh`（起真服务复验，fails=0）**全 fails=0**。
  - ④**零新依赖**：`ehr-api/Cargo.lock` 与 `Cargo.toml` **未改动**（依赖包数 **204** 不变，可验事实）。
  - ⑤**dry-run 优先**：先跑库级 `cargo test --all-targets`（不起服务、不落状态）与 `--print-openapi` 干跑渲染（**paths=16 / operations=18**，audit 原是 stub 路径 ⇒ 路径数不变），再起真进程跑 audit-e2e 与旧门禁复验。
  - ⑥**参数/状态生效有证据**：`api-doc-check.sh` **fails=0**（文档 **35632 bytes** 与生成物逐字一致）；门禁 A5 的 stub 计数由 **2 降到 1**（数字变化即客观证据）；`?op=导入` ⇒ **1** 而 `?op=create` ⇒ **2** 而全量 ⇒ **3**（过滤器真进判据）；未知人 ⇒ **0**（不是 404/500）。
  - ⑦**skill 层同步暴露**：`ehr.sh audit [--person PID] [--op OP]` 新命令 + SKILL.md 命令表 1 行 + `references/rest-contracts.md` 契约行（两层真相层的口径说明）+ `skill-rest-check.sh` 扩到 **16 命令 + 5 条新断言**（两层都在 / person 过滤收窄 / op 过滤生效 / 未知人 0 条）⇒ **agent 侧真的能调**。
- ✅ **已提交并推送**：`9981c8e`（`feat(audit): T13 审计端点实装（D-030）`）→ `main`（D-015 通道，push 成功）。
- 🔄 **下一步**：①**stub 端点只剩 1 个**：`PUT /documents/{sha}/extraction`——**新功能**（非「补读侧」）⇒ 需单独设计（字段级 evidence 锚点该怎么表达？上游 `deid::verify` 要求每个字符串字段是**原文逐字子串**，且 `evidence` 必须是原文逐字子串）⇒ **建议下一个里程碑仍是 T5 Web 管理端正式化（选型先报方案再动手）**——目标验收①最后一块；②**MCP 侧还没有审计工具**（REST 独有）——若补 **须复用同一 `audit_view()`**，不得另写一份判据；③「家庭自测趋势」专用视图仍未做（agent 需自己按 `self_measured` 过滤）；④公网暴露前：`EHR_BIND_ADDR` 改 `0.0.0.0` 前**必须**先设 token；⑤ghcr 包可见性仍 private；⑥真·第三方 MCP 客户端互通仍未验；⑦`.dcm`/OCR worker 未做。
- ⚠️ 未销账：目标验收①**Web 管理端仍是单文件最小版**（`ehr-api/web/index.html` 编译期内嵌，无构建链）；目标验收②的 6 个 stub 端点已实装 5 个，**剩 `extraction` 一个**（文档已逐个标注）；**MCP 侧工具面仍无审计工具**（REST 独有，SKILL.md 已如实标注）；`docs/API.md` 是**生成物**，改接口必须跑 `./api-doc-check.sh --write`，手改会被 A3 门禁打回；文档覆盖的是 **REST 面**，`/mcp` 的工具契约仍在 `skills/medme-ehr/references/tool-contracts.md`；**上游分类缺口**（`parser::classify` 认「出院记录」不认「出院小结」）仍在（D-027 未销账⑧）；**上游审计口径缺口**（`audit_log()` 不展示手动条目的合成导入）已在 D-030 如实标注（**不造事件补齐**）。

- ✅ **T11 交付（`/persons/{pid}/measurements` + 三个 `POST /persons/{pid}/entries/*` 从 stub→501 毕业为真实现）**：①**写侧三类手动条目**（`self-measurement`/`note`/`profile-event`）**同走一条写入路径**（`add_synthetic_document`：合成文本当文件 → `vault.import` → `add_document` → `add_ocr`）——抄上游 mobile 端同一手法，**不写三份**；②**CAS 按内容去重会静默吞写** ⇒ 三类条目正文一律带**毫秒级记录时间行** ⇒ 每次写入字节必不同（回执 `deduped` 如实标注，不假装写成功）；③**数值只做「生理学可能性」兜底**（复用上游 `parser::validate_self_measured_values`：138388 mmHg / 收缩压≤舒张压 ⇒ **400 带原因**），**不换算单位、不做 analyte 白名单、查不到出处就不给区间**（`flag` 恒缺省，不猜「正常」）；④读侧 `measurements` 逐字用上游 `parse_*_payload` 读回，**读不出来就标 `payload_readable=false`**（原文仍在 `text` 里，半猜比不猜坏）；⑤**家庭自测值与医院指标结构上分开**（上游 `GroupKey::SelfMeasured`）**且必须说出去** ⇒ `LabSeriesOut` 如实透传 `self_measured` 标志（上游不告诉消费方，不告诉 ⇒ 消费方会把家庭值当医院值解读）；⑥`kind`/`from`/`to` **真过滤**（非法 kind/日期/`from>to` ⇒ 400）；⑦四个端点从 `CONTRACT` 表移出、正式进 utoipa `paths(...)`。
- ✅ **顺手修掉一个真并发 bug**：`tests/auth.rs` 的 `fixture()` 原先**只用 `scope_spec` 派生目录名** ⇒ 同一 spec 的两个测试**共享同一个 registry 目录** ⇒ cargo test 并行时同时追加 `persons.jsonl` ⇒ 行交错 ⇒ 随机 500（实测 8 跑 3 红，像「抖动」，实为**真 bug**）⇒ 目录名加**进程内单调计数器**，每次调用独占（D-027 暂存目录互删的镜像）。
- ✅ **证据链（缺一即不算交付）**：
  - ①**`entries-e2e.sh`（新增门禁）真进程 PASS / fails=0**：三类写入全 201；**M1 写入真落地 + 相同内容未被吞写**；`GET /measurements` **200**、载荷**逐字读回**（区间带 `ref_source`、事件日透传、笔记原文原样）；**M2 provenance verified measurements=4**（每条 docref 都能经 HTTP 取回原件字节并含上游载荷哨兵）；**M3 数值不被粉饰**（不可能值 ⇒ 400 带原因；未知 analyte ⇒ 201 且无区间无标志）；**M4 过滤器真过滤**（`kind=note` 只回 2 条；非法 kind/日期 ⇒ 400）；**M5 OpenAPI 里是真 operation + typed schema**；**M6 不回归**（labs/medications/conditions 仍正常，自测值只进自测序列且标 `self_measured=true`）；未知人 **404**；**M8 四端点不在 CONTRACT 表**。
  - ②**`cargo test --all-targets` 全绿**：新增 `tests/entries.rs` **6 门** + 旧门禁未破 ⇒ 合计 **55 门全绿**。
  - ③**旧 e2e 门禁全绿未被破坏**：`ehtest.sh`(34 门) / `mcp-e2e.sh` / `auth-e2e.sh` / `rebuild-e2e.sh` / `rebuild-lock-e2e.sh` / `skill-rest-check.sh`（扩到 **15 命令 + 10 条新断言**） / `skill-contract-check.sh` / `api-doc-check.sh`(8 门) / `web-e2e.sh`（起真服务复验，fails=0）**全 fails=0**。
  - ④**零新依赖**：`ehr-api/Cargo.lock` 与 `Cargo.toml` **未改动**（依赖包数 **204** 不变，可验事实）。
  - ⑤**dry-run 优先**：先跑库级 `cargo test --all-targets` 与 `--print-openapi` 干跑渲染（**paths=16 / operations=18**），再起真进程跑 entries-e2e 与旧门禁复验。
  - ⑥**参数/状态生效有证据**：`api-doc-check.sh` **fails=0**（文档 **33194 bytes** 与生成物逐字一致）；门禁 A5 的 stub 计数由 **6 降到 2**（数字变化即客观证据）；`?kind=nonsense` ⇒ **400**、`?from=not-a-date` ⇒ **400**、`values=[138388 mmHg]` ⇒ **400**（参数真进到判据里）。
  - ⑦**skill 层同步暴露**：新增 `ehr.sh measurements/self-measure/note/profile-event` 四个命令 + SKILL.md 命令表 + `references/rest-contracts.md` 契约行（含新字段与不变量「家庭自测值 ≠ 医院指标」）⇒ **agent 侧真的能调**（`skill-rest-check.sh` 逐条断言）。
- 🔄 **下一步**：①**stub 端点只剩 2 个**：`PUT /documents/{sha}/extraction`、`GET /audit`——都是**新功能**（非「补读侧」）⇒ 需单独设计，**建议下一个里程碑是 T5 Web 管理端正式化（选型先报方案再动手）**——目标验收①最后一块；②`medications`/`conditions`/`measurements` 的 **MCP 侧工具已补齐**（D-029，复用同一聚合/投影函数）——MCP 工具数 **11**（9 读 + 2 写）；③**还没有「家庭自测趋势」专用视图**（agent 需自己按 `self_measured` 过滤）；④T3 待办未打勾（其实已交付，待与 T5 合并收口）；⑤公网暴露前：`EHR_BIND_ADDR` 改 `0.0.0.0` 前**必须**先设 token；⑥ghcr 包可见性仍 private；⑦真·第三方 MCP 客户端互通仍未验；⑧`.dcm`/OCR worker 未做。
- ⚠️ 未销账：目标验收①**Web 管理端仍是单文件最小版**（`ehr-api/web/index.html` 编译期内嵌，无构建链）；目标验收②的 6 个 stub 端点已实装 4 个，**剩 `extraction`/`audit` 两个**（文档已逐个标注）；`medications`/`conditions`/`measurements` **MCP 侧工具已补齐**（D-029）；`docs/API.md` 是**生成物**，改接口必须跑 `./api-doc-check.sh --write`，手改会被 A3 门禁打回；文档覆盖的是 **REST 面**，`/mcp` 的工具契约仍在 `skills/medme-ehr/references/tool-contracts.md`；**上游分类缺口**（`parser::classify` 认「出院记录」不认「出院小结」）仍在（D-027 未销账⑧）；**shell 拼 JSON 的空数组坑**（`printf '%s	' "${PAIRS[@]}"` 空数组 ⇒ python 侧 unpack 崩）已修但值得记住。

### 历史状态（2026-10-23 · **T10 病程泳道端点已实装（D-027）· stub 端点 7→6**）

- ✅ **T10 交付（`/persons/{pid}/conditions` 从 stub→501 毕业为真实现）**：①`vaults.rs` 新增 `conditions_aggregate()`——与 `labs`/`medications` **同一次 `parser::aggregate()`** 的第三块输出，**同源同判据**（复用 D-026 抽出的 `collect_agg_rows()` + `rows_to_source_docs()`，**不另写一份取样逻辑**）；②**判据分工如实透传上游语义**（D-027 决策②）：上游 conditions 面**没有** doc_type 白名单，只跳过「手动录入类」文档 ⇒ 本层**不加私造白名单**；③**不推断 = 不粉饰**：`raw_text` 原样透传（上游无术语归一字典）、ICD 码**仅当原件印了才有**、`onset` 缺失即 `None`；④每条诊断带 `docrefs`（**跨文档合并后保留全部出处**）⇒ 可一路溯源到原件字节（D2）；⑤`conditions` 从 `CONTRACT` 表移出、正式进 utoipa `paths(...)` ⇒ OpenAPI/文档不再宣称 501（反向保险 `contract_entries_really_return_501` 保留，conds-e2e M5 门专门断言）。
- ✅ **顺手修掉一个真并发 bug（D-027 决策⑥）**：暂存目录名此前**只按内容哈希**命名 ⇒ **同内容**原件**并发**摄取共用目录 ⇒ 一方 `remove_dir_all` 删掉另一方暂存文件 ⇒ 对方解析失败 ⇒ 该文档**静默落成 raw-only**（无文本层）⇒ 在 labs/medications/conditions/search 四个面**全部隐形**，而上传接口照样回 200（典型的「以为有、其实没有」D5）。**症状像业务抖动**（实测 conds 门禁 5 跑 3 红）。修法 = 目录名加 **进程 id + 单调计数器**（`AtomicU64`）⇒ 每次调用独占，**零新依赖**；已用单测 `staging_dir_is_unique_per_call` 钉死不变量。
- ✅ **证据链（缺一即不算交付）**：
  - ①**`conds-e2e.sh`（新增门禁）真进程 12 门 PASS / fails=0**：`/conditions` → **200**；**M1 反凭空造**（化验单 docref 不出现）+ ICD 只透传不臆造 + 原文写法原样；**M2 provenance verified conditions=4**（每条诊断原文都在**经 HTTP 取回的其 docref 原件**里逐字出现）；**M3a labs 未退化**（诊断行未被读成假指标）；**M3b medications 未退化**；**M4 OpenAPI 里是真 operation + typed schema**；未知人 **404**；**M5 不在 CONTRACT 表**。
  - ②**`cargo test --all-targets` 全绿**：新增 `tests/conds.rs` **4 门**（库级聚合 + 证据回链 + 反凭空造 + ICD 透传 + labs/meds 不回归；HTTP 级端点 200/回链/不在 CONTRACT 表）+ 旧门禁**未破** ⇒ 合计 **45 门全绿**。
  - ③**旧 e2e 门禁全绿未被破坏**：`ehtest.sh`(34 门) / `mcp-e2e.sh` / `auth-e2e.sh` / `rebuild-e2e.sh` / `rebuild-lock-e2e.sh` / `skill-rest-check.sh` / `skill-contract-check.sh` / `api-doc-check.sh`(8 门) / `web-e2e.sh`（起真服务复验 fails=0）**全 fails=0**。
  - ④**零新依赖**：`ehr-api/Cargo.lock` 与 `Cargo.toml` **未改动**（依赖包数 **204** 不变，可验事实）。
  - ⑤**dry-run 优先**：先跑库级 `cargo test --all-targets` 与 `--print-api-doc` 干跑渲染，再起真进程跑 conds-e2e 与旧门禁复验。
  - ⑥**参数/状态生效有证据**：`api-doc-check.sh` 首跑 **fails=1**（文档仍写 501＝漂移）⇒ `--write` 重生成（**25746 bytes**）⇒ 复跑 **fails=0**；门禁 A5 的 stub 计数由 **7 降到 6**。
- 🔄 **下一步**：①**stub 端点继续毕业**（剩 **6 个**：`measurements`/三个 `entries`/`extraction`/`audit`）——**注意 `measurements` 与前几个不同类**：它是 `SelfMeasurement/ProfileEvent` 事件回放投影，而**其写侧（三个 `entries`）都还是 501** ⇒ 无法像 conditions 那样只补读侧就毕业，**需先决定写侧是否纳入范围**；②**T5 Web 管理端正式化（选型先报方案再动手）**——目标验收①的最后一块；③T3 待办未打勾（其实已交付，待与 T5 合并收口）；④公网暴露前：`EHR_BIND_ADDR` 改 `0.0.0.0` 前**必须**先设 token；⑤ghcr 包可见性仍 private；⑥真·第三方 MCP 客户端互通仍未验；⑦`.dcm`/OCR worker 未做。
- ⚠️ 未销账：目标验收①**Web 管理端仍是单文件最小版**（`ehr-api/web/index.html` 编译期内嵌，非正式前端，无构建链）；目标验收②中 `medications`/`conditions` **已实装**（D-026/D-027），但 `measurements`/三个 `entries`/`extraction`/`audit` 共 **6 个端点仍是 stub→501**（文档已逐个标注）；`medications`/`conditions` **MCP 侧暂无对应工具**（REST 独有，SKILL.md 已如实标注）；`docs/API.md` 是**生成物**，改接口必须跑 `./api-doc-check.sh --write`，手改会被 A3 门禁打回；文档覆盖的是 **REST 面**，`/mcp` 的工具契约仍在 `skills/medme-ehr/references/tool-contracts.md`；**上游分类缺口**（`parser::classify` 认「出院记录」不认「出院小结」⇒ 未分类文档走 labs 全文正则 ⇒ 诊断行被读成假化验指标，非本轮引入的回归）已用门禁钉死事实，修法不在本层（D5）。

### 历史状态（2026-10-23 · **T9 用药端点已实装（D-026）· stub 端点 8→7**）
- ✅ **T9 交付（`/persons/{pid}/medications` 从 stub→501 毕业为真实现）**：①`vaults.rs` 抽出**共用取样路径** `collect_agg_rows()` + `rows_to_source_docs()` ⇒ `labs` 与 `medications` **同源同判据、同一次 `parser::aggregate()`**（一个语义只允许一处实现）；②**`doc_type` 如实传给 `aggregate`**（D-026 决策①，最硬的一条）：已分类的必须如实传（否则化验单被当处方挖药，「肌酐 112 umol/L」读成 112U 药），**判不出类型的（unknown）要还原成 `None`**（上游语义「None=宽松放行」，传 `Some("unknown")` 会让该文档**两个面都读不出**——实测 labs 变 0 条）；③每条 mention 带 `docref`（=出处原件 sha256）、药名/剂量**原样透传不改写**、`status` 恒 `active` **不擅自改写成「已停」**；④`medications` 从 `CONTRACT` 表移出、正式进 utoipa `paths(...)` ⇒ OpenAPI/文档不再宣称 501（反向保险 `contract_entries_really_return_501` 保留）。
- ✅ **证据链（缺一即不算交付）**：
  - ①**`meds-e2e.sh`（新增门禁）真进程 12 门 PASS / fails=0**：`/medications` → **200**；**M1 类型分流**（用药只来自处方，化验单 docref 不出现）；**M2 provenance verified mentions=3**（每条 mention 的药名/剂量都在**经 HTTP 取回的其原件原文**里逐字出现——「docref 指向某原件」与「药名真从那份原件来」是两件事）；**M3 labs 未退化**（同一化验单仍读出指标点、docref 仍指向化验单）；**M4 OpenAPI 里是真 operation + typed schema**；未知人 **404**；**M5 不在 CONTRACT 表**。
  - ②**`cargo test --all-targets` 全绿**：新增 `tests/meds.rs` **2 门**（库级聚合 + HTTP 级端点/证据回链/CONTRACT 断言）+ 旧门禁**未破** ⇒ 合计 **40 门全绿**。
  - ③**旧 e2e 门禁全绿未被破坏**：`ehtest.sh`(34 门) / `mcp-e2e.sh` / `auth-e2e.sh` / `rebuild-e2e.sh` / `rebuild-lock-e2e.sh` / `skill-rest-check.sh` / `skill-contract-check.sh` / `api-doc-check.sh`(8 门) / `web-e2e.sh`（起真服务复验）**全 fails=0**。
  - ④**零新依赖**：`ehr-api/Cargo.lock` 与 `Cargo.toml` **未改动**（`git diff --stat` 为空，可验事实）。
  - ⑤**dry-run 优先**：先跑库级 `cargo test` 与 `--print-api-doc` 干跑渲染，再起真进程跑 meds-e2e 与旧门禁复验。
  - ⑥**新坑（已修）**：`skill-rest-check.sh` 首跑 fails=3 的真凶是**陈旧 release 二进制**（10月20构建，早于 D-026）——不是代码坏。`cargo build --release` 后 fails=0。**教训：门禁红 ⇒ 先问「它跑的是哪份二进制」，再怀疑代码**（D-011「红门先查门禁自身」的新变种：红的是**构建产物新鲜度**）。另：`ehtest.sh`/`rebuild-*.sh` 需显式传二进制绝对路径、`web-e2e.sh` 需先起真服务，二者的 usage 提示极易被误读成「门禁失败」。
- 🔄 **下一步**：①**stub 端点继续毕业**（剩 **7 个**：`conditions`/`measurements`/三个 `entries`/`extraction`/`audit`）——`conditions` 与 `medications` 同出 `parser::aggregate` 的 `agg.conditions` 面，成本最低，是下一个自然的小步；②**T5 Web 管理端正式化（选型先报方案再动手）**——目标验收①的最后一块；③T3 待办未打勾（其实已交付，待与 T5 合并收口）；④公网暴露前：`EHR_BIND_ADDR` 改 `0.0.0.0` 前**必须**先设 token；⑤ghcr 包可见性仍 private；⑥真·第三方 MCP 客户端互通仍未验；⑦`.dcm`/OCR worker 未做。
- ⚠️ 未销账：目标验收①**Web 管理端仍是单文件最小版**（`ehr-api/web/index.html` 编译期内嵌，非正式前端，无构建链）；目标验收②中 `medications` **已实装**（D-026），但 `conditions`/`measurements`/三个 `entries`/`extraction`/`audit` 共 **7 个端点仍是 stub→501**（文档已逐个标注）；`medications` **MCP 侧暂无对应工具**（REST 独有，SKILL.md 已如实标注）；`docs/API.md` 是**生成物**，改接口必须跑 `./api-doc-check.sh --write`，手改会被 A3 门禁打回；文档覆盖的是 **REST 面**，`/mcp` 的工具契约仍在 `skills/medme-ehr/references/tool-contracts.md`。

### 历史状态（2026-10-22 · **T7 重建期写互斥已交付（D-024）· T6 全量重建已交付（D-023）**）

- ✅ **T7 交付（销账 D-023 未销账项①「重建期间禁写」互斥）**：①新增 `ehr-api/src/rebuildlock.rs`——**跨进程锁文件** `{EHR_HOME}/.rebuild.lock`（`O_CREAT|O_EXCL` 原子创建，锁内写 `pid/ts/mode`），`RebuildLock` 为 RAII 守卫（drop 即释放文件）；②`--rebuild-index`（真重建）**持锁执行** ⇒ 重建期间运行中的服务对**业务路径一律 503**（`Retry-After: 5`，body 写明「rebuild in progress (pid=…, since=…)」），公开路径（healthz/openapi.json/首页）**放行**（不碰派生层，不该被误伤）；③**并发重建直接被拒**（第二个 `--rebuild-index` 非零退出 + 报 `already held`），不排队、不静默；④**陈旧锁自动回收**（锁内 pid 进程已不存在 ⇒ 视为崩溃残留，服务面不阻塞、下一重建回收）⇒ `kill -9` 不会把服务永久锁死；⑤锁文件**放 home 根而非 `vaults/` 下** ⇒ 重建不会删掉自己的锁（不自锁）。
- ✅ **证据链（缺一即不算交付）**：
  - ①**`rebuild-lock-e2e.sh`（新增门禁）真进程 15 门 PASS / fails=0**：基线业务路径 200（L1）→ 持锁后同一路径 **503** 且 body 含 `rebuild in progress`、带 `Retry-After` 头（L2a/b/c）→ 持锁期间 healthz/openapi.json/首页**仍 200**（L3 ×3）→ 并发重建 **rc=1** 且报 `already held`（L5a/b）→ 释放锁后**立刻恢复 200**（L4）→ 陈旧锁(pid=4294967295) **不阻塞服务**且可被重建回收（L6a/b/c）→ 锁文件不在 `vaults/` 下（L7）。
  - ②**`cargo test --all-targets` 全绿**：新增 `rebuildlock` 单测 **2 门**（取锁后二次取锁失败且报错带原因 + 释放后可再取；陈旧锁被回收且服务面不阻塞）+ 旧门禁 **32 门未破**（lib 10→12 / auth 7 / contract 1 / mcp 10 / rebuild 3 / sha_gate 1）。
  - ③**旧 e2e 门禁全绿未被破坏**：`ehtest.sh` 34 门 fails=0、`mcp-e2e.sh` fails=0、`auth-e2e.sh` fails=0、`rebuild-e2e.sh` fails=0、`skill-rest-check.sh` fails=0、`skill-contract-check.sh` fails=0、`web-e2e.sh` fails=0（起真服务复验）。
  - ④**零新依赖**：`ehr-api/Cargo.lock` 未改动（锁用 `std::fs::OpenOptions::create_new` 的原子语义，不引 libc/flock）。
  - ⑤**参数生效有证据**：锁的**有无**直接改变服务行为（无锁 200 / 有锁 503），且锁释放后**自动恢复**——不是「传进去没反应」也不是「一次性开关」。
- ✅ **T6 交付（目标验收④销账：派生索引可全量重建，有重建脚本证明）**：①新增 `ehr-api/src/rebuild.rs`——**显式命令** `ehr-api --rebuild-index`（全部人）/ `--rebuild-index=<pid>`（单人）/ `--rebuild-index=dry-run`（只列计划，不动文件）；②动作 = **删派生层（`medme.db`+wal/shm）→ 从事件日志全量回放**（走内核 `open_with_device_id_resilient`，与 POC 步骤09 同一条路径），重建后当场量出 `timeline_entries`/`lab_series`/`lab_points`/`search_probe_hits` 四项计数作为证据；③**真相层零改动**（`log/`、`objects/` 只读）；④JSON 报告走 stdout、`[证据]` 行走 stderr（沿用 D1 惯例，机器可读）。
- ✅ **T6 证据链**：`rebuild-e2e.sh` 真进程 22 门 PASS / fails=0（dry-run 不删库 R1d → 真重建 exit=0 且 `derived_db_removed=true`/`rebuilt_from_log=true` → 重建前后 timeline 逐字相等 2 vs 2 → FTS 中文探针命中 2 → 真相层 log/+objects/ 仍在 → 重建后再起服务 HTTP 读侧三项计数逐字相等 R6b → 脏参数非零退出 R5）；`tests/rebuild.rs` 3 门；`cargo test --all-targets` 全绿；`ehtest.sh` 34 门 fails=0；**零新依赖**（Cargo.lock 未改动）。
- 🔄 **下一步**：①T5 Web 管理端正式化（选型先报方案再动手）；②公网暴露前：`EHR_BIND_ADDR` 改 `0.0.0.0` 前**必须**先设 token；③ghcr 包可见性仍 private；④真·第三方 MCP 客户端互通仍未验；⑤`.dcm`/OCR worker 未做。
- ⚠️ 未销账：重建互斥**是跨进程锁文件**，**不覆盖多机共享存储**（NFS/多副本部署需换分布式锁——本项目部署形态是单副本容器，暂不引入）；503 期间客户端需自行重试（`Retry-After: 5`），**无排队/无补偿**；派生层重建**不重建 registry 层**（`registry/persons.jsonl` 本身即真相，无派生）；ghcr 包可见性、`.dcm`/OCR 未做。

### 历史状态（2026-10-22 · **T6 派生索引全量重建已交付（D-023）**）


- ✅ **T6 交付（目标验收④销账：派生索引可全量重建，有重建脚本证明）**：①新增 `ehr-api/src/rebuild.rs`——**显式命令** `ehr-api --rebuild-index`（全部人）/ `--rebuild-index=<pid>`（单人）/ `--rebuild-index=dry-run`（只列计划，不动文件）；②动作 = **删派生层（`medme.db`+wal/shm）→ 从事件日志全量回放**（走内核 `open_with_device_id_resilient`，与 POC 步骤09 同一条路径），重建后当场量出 `timeline_entries`/`lab_series`/`lab_points`/`search_probe_hits` 四项计数作为证据；③**真相层零改动**（`log/`、`objects/` 只读）；④JSON 报告走 stdout、`[证据]` 行走 stderr（沿用 D1 惯例，机器可读）。
- ✅ **证据链（缺一即不算交付）**：
  - ①**`rebuild-e2e.sh`（新增门禁）真进程 22 门 PASS / fails=0**：dry-run 计划列出「派生文件清单 + truth_present」且**实测未删库**（R1d）→ 真重建 exit=0 且报告 `derived_db_removed=true`/`rebuilt_from_log=true` → **重建前后 timeline 条数逐字相等**（2 vs 2）、FTS 探针中文命中 2（索引真热，不只是表建出来）→ **真相层 log/+objects/ 仍在** → 重建后**再起服务，HTTP 读侧 timeline/labs/search 三项计数与重建前逐字相等**（R6b）→ 脏参数 `--rebuild-index=nonsense` 必须非零退出（R5）。
  - ②**`cargo test --all-targets` 全绿**：新增 `tests/rebuild.rs` **3 门**（未知 person_id 报错**且不建空箱**；空 home 计划为空非报错；重建后计数与二次重建逐字相等）+ 旧门禁 30 门未破（lib 10 / auth 7 / contract 1 / mcp 10 / sha_gate 1）。
  - ③**旧 e2e 门禁全绿未被破坏**：`ehtest.sh` 34 门 fails=0、`mcp-e2e.sh` fails=0、`auth-e2e.sh` fails=0、`skill-rest-check.sh` fails=0、`skill-contract-check.sh` fails=0、`web-e2e.sh` fails=0。
  - ④**零新依赖**：`ehr-api/Cargo.lock` 未改动。
  - ⑤**参数生效有证据**：`--rebuild-index=dry-run` 的 `[证据]` 行显式 `mode=rebuild(dry-run)`；真重建行 `mode=rebuild | vaults=1 | timeline_entries=2 | lab_points=5`——参数传进去 = 有对应行为差异（dry-run 不删库 / 真重建删库回放）。
- 🔄 **下一步**：①T5 Web 管理端正式化（选型先报方案再动手）；②公网暴露前：`EHR_BIND_ADDR` 改 `0.0.0.0` 前**必须**先设 token；③ghcr 包可见性仍 private；④真·第三方 MCP 客户端互通仍未验；⑤`.dcm`/OCR worker 未做。
- ⚠️ 未销账：重建是**离线运维动作**（需停服或容忍并发写者——本轮未做「重建时禁写」互斥；vault 侧 SQLite 单写者约束由内核兜底，实践中建议停服重建）；派生层重建**不重建 registry 层**（`registry/persons.jsonl` 本身即真相，无派生）；ghcr 包可见性、`.dcm`/OCR 未做。



### 历史状态（2026-10-21 · **T4 API token 鉴权已交付（D-022）**）

- ✅ **T4 交付（静态 Bearer token 鉴权）**：①新增 `ehr-api/src/auth.rs`——env 注入的静态 token 表（`EHR_API_TOKENS` 多 token / `EHR_API_TOKEN` 单 token 兼容），**scope 分权 ro/rw**，手写常量时间比较，**零新依赖**；②`axum` 统一鉴权中间件 `auth_layer`（`from_fn_with_state`）挂在最外层 Router ⇒ `/api/v1/*`、`/mcp` 一次覆盖；③**401 vs 403 分开**（401=缺/错 token⇒换 token；403=只读 token 想写⇒换读写 token）；④MCP 侧**工具粒度分权**（ro token 调写侧工具 ⇒ JSON-RPC `-32004`，不是 HTTP 403）；⑤OpenAPI 注入 `components.securitySchemes.bearerAuth` + 全局 `security`，`/healthz` 显式 `security: []`；⑥Web 管理端加 token 输入框（localStorage / `?token=`），页面本身仍免鉴权但 XHR 带 token。
- ✅ **证据链（缺一即不算交付）**：
  - ①`cargo test --all-targets` 全绿：**auth 单测 5 门**（scope 解析/拒绝带原因/权限矩阵/认证拒绝/常量时间比较）+ **tests/auth.rs 7 门**（无 token 401 于 persons/timeline/labs//mcp；错 token 401；裸 token 401；ro 读 200；ro 写 403；rw 写 201；MCP ro→写工具 -32004 / ro→读工具 ok；openapi 声明 bearerAuth；open 模式放行）；**旧门禁未被破坏**：`ehtest.sh` **34 门 fails=0**、`mcp-e2e.sh` 22 门 fails=0、`skill-rest-check.sh` fails=0、`skill-contract-check.sh` fails=0、`web-e2e.sh` fails=0。
  - ②**`auth-e2e.sh`（新增门禁）真进程 21 门 PASS / fails=0**：起真服务（`EHR_API_TOKENS=rwsecret:rw,rosecret:ro`）→ 启动 `[证据]` 行显示 **`auth=bearer(tokens=2)`** → 无/错/裸 token 全 401 → 公开路径（healthz/openapi/首页）200 → ro/rw 读全 200 → **ro 写 403、rw 写 201** → **被拒的写不留半成品**（persons 里查无「只读不该建成」）→ MCP ro 调写侧工具 `-32004` → openapi 含 bearerAuth → **open 模式**（不配 token）放行且 `[证据]` 行标 `auth=open`。
  - ③**零新依赖**：`Cargo.lock` 未改动（依赖数不变）——鉴权面手写实现（token 表解析 + 常量时间比较）不引 subtle/argon2。
  - ④**参数生效有证据**：`EHR_API_TOKENS` 通过真实 env 注入后，`[证据]` 行的 token 计数与实际放行行为**逐条对齐**（tokens=2 ⇒ 两个 token 各自的行为符合 scope 矩阵）；compose 三个编排文件均已注入 `EHR_API_TOKENS`/`EHR_API_TOKEN`（`docker compose config` 实测可见）。
- 🔄 **下一步**：①T5 Web 管理端正式化（选型先报方案再动手）；②公网暴露前的运维项：`EHR_BIND_ADDR` 由 `127.0.0.1` 改 `0.0.0.0` 前必须先设 token（否则 open 模式裸奔）；③ghcr 包可见性仍 private；④真·第三方 MCP 客户端互通仍未验（MCP 已降为可选壳，优先级下调）。
- ⚠️ 未销账：`EHR_API_TOKEN` 是**静态 token**（无过期/无撤销列表/无抗爆破）⇒ 定位是「最小可用安全基线」，不是 IAM；生产建议定期轮换 + 走 TLS 反代。`.dcm`/OCR worker 未做；volume 内累计人档历轮验收遗留。

### 历史状态（2026-10-20 · **T3 免维护接入已交付（D-021）**）

- ✅ **T3 交付（免维护接入）**：①README 全新「一键部署」块（ghcr 编排 + local 编排双写法 + 部署后自检口径 + 数据落 volume 说明）+「AI agent 接入」块（clone → export EHR_API_BASE/EHR_API_TOKEN → 放 skill 目录的宿主对照表）；②**SKILL 重写为 REST 优先**（新增 `skills/medme-ehr/scripts/ehr.sh` 纯 curl helper + `references/rest-contracts.md` 字段契约；MCP 降为可选壳并给出 REST↔MCP 工具一一对照表）。**验收口径=「升级不改配置」**：API base 与 token 全走 env，skill 内不写死地址/token（门禁逐条断言）。
- ✅ **证据链（缺一即不算交付）**：
  - ①`skill-rest-check.sh`（新增门禁）真进程 **fails=0**：7 组字段名（persons/timeline/search/labs/documents/detail/upload，共 52 个）逐个存在于**真实响应**；命令面 SKILL↔helper 10 个对齐；不变量 3 条（labs 带 docref / 搜索命中=1 / 罕见词=0）。
  - ②`agent-e2e.sh`（新增门禁）**真·第三方 agent（Copilot CLI 1.0.98 + 本地 Strata）** 在**只看得见 skill 目录**的沙箱里完成真实查档：找到「一键部署验证」、报出白细胞 **13.5**（H 偏高）、给出 docref `43b5077d…` ⇒ **SKILL 零注册接入被真 agent 兑现**（D-018 期望的"仅 skill+服务即可用全功能"首次有端到端证据）。
  - ③ghcr 编排端到端仍 GREEN：`up -d` → 容器 stderr `[证据] ehr-api v0.0.1 | mode=serve | EHR_HOME=/home/ehr | ADDR=0.0.0.0:8443` → healthz 200 → openapi paths=16 → 经 **ehr.sh** 建档+上传+labs（wbc 13.5 H，docref 逐字）→ Web 首页 200/8591B。
  - ④旧 `skill-contract-check.sh`（MCP 侧）**fails=0** 未被本轮改动破坏（SKILL.md 增补 REST↔MCP 对照表后 8 工具名门禁仍全绿）。
- 🔄 **下一步（阶段4 收口 → T4）**：T4 API token 鉴权（静态 Bearer，多 token 按只读/读写分权，中间件统一拦截，OpenAPI 标 securitySchemes；验收=无 token 401、对 token 全绿）。**注意**：本轮起 `EHR_API_TOKEN` 已在 skill helper 里预留但**服务端尚未校验**（填了也不生效）⇒ T4 是「零维护接入」的最后一环。
- ⚠️ 未销账：ghcr 包可见性=private（对外 pull 需网页设 public 或 PAT 加 `admin:packages`）；真·第三方 MCP 客户端互通未验（本轮 SKILL 主路径已由 Copilot CLI 兑现，MCP 为可选壳，优先级下调）；`.dcm`/OCR worker 未做；volume 内累计人档已增至 10（历轮验收遗留，要干净基线 `docker volume rm deploy_ehr-home`）。

### 历史状态（阶段4 第二步②，D-019）

- ✅ **MCP 写侧交付（D-019）**：MCP 工具数 **6 → 8**，新增 `create_person`（建档）与 `upload_document`（传原件）⇒ agent 全流程**不必绕 REST**。
- ✅ 证据：`cargo test --all-targets` 全绿（**b64 5 + contract 1 + mcp 10** + sha_gate 1）；`mcp-e2e.sh` 真进程 **22 门 PASS / fails=0**（含写侧闭环：create_person→upload_document→search/labs 立刻可见→重传 deduped=true、sha 与解码后字节 sha256 逐字相等）；`skill-contract-check.sh` **18 门 PASS / fails=0**（写侧 5 字段 + create_person 7 字段逐个存在于真实写侧响应）；**Cargo.lock 依赖数 204 未变（零新依赖）**。
- ✅ **base64 手写实现**（`src/b64.rs`，标准字母表 + 严格模式：padding 自洽 + 丢弃位必须为 0 + 忽略折行空白），自带 5 个单测（RFC4648 向量 / 256 字节二进制往返 / 垃圾输入拒收）。**理由**：JSON 传二进制的通用做法就是 base64；手写可被测试钉死，零外部依赖（同 D-017 选型逻辑）。
- ✅ **参数校验带原因**：校验失败时 `content[0].text` 给出具体原因（如 "`content_base64` is not valid base64: ..."）⇒ agent 知道怎么改，而不是只拿到一个码。
- 🔄 **下一步（阶段4 第三步）**：SKILL 注册进 `~/.qwenpaw/skill_pool`（真·agent 自动发现未验）→ 与 T3「免维护接入块」合并做；随后 T4 token 鉴权。
- ⚠️ 未销账：真·第三方 MCP 客户端互通未验（D-017③ 同类，代偿=协议层断言+双面一致性）；`.dcm`/OCR worker 未做（⇒ `-32003`）。

- ✅ **QwenPaw SKILL 交付（D-018）**：`skills/medme-ehr/`（instruction 型：SKILL.md + references/tool-contracts.md）。写「何时用哪个工具 / person_id 只能来自 list_persons / 透传不猜 / 错误码即行为指引」。
- ✅ 证据：`skill-contract-check.sh`（新增门禁）真进程 **13 门 PASS fails=0**（48 个字段名逐个存在于真实响应、6 工具名与 tools/list 双面对齐、-32001 实测、frontmatter 合规）；`mcp-e2e.sh` 15 门 fails=0；`cargo test --all-targets` 全绿（contract1+sha_gate1+mcp5）。
- 🔄 **下一步（阶段4 第二步②）**：MCP **写侧**工具（建档 / 传原件），使 agent 全流程不必绕 REST；随后可并入 T3「免维护接入块」。
- ⚠️ 未销账：SKILL 尚未注册进 `~/.qwenpaw/skill_pool`（真·agent 自动发现未验，留给 T3）；真·第三方 MCP 客户端互通未验（D-017③ 同类）。

- ✅ **MCP server 读侧 6 工具交付（D-017）**：`POST /mcp`（JSON-RPC 2.0，零新依赖，无 session）→ `list_persons` / `get_timeline` / `search_records` / `get_labs` / `list_documents` / `get_document_detail`。薄适配层复用 `vaults`，与 REST **同真相层**；`Cargo.toml` 依赖零新增（构建期零外网不变量未破）。
- ✅ 证据：`cargo test --all-targets` 全绿（contract 1 + sha_gate 1 + **mcp 5**）；`mcp-e2e.sh` 真进程 **15 门 PASS / fails=0**（protocolVersion=2025-06-18、tools=6、timeline=1、search 高亮命中 & 罕见词零假阳性、labs 非空、documents.sha 与 REST 逐字相等、4 类错误码、openapi_paths=16 且 `/mcp` 不入契约）。
- 🔄 **下一步（阶段4 第二步）**：①**QwenPaw SKILL**（工具契约已就绪，SKILL 写「何时用哪个工具 + person_id 怎么来」即可）；②MCP 写侧工具（建档 / 传原件）。
- ⚠️ 未销账：真·第三方 MCP 客户端互通未验（本机无可用 MCP client，与 D-016 浏览器渲染同类，代偿=协议层断言+双面一致性）。


- ✅ **阶段3 Web 管理端最小版达成（D-016）**：`ehr-api/web/index.html` 单文件零依赖 UI，编译期 `include_str!` 内嵌 → `GET /` 同源提供（免反代跨域）。功能=建档/选人/时间线/文档上传+详情/指标趋势/检索高亮。
- ✅ **待决项已销账：「Next 服务选型」= 不引入 Node**（本机 node/npm command not found；引 Node 会破坏 D-014 的构建期零外网 vendor 离线不变量）。换正式前端只需替 `ehr-api/web/`，API 契约 paths=16 不动。
- ✅ 证据：`cargo test --all-targets` 全绿（含**新增第4段守卫** `/` 必须 200+text/html+>2000B+含 `/api/v1`）→ `docker compose build` exit=0（`[证据] openapi_paths=16`）→ 起栈 e2e（`/` http=200 bytes=8591；建档 pid=e6ccc954；上传 sha=a4bc325e0215；timeline=1/docs=1/series=8/search「血常规」count=1）→ **`web-e2e.sh` fails=0**（含 JS 读用 20 个字段在真实响应里逐个存在）→ `down` 后 8585→000。
- ✅ 已 commit `506753a` 并 push `main`（D-015 通道）。
- ⚠️ **未销账项**：真·浏览器渲染（像素级）未验证——本机 playwright 镜像缺 npm 模块、QwenPaw Browser SDK 起 chromium 失败（X/VAAPI）。已用 `node --check`(JS_SYNTAX=OK) + 字段交叉核对 代偿，风险低，待可用无头浏览器再销。
- 🔄 **下一里程碑=阶段4 MCP server + QwenPaw SKILL**（前置条件已全部具备：16 路径 OpenAPI + 容器内 e2e 门 + Web 管理端）。
- ⚠️ 残留：volume `deploy_ehr-home` 内累计 **7 个人档**（历轮验收）；要干净基线 `docker volume rm deploy_ehr-home`。
- 🔄 挂账小项（同前）：a) 派生索引「HTTP触发式删除重建」是否入 API；b) 人数小规模下 cross-person search 逐 vault fan-out（<10 人免过度设计）。

- ✅ 阶段0完成（六假设销账，见 upstream-study.md）；**架构已获用户确认可自主推进**(21点对话)

- ✅ **阶段3 server 最小版达成（D-014）**：build exit=0 → `up -d`（boot Exited(0) + ehr-api Up，8585→8443）→ 容器内 e2e 全通（healthz 200 / persons 201 UUID / multipart 200 / timeline 回中文真日期 / openapi paths=16）→ `down` 干净收尾。依赖走宿主 vendor 离线，**构建期零外网**。
- ✅ 本轮改动仅 Dockerfile 一处：构建段 dry-run 探针去 `python3`（`rust:1-slim-bookworm` 无此解释器，exit 127）→ 纯 shell `grep -c` 计 paths + `-ge 16` 硬门；改前裸机 dry-run 同一表达式实测=16。
- ⚠️ D4 新变种已机制化：done 哨兵不许解析 log 尾文本，须写独立 `.rc` 退出码文件；Broken pipe 类次生报错要先看 exit 码再判因。
- 🔄 **阶段3 剩余 = Web 管理端最小版**（Dockerfile/compose 已就位，Web 侧待接；Next 服务选型未定）。
- 🔄 **阶段4 MCP server + QwenPaw SKILL** 已具备前置条件（16 路径 OpenAPI + e2e 门全绿）。
- 📌 目标验收②REST覆盖：容器内已复验，阶段2成果在容器形态下同样成立。
- ⚠️ 残留待清：容器 volume `deploy_ehr-home` 内存有前轮会话遗留 3 个人档（容器验收/冒烟·张三、容器验收·李四）；如需干净基线 `docker volume rm deploy_ehr-home`。
- SSH key(SHA256:pi+…xAKE)未绑 GitHub → fetch OK、push/PR 待用户绑定。

- ✅ 阶段0完成（六假设销账，见 upstream-study.md）；**架构已获用户确认可自主推进**(21点对话)
- ✅ **每小时自动推进任务已建**：cron `medme-ehr-hourly-push`(整点,silent,timeout≤1800s)——自检后台构建→按 PLAN 推进一步可验证小步→回写文档;里程碑完成需方向确认或真实阻塞时才汇报
- ⚠️ 教训(D4):**rustup 首轮安装被中断留下无 manifest 残破工具链**(详见 D-007b,误判的网络结论已作废)。干净重装解决(rustc 1.99.1)，crates 源=TUNA sparse。**D4新变种(D-010):setsid/nohup 后台 PATH 缺 ~/.cargo/bin → cargo not found、失败进程不 touch done 哨兵 → watcher 空转47min。准则:脚本调 cargo 前显式 export PATH="$HOME/.cargo/bin:$PATH";done 只在成功路径 touch 且 watcher 必须同时扫 log 错误行后自摘**
- ✅ **ehr-api 骨架验收 GREEN(2026-10-20,D-010)**：serve+curl e2e(healthz/persons中文建档/折叠list/stub 501/openapi.json=16 paths)+契约守卫测试(tests/contract.rs:CONTRACT↔router↔openapi三方交叉,此前"注释声称有测试实际缺失"已补账)+未知参数即拒(D4-b实战)+registry JSONL append-only真相层落盘核验；bin/lib 拆分完成(lib=路由与OpenAPI合成,bin=启动)
- ✅ **阶段 2 交付达成(2026-10-20,D-011)**：documents三件套(multipart上传/dedup幂等/detail/original字节级)、timeline挂真Vault per-person、search接FTS+jieba(高亮/q回显/稀有词零假阳性)、labs接parser::aggregate(docref证据回链+跨报告趋势+token级溯源红线)——ehtest.sh A~I **34门全绿fails=0**。两处遗留闭合:H2 typed-schema已在代码补齐并现场回显;G3真凶=ehtest探针漏传`API=`环境变量(**非**溯源红线违反，DBG复刻两次MISS=[]、「先怀疑测量方法」第三次应验)。新坑:**snippet()仅截~20窗不能当AND完整性证明面**(见D-011③)。git首提交+.gitignore已做(target ~5G排除)。
- 🔄 **下一里程碑=阶段 3 docker-compose(server最小版先行，web可后补)**。方向按既定PLAN无需再确认(D-009用户已认可自主推进)，两个待决小项挂账：a)派生索引「HTTP触发式删除重建」是否入API(MVP已有resilient open兜底,D-009删库POC实证);b)人数小规模下cross-person search直接逐vault fan-out(<10人免过度设计,upstream-study§4判据)。完成后目标验收②REST覆盖基本闭环→届时汇报请用户定Web端/MCP优先级。
- SSH key(SHA256:pi+…xAKE)未绑 GitHub → fetch OK、push/PR 待用户绑定。

## 待办队列（2026-10-20 用户下达，序号即优先级执行顺序，完成后打勾并记 DECISIONS）

- [x] T1 GitHub Actions CI：打 tag(v*) → 自动 build 多阶段镜像 → push ghcr.io/lbbit/medme_pro:<tag>（GH_TOKEN 权限/GCR 写入需验证；tag 触发规则、缓存策略写入 workflow 注释）—— **D-020：CI run success，镜像可 pull**
- [x] T2 双编排文件：`deploy/docker-compose.local.yml`(本地 build) + `deploy/docker-compose.ghcr.yml`(引用 ghcr 镜像,免构建一键起)；共用 .env 样例 —— **D-020：ghcr 编排端到端 GREEN**
- [ ] T3 部署/接入方式"免维护设计"：README 提供一键复制粘贴的 部署块 + **SKILL 接入块（MCP 不交付，见 D-018：skill=纯md+curl，Copilot CLI 实测，归档 repo 内随版本走）**（skill 文件放仓库固定路径、API base 用 env、SKILL 内不写死 token——升级不改配置才算达标）—— **D-021：已交付（README 部署块+接入块 / REST 优先 SKILL / agent-e2e 真 agent 兑现）**
- [x] T4 API token 鉴权（面向公网）：静态 Bearer token(env 注入,可多 token 按只读/读写分权),中间件层统一拦截,OpenAPI 标 securitySchemes；验收=无 token 401、对 token 全绿 —— **D-022：已交付（auth-e2e 21 门 PASS / fails=0；单测 7+5 门；Cargo.lock 零新依赖）**
- [ ] T5 Web 管理端：人员/时间线/上传/检索最小可用（技术栈选型时先报方案再动手）
- [x] T6 派生索引全量重建（目标验收④）：显式 `--rebuild-index` 命令 + 重建门禁 —— **D-023：已交付（rebuild-e2e 22 门 / tests/rebuild 3 门 / 零新依赖）**
- [x] T7 重建期写互斥（销账 D-023 未销账项①）：跨进程锁文件 + 服务面 503 拒写 + 并发重建被拒 + 陈旧锁回收 —— **D-024：已交付（rebuild-lock-e2e 15 门 PASS / fails=0；单测 2 门；Cargo.lock 零新依赖）**
- [x] T8 人类可读接口文档（目标验收⑤）：`docs/API.md` 由运行时 OpenAPI 自动生成 + 漂移门禁 —— **D-025：已交付（api-doc-check.sh 8 门 PASS / fails=0；tests/apidoc 3 门；单测 1 门；顺手修掉 labs 端点「文档说 501、实际 200」的漂移）**
- [x] T9 用药端点实装（`/persons/{pid}/medications` 从 stub→501 毕业）：与 labs 同源同判据的 `agg.meds` 面 + doc_type 如实分流 + 逐条证据回链 —— **D-026：已交付（meds-e2e 12 门 PASS / fails=0；tests/meds 2 门；Cargo.lock 零新依赖）**
- [x] T10 病程泳道端点实装（`/persons/{pid}/conditions` 从 stub→501 毕业）：第三个 aggregate 面 `agg.conditions` + 反凭空造门 + 顺手修掉暂存目录并发互删真 bug —— **D-027：已交付（conds-e2e 12 门 PASS / fails=0；tests/conds 4 门；45 门全绿；零新依赖）**

## 测试资源(2026-10-21 用户提供,已 gitignore 不入库)

- `docs/testres/llmtestapi.md` — LLM 网关(base_url/key/模型列表,实测 1p_deepseek-v4.1-flash 可用,回复带 reasoning_content)
- `docs/testres/检查单/lbb/` — 2 份真实体检 PDF(2020-09 15页/2021 21页,均有文本层,pdftotext 可抽)
- `docs/testres/检查单/zmf/` — ~15 张真实检查单照片(血常规/血脂/CT/MRI 等,OCR 需求样本)+1 份 AI 诊断参考 md
- 用途:T3 SKILL 验收 = Copilot CLI 拿真实检查单走"建档→上传→OCR/结构化→化验趋势→AI解读"全链路;
  PDF 文本层可直接测 ingest;图片样本检验 OCR feature 决策(开/关的实测依据)。
- ⚠️ 红线:testres 含个人健康隐私+key,任何日志/报告引用须脱敏;绝不出现在 git/容器镜像/公开渠道。

## 真实场景测试报告(2026-10-21,用户要求"贴近真实使用场景"后首轮)

### A. 真实体检PDF全链路(testres/lbb/2021_0907.pdf, 21页,1.9MB)
| 门 | 结果 |
|---|---|
| multipart上传→ingest | ✅ doc_type=surgery(分类不理想→见缺口①), text_indexed=true |
| 文本层抽取 | ✅ 11370字进库,search「总胆红素」FTS命中带高亮;「脂肪肝」等临床词命中 |
| CAS完整性 | ✅ 上传1,923,826B 回读sha256逐字节一致;kill -9 容器重启后再次回读仍一致 |
| labs聚合 | ❌ **series=0** —— PDF竖排「名称/缩写/值/区间/单位」列布局,上游extract_labs不识别(缺口①) |
| timeline日期 | ✅ 2021-09-07 从文件名解析;doc_type=surgery 而非体检报告(分类缺口②) |

### B. 图片检查单(zmf/血常规.jpg) 
- ❌ 501 "needs dicom/ocr worker (stage-3+)" —— 已知边界,如实拒绝而非假成功 ✅;OCR worker立项依据补实

### C. 容器韧性
- ✅ kill -9 → compose up 自愈 → persons/timeline/CAS 全部完好(事件溯源+CAS设计兑现)

### D. 全部既有门禁回归(容器+本机双跑)
- ✅ ehtest(A~I) / auth / entries / audit / rebuild-lock / web / mcp 全部 fails=0

### E. agent-e2e(Copilot CLI 真 agent 查档)
- ⚠️ exit=124 超时300s —— LiteLLM:4001 对长prompt间歇500/超时(Strata引擎忙,非skill/服务问题;
  直接curl LiteLLM 同样超时)。skill 本身语法/契约已由 contract-check 守卫;真agent验收待引擎空闲重跑

### 缺口登记(按影响排序)
1. **体检报告PDF列布局的labs抽取为0** —— 真实体检报告是核心输入之一;上游extract_labs面向化验单
   (横排「项目 值 范围 单位」),竖排列布局不识别。方案:a)LLM结构化抽取(extraction管线,testres
   网关可用)→作为extraction_added路径的真实用例; b)上游提PR支持列布局。倾向a。
2. doc_type分类:体检报告→surgery(上游classify词典缺「健康体检」类目)→随1的extraction修正或上游PR
3. OCR worker缺位使图片单501(如实拒绝✅但功能缺):若用户常用手机拍照上传,优先级应提前——请用户定

## 真实场景测试第二轮(2026-10-26 · OCR 容器跑通后)——图片检查单闭环 ✅

- OCR 容器(medme-ehr-ocr, trixie/glibc2.38, 298MB) up 于 8587；`/capabilities` 报 ocr_available:true + 7 种 ext
- **zmf 真实检查单照片 4 张全部 OCR 成功**:血常规 doc_type=lab_report、1781字文本层入库；血脂×3 同样全绿
- **labs 聚合闭环兑现**: 23→40 序列/41 点(含 flag H/L)；**跨年趋势首次成线**——总胆固醇 2025-08-05 5.9(H) → 2025-09-22 1.23(N)、低密度 3.9、甘油三酯 1.43，全部带 docref 溯源
- ehtest A~I 容器内复跑 fails=0（OCR 不破坏既有门禁）
- 缺口收窄：原「缺口① PDF列布局 labs=0」仍待 LLM extraction；「缺口③ OCR worker」**已销账**

## 真实场景测试第三轮(2026-10-26 · LLM extraction 端点实装+真网关全链路)——图片单→结构化→趋势 闭环 ✅

- **`PUT /persons/{pid}/documents/{sha}/extraction` 从 stub→501 毕业**(D-023 设计落地:deid schema v1 校验 + 字段级 evidence 闸 + append-only ExtractionAdded 事件 + result_json→CAS)
- **真网关(space-bunny,直出content)抽取真 OCR 文本**: labs=23 条全部进库,doc_date 原样保留 `2025/09/22`
- **evidence 闸真实工作**: 首跑 doc_date 写成 `2025-09-22`(LLM 规范化了)→unverified=7 精确点名;改 prompt 要求逐字复制后→unverified_fields=[] 且值正确
- **mode 语义实测**: image 模式验不过→unverified 标记(6个,charting-safety 设计内);text 模式→rejected=1(白细胞值 4.35 vs 原文不符则清字段)——两档行为都符合上游红线
- **labs 聚合吃上 extraction**: 白细胞 4.35(N) 从 extraction 进聚合,unverified:false
- 待销账: ①unverified_fields 列表为空但 unverified=6 的计数粒度(上游只有 top_field 进列表,内部字段只计数——如实记录);②体检 PDF 列布局同样可用此管线(labs=0 缺口对齐);③extraction 提示词进 SKILL.md

## T30 澄清与补强（2026-10-27，用户两点反馈）

### ① "本机无 node"表述澄清
用户 10-22 08:08 手动装了 node v20.18.0(`~/.local/bin/node`)。我此前的"本机无 node"判断
出自 D-016(10-20 12:15 Web 版落地时)——**当时确实没有**；W2 静默 SKIP 的窗口是
10-20~10-22 08:14(修复提交)。用户装 node 的动作与 CI 首跑在同一时间窗，两者共同促成修复。
此后门禁一律以 `command -v node` 实时探测，不再沿用历史判断。

### ② "CI 抓出 Web 跑不起来而我没发现"的根因与机制化补强(用户定性:严重问题)
根因链:W2 依赖 node→本机(当时)无 node→**静默 SKIP**(设计缺陷:跳过=不存在)→
页面 JS SyntaxError 数月无门能抓→仅 W1/W3(HTML 200+字段对齐)绿→假绿。
用户定性正确:这说明**没有 CI 时,本机"门禁全绿"不能保证基本功能可用**。
已落地(D-047,真 CI 首跑即抓出并修复):
- W2 缺依赖由 SKIP 改 **红**(门禁体系不再允许"因缺依赖假装通过")
- CI workflow 装 node(TUNA 通道 v20.18.0)
- 修复 Web JS 真缺陷(const t 重复声明),真 runner 复跑 18/18 绿
机制化三条(本轮新增,防复发):
1. **run-all-gates.sh 为唯一真跑入口**——人工"逐个跑"不再是有效证据;`--list` dry-run 随时核对登记表
2. **SKIP 必须附"谁负责补跑"**——container-smoke/agent-e2e 两门在 PLAN 当前状态区有对应补跑节拍
3. **浏览器级冒烟提上待办**:W2 只查 JS 语法,DOM 渲染/事件绑定仍无门——T5 Web 正式化时以
   headless 浏览器(container 内 chromium+playwright)补"页面真的能渲染出表格"的门
