# medme-ehr MCP 工具契约（读侧 6 个）

真源：`ehr-api/src/mcp.rs` 的 `tool_schemas()` 与 `call_tool()`。本文件是它的人类可读镜像；改动须两边同步。
传输：`POST /mcp`，JSON-RPC 2.0，`protocolVersion=2025-06-18`，无 session。

返回值一律 `result.content[0].text`（人读）与 `result.structuredContent`（机读）**双份同源**，由测试断言相等。工具级失败时 `result.isError=true`，并在 **`result._code`**（顶层，非规范字段，便于断言/改行为）里给出错误码。注意不是 `structuredContent._code`。

## 参数与返回

| 工具 | 必填参数 | structuredContent 关键字段 |
|---|---|---|
| `list_persons` | — | `count`, `persons[]`：`person_id`/`name`/`birthday`/`gender`/`id_note` |
| `get_timeline` | `person_id` | `person_id`, `count`, `items[]`：`doc_date`/`doc_type`/`title`（`doc_date` 倒序，文档级） |
| `search_records` | `person_id`, `q`；可选 `limit`(1–100, 默认 20) | `person_id`, `q`, `count`, `items[]`：`title`/`snippet`（命中片段 `[词]` 高亮） |
| `get_labs` | `person_id` | `person_id`, `count`, `series[]`：`group_name`/`unit_canonical`/`any_abnormal`，`points[]`：`date`/`value`/`flag`(H/L)/`docref` |
| `list_documents` | `person_id` | `person_id`, `count`, `items[]`：`content_hash`(=docref sha256)/`original_name`/`doc_type`/`doc_date`/`byte_size` |
| `get_document_detail` | `person_id`, `sha256`(64 位小写 hex) | 恒在：`content_hash`/`original_name`/`mime_type`/`byte_size`/`imported_at`/`document_id`/`doc_type`/`doc_date`/`doc_date_end`/`title`/`language`/`page_count`/`encounter_id`/`text_indexed`/`text_truncated_bytes`；**可选(缺省即无该键)**：`text`(无文本层时无)、`ocr_meta`、`extraction`(`{parseable, json?, raw?}`)；解析降级时 `parseable=false` 且内容留在 raw |

## 错误码

| 码 | 含义 | agent 应对 |
|---|---|---|
| `-32602` | 参数非法（缺参/空串/坏 limit）——在任何 IO 之前拒绝 | 改参数后重试 |
| `-32601` | 未知工具名 / 未知方法 | 不要重试 |
| `-32001` | 查无此人（与 REST 404 语义对齐） | 换人或向用户确认 |
| `-32002` | 内部错误 | 告知用户稍后再试，别重试打转 |

## 写侧

当前**无写侧工具**：建档/传原件只能走 REST（`POST /api/v1/persons`、`POST /api/v1/persons/{id}/documents`）。写侧 MCP 工具待阶段4 第二步②。