# medme-ehr MCP 工具契约（读侧 11 + 写侧 2，共 13）

真源：`ehr-api/src/mcp.rs` 的 `tool_schemas()` 与 `call_tool()`。本文件是它的人类可读镜像；改动须两边同步。
**工具清单的真源是实现**：`ehr-api --print-mcp-tools`（干跑，stdout 为 JSON）输出
`{count,read,write,names[],read_names[],write_names[]}`——本文档标题里的「共 13 / 读侧 11 / 写侧 2」
由 `skill-contract-check.sh` 与该输出**逐项核对**（不一致 ⇒ 门禁红）。写侧工具名单**只此一处**
（`mcp::WRITE_TOOLS`），鉴权分权（ro/rw）也读它 ⇒ 「公布清单」与「谁能写」不会漂移。
传输：`POST /mcp`，JSON-RPC 2.0，`protocolVersion=2025-06-18`，无 session。

返回值一律 `result.content[0].text`（人读）与 `result.structuredContent`（机读）**双份同源**，由测试断言相等。工具级失败时 `result.isError=true`，并在 **`result._code`**（顶层，非规范字段，便于断言/改行为）里给出错误码。注意不是 `structuredContent._code`。
参数校验失败时，`result.content[0].text` 里带**具体原因**（例如 "`content_base64` is not valid base64: ..."），可据此改参数。

## 读侧参数与返回

| 工具 | 必填参数 | structuredContent 关键字段 |
|---|---|---|
| `list_persons` | — | `count`, `persons[]`：`id`(=person_id)/`name`/`birthday`/`gender`/`id_note`/`created_at`/`updated_at` |
| `get_timeline` | `person_id` | `person_id`, `count`, `items[]`：`document_id`/`doc_date`/`doc_type`/`title`；可选键：`doc_date_end`（`doc_date` 倒序，文档级） |
| `search_records` | `person_id`, `q`；可选 `limit`(1–100, 默认 20) | `person_id`, `q`, `count`, `items[]`：`document_id`/`title`/`snippet`（命中片段 `[词]` 高亮） |
| `get_labs` | `person_id` | `person_id`, `documents_scanned`, `series_count`, `total_points`, `series[]`：`analyte_key`/`group_name`/`loinc`/`unit_canonical`/`values_converted`/`any_abnormal`/`self_measured`/`ref_high`/`ref_high_canonical`，`points[]`：`date`/`value`/`unit`/`value_canonical`/`flag`(H/L)/`unverified`/`docref` |
| `get_medications` | `person_id` | `person_id`, `documents_scanned`, `med_count`, `mention_count`, `medications[]`：`drug_key`/`name`/`raw_names`/`atc`/`latest_dose`/`latest_raw_name`/`status`(恒 `active`，上游不推断停药)/`unverified`/`docrefs[]`/`mentions[]`：`date`/`dose`/`raw_name`/`docref`/`origin` |
| `get_conditions` | `person_id` | `person_id`, `documents_scanned`, `condition_count`, `conditions[]`：`raw_text`(原文写法，不做术语归一)/`onset`(**缺省即无该键**：来源文档无日期时无)/`icd_code`(**仅当原件印了**才有)/`docrefs[]` |
| `get_measurements` | `person_id`；可选 `kind`(`self_measurement`/`note`/`profile_event`)、`from`/`to`(`YYYY-MM-DD`，含端点) | `person_id`, `count`, `measurements[]`：`document_id`/`kind`/`taken_at`/`title`/`docref`/`payload_readable`/`values[]`(`analyte_key`/`value`/`unit`/`ref_high`/`ref_source`/`flag`)/`text`/`event_kind`/`event_package`/`event_at`；**无参考区间时 `flag` 缺省**（不猜「正常」）；**无区间下限时 `ref_low` 缺省**；`payload_readable=false` 表示结构化载荷读不出来（原文仍在 `text` 里） |
| `get_audit` | —（全部人/全部动作）；可选 `person`(=person_id，别名 `person_id`)、`op` | `persons_scanned`, `count`, `entries[]`：`source`(`registry`=人员主数据 / `vault`=上游内核事件日志)/`op`(`create`\|`update`\|`delete` 或上游中文 `导入`\|`导出`\|`分享`\|`删除`)/`ts`/`person_id`/`detail`(可缺省)/`sha256`(**registry 侧缺省**：registry 事件无内容哈希) |
| `get_capabilities` | —（无参） | `ocr_available`(bool：本二进制**链了** OCR 引擎)/`ocr_engine`(null 或引擎名)/`routes[]`：`id`/`name`/`exts[]`/`enabled`/`reject_reason`(`enabled=true` 时为 null)/`accepted_exts[]`(**本构建真能吃**)/`rejected_exts[]`(**接不了**，原因在 `routes[]` 的 `reject_reason` 里) |
| `list_documents` | `person_id` | `person_id`, `count`, `items[]`：`document_id`/`content_hash`(=docref sha256)/`original_name`/`mime_type`/`byte_size`/`doc_type`/`doc_date`/`title`/`text_indexed`；可选键：`doc_date_end` |
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

