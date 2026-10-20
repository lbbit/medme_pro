# medme-ehr 阶段计划

> 长期项目。以 MedMe(Rust 内核, AGPL-3.0)为底座做多人数电子健康档案(EHR)后端服务。
> 每次开工先读「当前状态」。上游只读参考在 `upstream-medme/`。

## 目标验收标准

- [ ] docker-compose 一键起：EHR API + Web 管理端（+数据库如需要）
- [ ] REST API（OpenAPI 自动生成）覆盖：人员 CRUD、指定人员的健康事件/报告/指标查询、数据维护写入
- [ ] QwenPaw SKILL + MCP server 接入，AI agent 可按人查询并辅助诊断
- [ ] 原件(CAS)+事件日志是唯一真相；派生索引可全量重建（有重建脚本证明）
- [ ] docs/API.md 人类可读接口文档

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
| 2 | person 层设计 + ehr-api 骨架（axum, OpenAPI）——**已完成** | /api/v1 MVP；curl版抽查对齐POC证据链(D-011:34门全绿) | ✅ GREEN(2026-10-20) |
| 3 | docker-compose（server+web）+ Web 管理端最小版 | 一键部署可用 | ✅ GREEN(2026-10-20，D-014/D-016) |
| 4 | MCP server + QwenPaw SKILL | agent 可按人查档/写档 | 🔄 MCP server 读侧 6 工具已交付(D-017)；SKILL + 写侧待做 |
| 5 | 认证与多客户端（token/token scope），手机客户端预留 | 安全基线 | ⬜ |

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

## 当前状态（2026-10-20 · **阶段4 已开题：MCP server 读侧交付（D-017）**）

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

- [ ] T1 GitHub Actions CI：打 tag(v*) → 自动 build 多阶段镜像 → push ghcr.io/lbbit/medme_pro:<tag>（GH_TOKEN 权限/GCR 写入需验证；tag 触发规则、缓存策略写入 workflow 注释）
- [ ] T2 双编排文件：`deploy/docker-compose.local.yml`(本地 build) + `deploy/docker-compose.ghcr.yml`(引用 ghcr 镜像,免构建一键起)；共用 .env 样例
- [ ] T3 部署/接入方式"免维护设计"：README 提供一键复制粘贴的 部署块 + QwenPaw SKILL 接入块（skill 文件放固定路径、API base 用 env、SKILL 内不写死 token——升级不改配置才算达标）
- [ ] T4 API token 鉴权（面向公网）：静态 Bearer token(env 注入,可多 token 按只读/读写分权),中间件层统一拦截,OpenAPI 标 securitySchemes；验收=无 token 401、对 token 全绿
- [ ] T5 Web 管理端：人员/时间线/上传/检索最小可用（技术栈选型时先报方案再动手）
