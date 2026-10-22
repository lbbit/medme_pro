---
name: medme-ehr
description: "查询与维护 medme-ehr 多人电子健康档案（ehr-api REST）：按人列时间线、检索病历原文、看检验指标趋势、读文档 OCR 文本；也可用建档/传原件写入档案。用于「张三的化验单怎么样」「他最近一次体检有什么异常」「查一下某人某项指标的变化」「把这份化验单存进某人的档案」这类问题。"
metadata:
  qwenpaw:
    emoji: "🗂️"
    requires: {}
---

# medme-ehr 档案工具集（REST 优先 · MCP 可选）

服务端是 `ehr-api` 的 REST API（OpenAPI 自动生成，17 路径 / 19 operation，base = `$EHR_API_BASE/api/v1`）。
> 这两个数字**由 `skill-contract-check.sh` 核对**（实现派生清单 `--print-rest-manifest` 为真源），不是手抄快照。
**本 skill 只需一个能执行 shell 的 agent**（Copilot CLI / codex / QwenPaw / 手机端脚本），零注册、零守护进程。

## 前置：先定人，再查档

服务端**只认 person_id（UUID），不认姓名**。所以标准起手式永远是：

1. 列出全部在档人：`ehr.sh persons` → 每人一条 `id`/`name`/`birthday`/`gender`/`id_note`
2. 用户说的名字与列表里的 `name` 对不上 ⇒ **先向用户确认是哪一位**，不要瞎猜 id
3. 列表里根本没这个人 ⇒ 明确告诉用户「该服务里没有这个人」，不要换 id 重试

## 命令速查（`ehr.sh` = scripts/ehr.sh，纯 curl，输出 JSON）

| 用户想干什么 | 命令 | 注意 |
|---|---|---|
| 服务活着吗 | `ehr.sh health` | `status:ok` |
| 「这个服务能收什么文件」 | `ehr.sh capabilities` | 能力清单**从实现派生**：`accepted_exts`（真能吃）/ `rejected_exts`（接不了+怎么开）；**上传前先看它**，别把 501 当故障 |
| 「有哪些人 / 找一下某人」 | `ehr.sh persons` | 永远的第一步 |
| 「什么时候做过什么检查」 | `ehr.sh timeline PID` | 文档级，`doc_date` 倒序，无日期件排最后 |
| 「报告里提到 X 吗」 | `ehr.sh search PID "白细胞"` | 中文全文检索，命中片段带 `[高亮]` |
| 「白细胞这段时间怎么变的」 | `ehr.sh labs PID` | 跨文档趋势；每点带 H/L 标记与 `docref` 证据 |
| 「他在吃什么药 / 剂量多少」 | `ehr.sh medications PID` | 用药跨度；`latest_dose` 为最近剂量，逐次 `mentions[].docref` 可溯原件 |
| 「他上传了哪些原件」 | `ehr.sh documents PID` | `content_hash` = 原件 sha256；它就是 `labs`/`medications`/`measurements` 里那个 `docref`（**`docref` 是那几个命令的键名，本命令的键是 `content_hash`**） |
| 「他自己在家量了血压/血糖？」 | `ehr.sh measurements PID [--kind self_measurement]` | 家庭自测值；**与医院指标永不同组**（`labs` 里 `self_measured=true` 一眼可辨）；区间带 `ref_source` 出处 |
| 「记一条今天头晕」 | `ehr.sh note PID "今天有点头晕"` | 纯文本笔记，原样落库；写后 `measurements --kind note` 可回读 |
| 「记一次病程发作」 | `ehr.sh profile-event PID --kind flare --package sle --at 2026-10-22` | `--at` 是**事件发生那天**（不是记录那天）；`--payload` 可带 JSON |
| 「把这次自测血压存进去」 | `ehr.sh self-measure PID --value bp_systolic=128 --value bp_diastolic=82 --unit mmHg` | 血压给两个 `--value`；生理学不可能的值 ⇒ 服务端 400 |
| 「把这份报告原文给我看」 | `ehr.sh detail PID SHA` | 含 OCR 文本层 / extraction |
| 「把这份报告的结构化结论写进档案」 | `ehr.sh extraction PID SHA result.json --mode text\|image` | **字段级 evidence 闸**：每条字段须是**原文逐字子串**（抄原文，别规范化日期/单位）；判假的字段被清空并标 `unverified`；**未经核验的模型输出进不了真相层** |
| 「下载原件」 | `ehr.sh original PID SHA` | CAS 按 hash 寻址，服务端每次核 sha256 |
| 「给某人建个档」 | `ehr.sh person-create "张三" --gender male --birthday 1990-01-02` | 返回 `id`；建档前先 `persons` 核对同名者 |
| 「把这张化验单存进档案」 | `ehr.sh upload PID 文件.txt` | 仅 .txt/.pdf；重传同字节 `deduped=true` |
| 「这个人档案里都发生过什么 / 谁动过」 | `ehr.sh audit [--person PID] [--op OP]` | **两层真相层**：`source=registry`（建档/改档/归档）与 `source=vault`（导入/删除）；`--op` 取 `create\|update\|delete` 或上游中文名 `导入\|导出\|分享\|删除` |

