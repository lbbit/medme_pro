# medme-ehr MCP 工具契约（读侧 6 + 写侧 2，共 8）

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
| `list_documents` | `person_id` | `person_id`, `count`, `items[]`：`content_hash`(=docref sha256)/`original_name`/`doc_type`/`doc_date`/`byte_size` |
| `get_document_detail` | `person_id`, `sha256`(64 位小写 hex) | 恒在：`content_hash`/`original_name`/`mime_type`/`byte_size`/`imported_at`/`document_id`/`doc_type`/`doc_date`/`doc_date_end`/`title`/`language`/`page_count`/`encounter_id`/`text_indexed`/`text_truncated_bytes`；**可选(缺省即无该键)**：`text`(无文本层时无)、`ocr_meta`、`extraction`(`{parseable, json?, raw?}`)；解析降级时 `parseable=false` 且内容留在 raw |

## 写侧参数与返回

| 工具 | 必填参数 | 可选参数 | structuredContent 关键字段 |
|---|---|---|---|
| `create_person` | `name`(非空) | `birthday`(`1988-03-02` 或 `1988`，宽松)、`gender`(原文透传)、`id_note`(就诊卡号等) | `id`(=person_id)/`name`/`birthday`/`gender`/`id_note`/`created_at`/`updated_at` |
| `upload_document` | `person_id`、`filename`(含扩展名)、`content_base64`(原件字节的**标准 base64**，可含换行) | — | `content_hash`(=docref sha256)/`deduped`(同字节重传即 true)/`document_id`/`doc_type`/`text_indexed`/`stored_raw_only`/`messages[]` |

写侧约束：
- **只支持 `.txt` / `.pdf`**；其他扩展名（如 `.dcm`）⇒ `-32003`（需 ocr/dicom worker，尚未落地）。PDF 若解析失败，原件仍入 CAS 永不丢（`stored_raw_only=true` + `messages[]` 说明）。
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