## resources 面（读侧只读上下文）

MCP 的**第二个协议面**：tools 是「动作」，resources 是「某个东西本身」（由 URI 直接定位）。
- 传输同 tools：`POST /mcp`，JSON-RPC 2.0。方法：`resources/list` / `resources/templates/list` / `resources/read`。
- `initialize` 的 `capabilities` 现在**宣告 `resources`**（`subscribe:false` / `listChanged:false`）。
- **只读**：resources 一律不改真相层 ⇒ ro token 也能读（与读侧工具同权限口径）。
- **同源**：每个 resource 的内容由**与 tools 面完全相同**的 payload builder 产出 ⇒
  「一个语义只允许一处实现」（D2）⇒ resources 面**不可能**与 REST 面分叉。
- 成功响应形状：`result.contents[0] = {uri, mimeType:"application/json", text:<JSON 文本>}`；
  **失败走 JSON-RPC error 信封**：`{"error":{"code":<码>,"message":"…"}}`（**不带** `isError`）
  —— 与 tools 面的错误信封**不同**，各有各的规范依据：
  - `tools/call` 的失败是**工具执行失败** ⇒ MCP 规范用 `result.isError=true` + `result._code`；
  - `resources/read` 的失败是**协议级错误** ⇒ 规范用 JSON-RPC `error` 对象。
  ⚠️ 这条不是洁癖：实测（T35）若把 resources/read 的失败回成 `isError` 形状，
  **官方 MCP SDK 会崩**（它把响应当 ReadResourceResult 校验，缺 `contents`），
  宿主 agent 拿不到错误码 ⇒ 错误码白设计。已由 `mcp-interop-e2e.sh`（I6）与
  `ehr-api/tests/mcp.rs::mcp_resources_read_failures_use_jsonrpc_error_envelope` 双门守住。
  ⚠️ 更进一步（T36）：**两个官方实现各自独立地拒收了它** —— Python SDK 抛 `ValidationError`，
  TypeScript SDK 抛 `$ZodError` ⇒ 「官方 SDK 能吃」已从**一个实现**的结论升级为
  **两个独立解析器**的结论（`mcp-interop-e2e.sh` / `mcp-interop-ts-e2e.sh` 双门守住）。
  ⇒ 纪律：协议面的第三方验收须覆盖 **≥2 个官方实现**。

**URI 清单的真源是实现**：`ehr-api --print-mcp-resources`（干跑，stdout 为 JSON）输出
`{count, templates, uris[], template_uris[]}`。下表由 `mcp-resources-e2e.sh` 的 R7 判据
**与该输出逐项核对**（不一致 ⇒ 门禁红）——不是手抄快照。

| resource URI | 等价工具（同一份 payload） |
|---|---|
| `ehr://persons` | `list_persons` |
| `ehr://persons/{person_id}` | —（人员档案主数据，无对应工具） |
| `ehr://persons/{person_id}/timeline` | `get_timeline` |
| `ehr://persons/{person_id}/labs` | `get_labs` |
| `ehr://persons/{person_id}/medications` | `get_medications` |
| `ehr://persons/{person_id}/conditions` | `get_conditions` |
| `ehr://persons/{person_id}/measurements` | `get_measurements` |
| `ehr://persons/{person_id}/documents` | `list_documents` |
| `ehr://persons/{person_id}/documents/{sha256}` | `get_document_detail` |
| `ehr://audit` | `get_audit` |
| `ehr://capabilities` | `get_capabilities` |

`resources/list` 只列**当前可枚举的具体资源**（`ehr://persons` / `ehr://audit` / `ehr://capabilities`），
按人定位的那几个在 `resources/templates/list` 里以**模板**公布（`{person_id}` / `{sha256}` 占位）。

## 错误码

| 码 | 含义 | agent 应对 |
|---|---|---|
| `-32602` | 参数非法（缺参/空串/坏 limit/坏 base64/超 32MB）——在任何 IO 之前拒绝 | 改参数后重试 |
| `-32601` | 未知工具名 / 未知方法 | 不要重试 |
| `-32001` | 查无此人（与 REST 404 语义对齐） | 换人或向用户确认 |
| `-32002` | 内部错误 | 告知用户稍后再试，别重试打转 |
| `-32003` | 不支持的文件类型（需 worker） | 换 txt/pdf，别原样重试 |
| `-32005` | **未知资源 URI**（形状不对/种类不存在）——与「查无此人」(`-32001`) 区分 | 改 URI，不要重试 |
