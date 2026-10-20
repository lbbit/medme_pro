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
| 2 | person 层设计 + ehr-api 骨架（axum, OpenAPI）——**已完成** | /api/v1 MVP；curl版抽查对齐POC证据链(D-011:34门全绿)；读侧聚合面已毕业 labs(D-025)/medications(D-026)/conditions(D-027)；写侧手动条目已毕业 entries×3 + measurements(D-028) | ✅ GREEN(2026-10-24) |
| 3 | docker-compose（server+web）+ Web 管理端最小版 | 一键部署可用 | ✅ GREEN(2026-10-20，D-014/D-016) |
| 4 | MCP server + QwenPaw SKILL | agent 可按人查档/写档 | 🔄 MCP **读侧 9 + 写侧 2 = 11 工具已交付**（D-017/D-019/**D-029**）；SKILL REST 面已扩到 **15 命令**（含 D-028 四个新命令），注册进 skill_pool 待验 |
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

## 当前状态（2026-10-21 · **T12 MCP 侧补齐三个读侧工具（D-029）· MCP 工具数 8→11**）

- ✅ **T12 交付（MCP 侧补齐 `get_medications`/`get_conditions`/`get_measurements` 三个读侧工具 ⇒ 工具数 8→11）**：①**销账 D-028 未销账②**——`medications`/`conditions`/`measurements` 此前 **REST 独有**（SKILL.md 已如实标注），只吃 MCP 的宿主拿不到用药跨度/病程泳道/家庭自测；②**零新判据**：MCP 侧**直接调用** REST 那三个函数（`vaults::meds_aggregate`/`vaults::conditions_aggregate`/`entries::measurements_projection`），**不新建第二份取样/判据**（D-028 未销账②的原文要求）——硬证据不是「两边都有数据」，而是 **MCP 返回的 docref 与 REST 逐字相等**；③**400 语义对齐**：新增 `BAD_REQUEST ⇒ -32602` 映射（此前 400 掉进 `-32002`「稍后再试」⇒ 语义反了：参数非法该改参，不是重试）；④**分组不变量断言在正确形状上**：自测值本就出现在 labs 的**独立分组**（`self_measured=true`）⇒ 正确判据是「医院序列（`self_measured=false`）里不得出现自测 analyte_key」+「自测序列必带标志」；⑤**skill 层同步暴露**：SKILL.md（工具数 8→11 + REST↔MCP 对照表 3 行）+ `references/tool-contracts.md`（三个新工具字段契约行）。
- ✅ **证据链（缺一即不算交付）**：
  - ①**`mcp-e2e.sh` 真进程 29 门 PASS / fails=0**：`tools_count = 11`（9 读 + 2 写）⇒ `get_medications` 非空+docref（`True/True`）⇒ `get_conditions` 非空+docref（`True/True`）⇒ `get_measurements` 逐字读回（`1/self_measurement/True`）⇒ **`kind=note` ⇒ 0 条**（过滤器真进判据）⇒ **`kind=nonsense` ⇒ -32602** ⇒ 未知人 ⇒ **-32001** ⇒ 旧门禁（写侧闭环/幂等/坏 base64/-32003/openapi paths=16）全未破。
  - ②**`cargo test --all-targets` 全绿**：新增 `tests/mcp.rs` **2 门**（**MCP 与 REST 的 med_count/condition_count/count 与 docrefs 逐字相等** + 自测值分组不变量；缺参⇒-32602 / 未知人⇒-32001 / 非法 kind 与日期⇒-32602 / `kind=note` 真过滤）+ 旧门禁**未破** ⇒ 合计 **57 门全绿**。
  - ③**旧 e2e 门禁全绿未被破坏**：`ehtest.sh`(34 门) / `mcp-e2e.sh` / `auth-e2e.sh` / `entries-e2e.sh` / `conds-e2e.sh` / `meds-e2e.sh` / `rebuild-e2e.sh` / `rebuild-lock-e2e.sh` / `skill-rest-check.sh` / `skill-contract-check.sh` / `api-doc-check.sh` / `web-e2e.sh`（起真服务复验 fails=0）**全 fails=0**。
  - ④**零新依赖**：`ehr-api/Cargo.lock` 与 `Cargo.toml` **未改动**（依赖包数 **204** 不变，可验事实）。
  - ⑤**dry-run 优先**：先跑库级 `cargo test --all-targets`（不起服务、不落状态），再起真进程跑 mcp-e2e 与旧门禁复验。
  - ⑥**参数/状态生效有证据**：**工具数 8→11**（数字变化即客观证据）；`kind=note` ⇒ 0 而缺省 ⇒ 1；`kind=nonsense` ⇒ **-32602**；MCP 与 REST 的 docref **逐字相等**。
  - ⑦**新坑三条**（D4 同族，已记入 D-029）：①**「用某名字不存在」当探针的断言会随功能上线而过期**（`get_medications` 实装后旧探针从 -32601 变 -32602 ⇒ 真凶是探针过期；改用 `get_no_such_tool`）；②**判据形状错了会把正确行为当失败**（上游就是把自测值放在 labs 独立分组里）；③**jq 多值输出要 `tr ' ' '/'`**（三个新门首跑 fails=4 之三）；④**`Option+skip_serializing_if` 字段不能断言必存在**（`conditions[].onset`）。
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