完整字段契约见 `references/rest-contracts.md`（真源 = 运行时 `GET /openapi.json`）。

## 字段承诺表（机器核对面 · 每个命令返回哪些键）

> 这张表是 **`SKILL.md` 自身的字段承诺**，由门禁 `skill-doc-field-check.sh` **逐命令**与
> **真进程真实响应**核对：① 表格承诺的字段必须真存在；② 真响应出现的键必须被表格提及；
> ③ 命令速查（散文）里提到的字段必须在本表同一命令行里。
> ⇒ 字段改了/删了 ⇒ 门禁红 ⇒ 文档不会悄悄变假（agent 照文档取字段取不到 ⇒ 数据全空）。
> 完整端点级契约见 `references/rest-contracts.md`（真源 = 运行时 `GET /openapi.json`）。

| 命令 | 字段 |
|---|---|
| `ehr.sh health` | `status` `home` |
| `ehr.sh capabilities` | `ocr_available` `ocr_engine` `routes[]`：`id` `name` `exts[]` `enabled` `reject_reason` `accepted_exts[]` `rejected_exts[]` `max_upload_bytes` |
| `ehr.sh persons` | `id` `name` `birthday` `gender` `id_note` `created_at` `updated_at` |
| `ehr.sh timeline PID` | `person_id` `count` `items[]`：`document_id` `doc_date` `doc_date_end` `doc_type` `title` |
| `ehr.sh search PID Q` | `person_id` `q` `count` `items[]`：`document_id` `title` `snippet` |
| `ehr.sh labs PID` | `person_id` `documents_scanned` `series_count` `total_points` `series[]`：`analyte_key` `group_name` `loinc` `unit_canonical` `values_converted` `any_abnormal` `self_measured` `ref_high` `ref_high_canonical` `points[]`：`date` `value` `unit` `value_canonical` `flag` `unverified` `docref` |
| `ehr.sh medications PID` | `person_id` `documents_scanned` `med_count` `mention_count` `medications[]`：`drug_key` `name` `raw_names` `atc` `latest_dose` `latest_raw_name` `status` `docrefs[]` `unverified` `mentions[]`：`date` `dose` `raw_name` `docref` `origin` `unverified` |
| `ehr.sh documents PID` | `person_id` `count` `items[]`：`document_id` `content_hash` `original_name` `mime_type` `byte_size` `doc_date` `doc_date_end` `doc_type` `title` `text_indexed` |
| `ehr.sh measurements PID` | `person_id` `count` `measurements[]`：`document_id` `kind` `taken_at` `title` `docref` `values[]`：`analyte_key` `value` `unit` `ref_high` `ref_source` `flag` `event_kind` `event_package` `event_at` `event_payload` `text` `payload_readable` |
| `ehr.sh detail PID SHA` | `content_hash` `original_name` `mime_type` `byte_size` `imported_at` `document_id` `doc_type` `doc_date` `doc_date_end` `title` `language` `page_count` `encounter_id` `text_indexed` `text_truncated_bytes`；可选键：`text` `ocr_meta[]`：`pages_with_text` `model_version` `backend` `extraction[]`：`parseable` `json`；可选键：`raw`（仅 `parseable=false` 时） |
| `ehr.sh extraction PID SHA FILE` | `content_hash` `document_id` `result_hash` `schema` `verified` `verify_detail[]`：`rejected` `unverified` `unverified_fields` `labs_count` `meds_count` `diagnoses_count` |
| `ehr.sh audit` | `persons_scanned` `count` `entries[]`：`source` `op` `ts` `person_id` `detail` `sha256` |
| `ehr.sh person-create NAME` | `id` `name` `birthday` `gender` `id_note` `created_at` `updated_at` |
| `ehr.sh upload PID FILE` | `content_hash` `deduped` `document_id` `text_indexed` `stored_raw_only` `messages[]` |
| `ehr.sh note PID TEXT` | `document_id` `content_hash` `doc_type` `title` `taken_at` `deduped` |
| `ehr.sh profile-event PID` | `document_id` `content_hash` `doc_type` `title` `taken_at` `deduped` |
| `ehr.sh self-measure PID` | `document_id` `content_hash` `doc_type` `title` `taken_at` `deduped` |
| `ehr.sh original PID SHA` | 原始字节（非 JSON） |


