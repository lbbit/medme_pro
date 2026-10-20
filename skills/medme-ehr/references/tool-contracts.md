# medme-ehr MCP 工具契约（读侧 10 + 写侧 2，共 12）

真源：`ehr-api/src/mcp.rs` 的 `tool_schemas()` 与 `call_tool()`。本文件是它的人类可读镜像；改动须两边同步。
传输：`POST /mcp`，JSON-RPC 2.0，`protocolVersion=2025-06-18`，无 session。

返回值一律 `result.content[0].text`（人读）与 `result.structuredContent`（机读）**双份同源**，由测试断言相等。工具级失败时 `result.isError=true`，并在 **`result._code`**（顶层，非规范字段，便于断言/改行为）里给出错误码。注意不是 `structuredContent._code`。
参数校验失败时，`result.content[0].text` 里带**具体原因**（例如 "`content_base64` is not valid base64: ..."），可据此改参数。

## 读侧参数与返回

| 工具 | 必填参数 | structuredContent 关键字段 |
|---|---|---|
| `list_persons` | — | `count`, `persons[]`：`person_id`/`name`/`birthday`/`gender`/`id_note` |
| `get_timeline` | `person_id` | `person_id`, `count`, `items[]`：`doc_date`/`doc_type`/`title`（`doc_date` 倒序，文档级） |
| `search_records` | `person_id`, `q`；可选 `limit`(1–100, 默认 20) | `person_id`, `q`, `count`, `items[]`：`title`/`snippet`（命中片段 `[词]` 高亮） |
| `get_labs` | `person_id` | `person_id`, `count`, `series[]`：`group_name`/`unit_canonical`/`any_abnormal`，`points[]`：`date`/`value`/`flag`(H/L)/`docref` |
| `get_medications` | `person_id` | `person_id`, `documents_scanned`, `med_count`, `mention_count`, `medications[]`：`name`/`raw_names`/`status`(恒 `active`，上游不推断停药)/`docrefs[]`/`mentions[]`：`date`/`dose`/`raw_name`/`docref`/`origin` |
| `get_conditions` | `person_id` | `person_id`, `documents_scanned`, `condition_count`, `conditions[]`：`raw_text`(原文写法，不做术语归一)/`onset`(**缺省即无该键**：来源文档无日期时无)/`icd_code`(**仅当原件印了**才有)/`docrefs[]` |
| `get_measurements` | `person_id`；可选 `kind`(`self_measurement`/`note`/`profile_event`)、`from`/`to`(`YYYY-MM-DD`，含端点) | `person_id`, `count`, `measurements[]`：`document_id`/`kind`/`taken_at`/`docref`/`payload_readable`/`values[]`(`analyte_key`/`value`/`unit`/`ref_low`/`ref_high`/`ref_source`/`flag`)/`text`/`event_kind`/`event_at`；**无参考区间时 `flag` 缺省**（不猜「正常」）；`payload_readable=false` 表示结构化载荷读不出来（原文仍在 `text` 里） |
| `get_audit` | —（全部人/全部动作）；可选 `person`(=person_id，别名 `person_id`)、`op` | `persons_scanned`, `count`, `entries[]`：`source`(`registry`=人员主数据 / `vault`=上游内核事件日志)/`op`(`create`\|`update`\|`delete` 或上游中文 `导入`\|`导出`\|`分享`\|`删除`)/`ts`/`person_id`/`detail`(可缺省)/`sha256`(**registry 侧缺省**：registry 事件无内容哈希) |
| `list_documents` | `person_id` | `person_id`, `count`, `items[]`：`content_hash`(=docref sha256)/`original_name`/`doc_type`/`doc_date`/`byte_size` |
| `get_document_detail` | `person_id`, `sha256`(64 位小写 hex) | 恒在：`content_hash`/`original_name`/`mime_type`/`byte_size`/`imported_at`/`document_id`/`doc_type`/`doc_date`/`doc_date_end`/`title`/`language`/`page_count`/`encounter_id`/`text_indexed`/`text_truncated_bytes`；**可选(缺省即无该键)**：`text`(无文本层时无)、`ocr_meta`、`extraction`(`{parseable, json?, raw?}`)；解析降级时 `parseable=false` 且内容留在 raw |

## 写侧参数与返回

| 工具 | 必填参数 | 可选参数 | structuredContent 关键字段 |
|---|---|---|---|
| `create_person` | `name`(非空) | `birthday`(`1988-03-02` 或 `1988`，宽松)、`gender`(原文透传)、`id_note`(就诊卡号等) | `id`(=person_id)/`name`/`birthday`/`gender`/`id_note`/`created_at`/`updated_at` |
| `upload_document` | `person_id`、`filename`(含扩展名)、`content_base64`(原件字节的**标准 base64**，可含换行) | — | `content_hash`(=docref sha256)/`deduped`(同字节重传即 true)/`document_id`/`doc_type`/`text_indexed`/`stored_raw_only`/`messages[]` |

审计口径（`get_audit`，D-031）：
- **与 REST `GET /audit` 同一 `audit_view()`**（同一个语义只允许一处实现）⇒ 同一份真相层、同一份过滤。
- `source=registry` = person registry 的 append-only **原始事件**（create/update/delete，**不折叠**）。
- `source=vault` = 上游 `Vault::audit_log()` 的投影（导入/导出/分享/删除）——**上游不展示**手动条目的合成导入，故「写了一条笔记」在 vault 侧**看不到行**（不造事件补齐）。
- 未知人过滤 ⇒ **0 条**（不是错误码：审计是全局视图，问「某人有哪些条目」答「没有」是合法答案）。

写侧约束：- **只支持 `.txt` / `.pdf`**；其他扩展名（如 `.dcm`）⇒ `-32003`（需 ocr/dicom worker，尚未落地）。PDF 若解析失败，原件仍入 CAS 永不丢（`stored_raw_only=true` + `messages[]` 说明）。
- 解码后单件上限 **32MB**；超出 ⇒ `-32602`。
- `upload_document` 走与 REST 上传**同一把「每人写锁」与同一条 ingest 路径** ⇒ 同人并发写不会互相踩。
- 幂等：同 person + 同字节重传 ⇒ `deduped=true`、`content_hash` 不变，**不重复建索引**。

## 错误码

| 码 | 含义 | agent 应对 |
|---|---|---|
| `-32602` | 参数非法（缺参/空串/坏 limit/坏 base64/超 32MB）——在任何 IO 之前拒绝 | 改参数后重试 |
| `-32601` | 未知工具名 / 未知方法 | 不要重试 |
| `-32001` | 查无此人（与 REST 404 语义对齐） | 换人或向用户确认 |
| `-32002` | 内部错误 | 告知用户稍后再试，别重试打转 |
| `-32003` | 不支持的文件类型（需 worker） | 换 txt/pdf，别原样重试 |
