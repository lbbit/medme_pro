# medme-ehr REST 契约（17 路径 / 19 operation · OpenAPI 自动生成）

真源：`ehr-api` 运行时 `GET /openapi.json`（现合成，paths=17 / operations=19）。本文件是人类可读镜像；改动须两边同步。
> T16：这两个数字**由门禁核对**，不靠人记——`skill-contract-check.sh` 从 `ehr-api --print-rest-manifest`（实现派生清单）取值，
> 与本文件、`SKILL.md` 里写的数字逐项比对；漂移即红。实现侧加路由时**不必**手工同步这两处数字。
> 另：完整路径**清单**（`METHOD path` 逐条）可用 `ehr-api --print-rest-manifest` 的 `paths_list` 直接取。
Base：`$EHR_API_BASE/api/v1`（默认 `http://127.0.0.1:8585/api/v1`）。
鉴权：T4 已落地。服务端配置 token 后，所有路径（除 `/healthz`、`/openapi.json`、Web 首页）都要求 `Authorization: Bearer <token>`。
- `EHR_API_TOKEN`：单 token（等价读写权限）。
- 服务端多 token/只读分权：请求头里的 token 决定权限；只读 token 遇写操作 ⇒ **403**（不是 401）。
- helper 侧只需设 `EHR_API_TOKEN`；未配置 token 的服务端（open 模式）不带头也能通。

## 路径与关键返回字段

| 方法+路径 | 作用 | 返回关键字段 |
|---|---|---|
| `GET /healthz` | 健康检查 | `status`(`ok`/`no-home`)、`home` |
| `GET /capabilities` | **ingest 能力清单**（D-034；从实现派生，非手抄）：本构建真能吃哪些文件类型、哪种接不了、怎么开 | `ocr_available`(bool)/`ocr_engine`(null 或引擎名)/`routes[]`：`id`/`name`/`exts[]`/`enabled`/`reject_reason`/`accepted_exts[]`/`rejected_exts[]` |
| `GET /persons` | 人员列表 | 数组：`id`/`name`/`birthday`/`gender`/`id_note`/`created_at`/`updated_at` |
| `POST /persons` | 建档（body: `name` 必填；`birthday`/`gender`/`id_note` 选填） | 201 + 同上字段（`id` = person_id） |
| `GET /persons/{pid}/timeline` | 健康事件时间线（文档级，`doc_date` 倒序） | `person_id`/`count`/`items[]`：`document_id`/`doc_date`/`doc_date_end`/`doc_type`/`title` |
| `GET /persons/{pid}/search?q=&limit=` | 中文全文检索（FTS5+jieba），命中片段带 `[高亮]` | `person_id`/`q`/`count`/`items[]`：`title`/`snippet` |
| `GET /persons/{pid}/labs` | 检验指标趋势（跨文档聚合） | `person_id`/`documents_scanned`/`series_count`/`total_points`/`series[]`：`analyte_key`/`group_name`/`loinc`/`unit_canonical`/`values_converted`/`any_abnormal`/`points[]`：`date`/`value`/`unit`/`value_canonical`/`flag`(H/L)/`unverified`/`docref` |
| `GET /persons/{pid}/documents` | 文档列表 | `person_id`/`count`/`items[]`：`document_id`/`content_hash`/`original_name`/`mime_type`/`byte_size`/`doc_date`/`doc_type`/`title`/`text_indexed` |
| `GET /persons/{pid}/documents/{sha}` | 文档详情（OCR 文本层 + extraction） | `content_hash`/`original_name`/`mime_type`/`byte_size`/`imported_at`/`document_id`/`doc_type`/`doc_date`/`title`/`language`/`page_count`/`text_indexed`/`text_truncated_bytes`；可选键：`text`、`ocr_meta`、`extraction`(`parseable`/`json`/`raw`) |
| `GET /persons/{pid}/documents/{sha}/original` | 原件字节流（CAS 寻址，服务端每次核 sha256） | 原始字节（非 JSON） |
| `POST /persons/{pid}/documents` | 上传原件（multipart，字段名 `file`） | `content_hash`/`deduped`/`document_id`/`doc_type`/`text_indexed`/`stored_raw_only`/`messages[]` |
| `GET /persons/{pid}/medications` | 用药跨度（跨文档聚合；**只从处方类原件抽药**） | `person_id`/`documents_scanned`/`med_count`/`mention_count`/`medications[]`：`drug_key`/`name`/`raw_names`/`atc`/`latest_dose`/`latest_raw_name`/`start`/`end`/`status`(恒 `active`=不推断停药)/`docrefs[]`/`unverified`/`mentions[]`：`date`/`dose`/`raw_name`/`docref`/`origin`(`regex`/`llm`)/`unverified` |
| `GET /persons/{pid}/conditions` | 病程泳道（与 labs/medications 同一批文档、同一次聚合的第三块输出） | `person_id`/`documents_scanned`/`condition_count`/`conditions[]`：诊断原文/`icd_code`/`onset`/`docrefs[]` |
| `GET /persons/{pid}/measurements` | 手动条目（自测值/笔记/动作事件）投影，可 `?kind=&from=&to=` 过滤 | `person_id`/`count`/`measurements[]`：`document_id`/`kind`/`taken_at`/`title`/`docref`/`values[]`(`analyte_key`/`value`/`unit`/`ref_low`/`ref_high`/`ref_source`/`flag`)/`event_kind`/`event_package`/`event_at`/`event_payload`/`text`/`payload_readable` |
| `POST /persons/{pid}/entries/self-measurement` | 写一条自测记录（body: `values[]`=`analyte_key`/`value`/`unit`；`taken_at` 可选） | 201 + `document_id`/`content_hash`/`doc_type`/`title`/`taken_at`/`deduped`；不可能的值 ⇒ **400**（带原因） |
| `POST /persons/{pid}/entries/note` | 写一条纯文本笔记（body: `text`；`taken_at` 可选） | 201 + 同上回执 |
| `POST /persons/{pid}/entries/profile-event` | 写一条病程动作日志（body: `kind`/`package`/`at`(YYYY-MM-DD 事件日)/`payload` 对象） | 201 + 同上回执 |
| `PUT /documents/{sha}/extraction` | 尚未实装 | **501** |
| `GET /audit` | 审计视图（**两层真相层**，口径分层不假装只有一层）：`source=registry`（人员主数据：建档/改档/归档，**不折叠**）+ `source=vault`（上游内核事件日志：导入/导出/分享/删除，**复用上游 `audit_log()` 判据**）。可 `?person=&op=` 过滤 | `persons_scanned`/`count`/`entries[]`：`source`/`op`/`ts`/`person_id`/`detail`/`sha256`（registry 侧无 sha256；vault 侧为原件内容哈希） |
| `GET /` | Web 管理端最小版（同源单文件 HTML） | HTML |
| `GET /openapi.json` | OpenAPI 文档 | paths=17 / operations=19 |
| `POST /mcp` | MCP JSON-RPC 端点（可选壳，见 tool-contracts.md） | JSON-RPC |