## 组合套路

- **「总结某人所有异常」**：`labs` 看 `flag`，再对异常项用 `search` 找原文佐证，报告里带 `docref`。
- **「最近一次体检说了什么」**：`timeline` 取第一条 → `detail` 读全文。
- **「跟去年比变化」**：用 `labs` 同 `unit_canonical` 的 series 逐点对比，别自己拿两份 text 做差值。
- **「这个药是哪儿开的」**：`medications` 拿 `mentions[].docref` → `detail PID SHA` 读原件全文 → 必要时 `original` 下字节核对。
- **「先建档再存化验单」**：`person-create` 拿 id → `upload` → `search` 回查确认写进去了。

## 硬规矩（来自服务端证据链，别绕过）

1. **透传不猜**：`text` / `extraction` 是原件层内容，原样引用（**两者都可能整个键不存在**——该文档没有文本层或未抽取，不是报错；`extraction.parseable=false` 时内容在 `raw` 里）。数值以 `labs` 的结构化 point 为准，**不要从 OCR 文本里抠数字当结论**。
2. **可溯源**：给结论时带上证据来源（`docref` / 文档日期），否则用户无法复核。
3. **检索零结果 ≠ 没病**：`search` 命中 0 时先换短词再试（`白细胞` 而非 `白细胞计数`），再下「查无此记录」结论。
4. **不是医疗器械**：本服务只做资料整理与检索，不给诊断结论、不建议用药。
5. **错误即指引**：400 参数非法（改参数重试）/ 401 缺或错 token（换 `EHR_API_TOKEN`）/ 403 只读 token 想写（换读写 token）/ 404 查无此人（换人或问用户）/ 501 该文件类型本构建接不了（换 .txt/.pdf，或查 `capabilities` 看怎么开）/ 500 内部错误（告知用户稍后再试）。别重试打转。
6. **写侧边界**：`upload` 能吃什么类型**由服务端能力清单决定**（`ehr.sh capabilities`）——默认构建真能吃的是 `.txt`/`.pdf`；图片类型（.jpg/.png/...）与 .dcm 被**如实拒**（不是故障，错误信息里写了怎么开）。原件永远进 CAS 永不丢，但 PDF 解析失败时只能存原件、建不了检索文本层（返回 `stored_raw_only=true`）。
7. **抽取结果必须逐字抄原文**：`extraction` 的每个字符串字段都要是**原文逐字子串**——把 `2025/09/22` 抄成 `2025-09-22`、把 `10^9/L` 抄成 `10^9 / L`，都会被 evidence 闸判假（字段被清空 / 标 `unverified`）。要「抄」不要「规范化」。

## 结构化抽取（LLM 回写）的调用要点

`extraction` 的输入是一段 **deid schema v1 JSON**（顶层字段 `labs`/`meds`/`diagnoses`，可带 `doc_date`/`impression`/`notes`）：