## 错误语义

| 状态 | 含义 | 应对 |
|---|---|---|
| 400 | 参数非法（缺 `q`、坏 `limit`、坏 sha、空上传、未知扩展名之外的多文件、**不可能的自测值**、坏 `kind`/日期） | 改参数后重试 |
| 401 | 缺/错 token | 换 `EHR_API_TOKEN`（别重试打转） |
| 404 | 查无此人 / 无该文档 | 换人或向用户确认 |
| 501 | 该扩展名本构建接不了（图片需 OCR 引擎、`.dcm` 需 dicom worker——**先查 `GET /capabilities` 的 `rejected_exts`**，里面有 `reject_reason` 写明怎么开），或该路径尚未实装（现仅 `extraction`） | 换 `accepted_exts` 里的类型，或按 `reject_reason` 指示开启对应能力 |
| 500 | 内部错误（含 CAS 完整性不符 = 投毒告警） | 告知用户稍后再试 |

## 不变量（读数据时必须遵守）

1. **数值只认 `labs` 的结构化 point**，不要从 OCR `text` 里抠数字当结论；`text`/`extraction` 是原件层，**原样引用**。
2. **可溯源**：结论须带 `docref`（= `content_hash` = 原件 sha256），用户才能复核；`docref` 可直接走 `documents/{sha}` 与 `.../original`。
3. **检索 0 命中 ≠ 没病**：先用短词（`白细胞`）再试长词（`白细胞计数`），长词可能 0 命中。
4. `text` / `ocr_meta` / `extraction` 是**可选键**（无文本层时整个键不存在，不是空值）；`extraction.parseable=false` 时内容在 `raw` 里。
5. `stored_raw_only=true` ⇒ 原件已入 CAS 永不丢，但**尚无检索文本层**（PDF 解析失败），此时 `search`/`labs` 查不到它。
6. 本服务只做资料整理与检索，**不提供诊断结论**、不建议用药（不是医疗器械）。

7. **家庭自测值 ≠ 医院指标**：自测值在 `measurements` 里（`kind=self_measurement`），在 `labs` 里是**独立分组**（`self_measured=true`），**永不与门诊值画成同一条线**；给结论时说明它是家庭自测。
8. **自测区间有出处**：`values[].ref_source` 是上游写明的区间来源；**无区间时 `flag` 恒缺省**（不猜「正常」），`ref_low`/`ref_high` 可能整个键不存在。