```json
{
  "labs": [
    { "name": "白细胞计数", "value": "6.2", "unit": "10^9/L", "ref_low": "3.5", "ref_high": "9.5", "flag": "" }
  ],
  "meds": [ { "name": "苯磺酸氨氯地平片", "dose": "5mg", "freq": "每日一次", "route": "口服" } ],
  "diagnoses": [ { "text": "高血压 2 级", "icd": "" } ]
}
```

- **逐字原则**：`name`/`value`/`unit`/`ref_low`/`ref_high`/`dose`/`freq`/`route`/`text`/`icd` 全部照抄原文（连标点与大小写）。
- **`mode` 必填**：`text`（文本档，严格：验不过就丢字段）/ `image`（图片档，容错：验不过标 `unverified` 保留）。
- **空值就是空**：抽不出来的字段**留空字符串**，不要猜、不要填「正常」。
- 回执看 `verified` 与 `verify_detail.rejected`/`unverified`：`verified=false` 说明有字段没通过逐字闸（多半是抄写了原文没有的内容），照 `unverified_fields` 改一版重交。
- 服务端**只存校验后的结果** ⇒ 你的伪造字段进不了真相层；`labs` 聚合会优先吃这份 extraction。

## 零维护要点（升级不改配置）

- API 地址来自环境变量 `EHR_API_BASE`（默认 `http://127.0.0.1:8585`），**skill 内不写死地址**。
- token 来自 `EHR_API_TOKEN`（T4 已落地：服务端会校验 `Authorization: Bearer`）。**skill 内不写死 token**。
  - 服务端未配置 token（open 模式）时，不带 header 也能用。
  - 收到 **401** = 缺/错 token ⇒ 换 `EHR_API_TOKEN`（**不要**重试打转）。
  - 收到 **403** = token 是只读(ro) ⇒ 需要读写 token 才能建档/上传/写侧 MCP 工具。
- 本 skill 目录随仓库版本走：`skills/medme-ehr/`（SKILL.md + references/ + scripts/），复制即用。

## 可选：MCP 壳

服务端另有 `POST /mcp`（JSON-RPC 2.0，13 个工具：读侧 11 + 写侧 2），供只吃 MCP 的宿主使用。
工具清单的**真源是实现**（`ehr-api --print-mcp-tools` 干跑输出 JSON）；本文档里的数字由门禁 `skill-contract-check.sh` 逐项核对（文档与实现不一致 ⇒ 门禁红）。
契约见 `references/tool-contracts.md`。**宿主不支持 MCP 时直接用上面的 REST 命令即可，两者同源。**

REST 命令 ↔ MCP 工具一一对应（同一真相层，任选其一）：

| REST（`ehr.sh`） | MCP 工具 | 读/写 |
|---|---|---|
| `persons` | `list_persons` | 读 |
| `timeline PID` | `get_timeline` | 读 |
| `search PID Q` | `search_records` | 读 |
| `labs PID` | `get_labs` | 读 |
| `medications PID` | `get_medications` | 读 |
| `conditions PID` | `get_conditions` | 读 |
| `measurements PID` | `get_measurements` | 读 |
| `documents PID` | `list_documents` | 读 |
| `detail PID SHA` | `get_document_detail` | 读 |
| `audit [--person PID] [--op OP]` | `get_audit` | 读 |
| `capabilities` | `get_capabilities` | 读 |
| `person-create NAME` | `create_person` | 写 |
| `upload PID FILE` | `upload_document` | 写 |

MCP 错误码：`-32602` 参数非法 / `-32601` 未知工具 / `-32001` 查无此人 / `-32002` 内部错误 / `-32003` 文件类型不支持。

MCP 另有 **resources 面**（只读上下文，按 URI 定位；`resources/list` / `resources/templates/list` / `resources/read`）：
内容与上表的**读侧工具同源同值**（`ehr://persons/{person_id}/labs` ≡ `get_labs`）⇒ 宿主想「把某人的某项指标当上下文挂载」时用 resources 面即可。
URI 清单的真源是实现（`ehr-api --print-mcp-resources`），由门禁 `mcp-resources-e2e.sh` 核对。
resources 错误码：`-32005` 未知资源 URI（改 URI）/ `-32001` 查无此人（换人）/ `-32602` 缺 `uri` 参数（改参数）。
