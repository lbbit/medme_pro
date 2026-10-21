# medme-ehr · API 接口文档（人类可读）

> ⚠️ **本文件是自动生成的，请勿手改。**
> 唯一事实源 = 服务运行时 OpenAPI（`GET /openapi.json`，服务端每次现合成）。
> 重新生成：`EHR_HOME=<dir> ehr-api --print-api-doc > docs/API.md`；
> 漂移门禁：`./api-doc-check.sh`（断言仓库内本文件与生成物**逐字一致**）。

## 1. 概览

- 标题：medme-ehr API
- 版本：`0.0.1`
- 许可：AGPL-3.0-only
- 说明：多人电子健康档案服务。真相层=原件CAS+事件日志；派生索引可重建。设计稿见 docs/API-DESIGN.md
- 根路径：所有业务端点都在 `/api/v1` 前缀下；`/openapi.json`（本机文档）、`/mcp`（MCP 协议面）、`/`（Web 最小版）不在该前缀内。
- 数据真相：**原件 CAS + 事件日志**是唯一真相；派生索引（SQLite/FTS）可随时重建（`ehr-api --rebuild-index`）。

## 2. 认证

- 方案：`http` / `bearer`（OpenAPI securitySchemes: `bearerAuth`）
- 说明：静态 Bearer token（env 注入）。scope: ro=只读 / rw=读写。EHR_API_TOKENS 逗号分隔，格式 <token>[:ro|rw] 或 <name>=<token>[:ro|rw]。
- 生效方式：请求头 `Authorization: Bearer <token>`。
- scope：`ro` 只读 / `rw` 读写；只读 token 调写侧 ⇒ **403**（HTTP）或 JSON-RPC **`-32004`**（MCP）。
- 未配置 token ⇒ **open 模式**（全部放行），启动 `[证据]` 行会显式打印 `auth=open(tokens=0) ⚠️`。

## 3. 状态码与错误约定

| 码 | 含义 | 客户端动作 |
|---|---|---|
| 200 | 成功 | — |
| 201 | 已创建（POST /persons） | — |
| 400 | 参数非法（缺 `q`、`limit` 非法、空 `name`、坏 JSON） | 改参数再试 |
| 401 | 缺 token / token 错 | 取或换 token |
| 403 | token 有效但 scope 不足（只读 token 想写） | 换读写 token |
| 404 | 人/文档不存在（或已归档） | 检查 `pid` / `sha` |
| 413 | 上传超出大小上限 | 压缩或分片（当前不支持） |
| 415 | 不支持的类型（`.dcm` 等，OCR/DICOM worker 未做） | 换格式 |
| 500 | 内部错误 | 看服务端日志 |
| 501 | **该端点尚未实装（stub）** | 用已实装端点或等待实现 |
| 503 | 派生索引正在重建中（响应带 `Retry-After: 5`） | 等待后重试 |

错误响应体统一为 `{"error": "<原因>"}`。

## 4. 端点一览

| 方法 | 路径 | 说明 | 状态 |
|---|---|---|---|
| `GET` | `/api/v1/audit` | 审计视图：两层真相层合并（**口径分层**，不假装只有一层）。 - `source="registry"`：person registry 的 append-only 原始事件（create/update/delete，**不折叠**）。 - `source="vault"`：上游 `Vault::audit_log()` 的投影（导入/导出/分享/删除）——**复用上游判据，不另写一份**。 参数 `person`/`op` 是**真过滤**（真进判据，不是摆设）。 | ✅ 已实装 |
| `GET` | `/api/v1/capabilities` | **ingest 能力清单**（D-034）：本构建**能吃哪些文件类型**、哪种类型走哪条路、 哪种能力**没链接进来**（默认构建没开 OCR ⇒ 图片被如实拒）。 | ✅ 已实装 |
| `PUT` | `/api/v1/documents/{sha}/extraction` | AI agent 结构化发现(引用不替代，字段级 evidence 锚点)（stub→501） | ⚠️ stub（501） |
| `GET` | `/api/v1/healthz` | 健康检查 | ✅ 已实装 |
| `GET` | `/api/v1/persons` | 人员列表（读 registry/persons.jsonl 全量折叠；MVP 规模足够，规模化后换派生索引——真相层不变） | ✅ 已实装 |
| `POST` | `/api/v1/persons` | 建档 —— person registry 是本项目唯一新增真相层（append-only JSONL，与内核事件日志同哲学） | ✅ 已实装 |
| `GET` | `/api/v1/persons/{pid}/conditions` | 病程泳道——与 labs/medications 同一批文档、同一次 parser::aggregate 的第三块输出（D-027）。 诊断原文**原样透传不归一**（上游无术语归一字典），ICD 码仅当原件印了才有； 证据回链 docref（CAS sha）可一路溯源到原件字节（D2）。 | ✅ 已实装 |
| `GET` | `/api/v1/persons/{pid}/documents` | 文档列表（documents.json 折叠视图，docref=sha256）。timeline 的扩展列版。 | ✅ 已实装 |
| `POST` | `/api/v1/persons/{pid}/documents` | MVP 直传同步入库(锁=每人串行占位)；OCR/DICOM worker 化后置。见 vaults::ingest_upload。 | ✅ 已实装 |
| `GET` | `/api/v1/persons/{pid}/documents/{sha}` | GET /persons/{pid}/documents/{sha} —— document + OCR文本层 + extraction JSON(API-DESIGN)。 | ✅ 已实装 |
| `GET` | `/api/v1/persons/{pid}/documents/{sha}/original` | GET .../documents/{sha}/original —— CAS read_object **每次核 sha256**(内核内建防投毒)后流字节。 dedup/raw-only 件都能取到（CAS 按 hash 寻址，不经索引）——这是「原件永不丢」的兑现端点。 | ✅ 已实装 |
| `POST` | `/api/v1/persons/{pid}/entries/note` | 写一条纯文本笔记。原文即内容（不解析、不关联用药/诊断）。 | ✅ 已实装 |
| `POST` | `/api/v1/persons/{pid}/entries/profile-event` | 写一条病程动作日志（如「开启病程档案」「停药」）。`at` 是**事件发生的那天** （YYYY-MM-DD，用户说的那天），`payload` 必须是 JSON 对象。 | ✅ 已实装 |
| `POST` | `/api/v1/persons/{pid}/entries/self-measurement` | 写一条自测记录（结构化、append-only）。血压是**两个值共享一条记录**。 生理学不可能的值（138388 mmHg / 收缩压≤舒张压）⇒ 400 并给出原因。 | ✅ 已实装 |
| `GET` | `/api/v1/persons/{pid}/labs` | 指标趋势——一人一箱下 vault 全集即档案；证据回链 point.docref(CAS sha)可溯原件。见 vaults::labs_aggregate。 | ✅ 已实装 |
| `GET` | `/api/v1/persons/{pid}/measurements` | 自测值/笔记/动作日志的**回放投影**（写侧见同批实装的三个 POST 端点）。 载荷用上游 `parse_*_payload` 逐字读回；家庭参考区间用上游 `home_ref_range` （**查不到出处就不给区间**）；`kind`/`from`/`to` 真过滤（非法日期 400 带原因）。 | ✅ 已实装 |
| `GET` | `/api/v1/persons/{pid}/medications` | 用药跨度——与 labs 同一批文档、同一次 parser::aggregate 的另一块输出（D-026）。 证据回链 mention.docref（CAS sha）可一路溯源到原件字节；剂量/日期**原样透传不改写**（D2）。 | ✅ 已实装 |
| `GET` | `/api/v1/persons/{pid}/search` | FTS5+jieba 全文检索(中文分词 POC步骤06实证)。q 必填；limit 缺省20、钳 [1,100]。 | ✅ 已实装 |
| `GET` | `/api/v1/persons/{pid}/timeline` | 健康事件时间线 —— Vault::timeline 直出；按 doc_date DESC、无日期件排在最后(内核 ORDER BY 语义)。 | ✅ 已实装 |

## 5. 端点详解

### `GET /api/v1/audit`

审计视图：两层真相层合并（**口径分层**，不假装只有一层）。 - `source="registry"`：person registry 的 append-only 原始事件（create/update/delete，**不折叠**）。 - `source="vault"`：上游 `Vault::audit_log()` 的投影（导入/导出/分享/删除）——**复用上游判据，不另写一份**。 参数 `person`/`op` 是**真过滤**（真进判据，不是摆设）。

- 鉴权：需要 Bearer token

**参数**

| 名 | 位置 | 必填 | 说明 |
|---|---|---|---|
| `person` | query | 否 | 只看某人（person_id） |
| `op` | query | 否 | 只看某动作：create|update|delete（registry 侧）或 导入|导出|分享|删除（vault 侧） |
**响应**

| 码 | 说明 |
|---|---|
| `200` | audit entries (newest first) |


**示例**

```bash
curl -X GET \
  -H 'Authorization: Bearer $EHR_API_TOKEN' \
  'http://127.0.0.1:8443/api/v1/audit?person=<person>&op=<op>'
```

### `GET /api/v1/capabilities`

**ingest 能力清单**（D-034）：本构建**能吃哪些文件类型**、哪种类型走哪条路、 哪种能力**没链接进来**（默认构建没开 OCR ⇒ 图片被如实拒）。

- 鉴权：需要 Bearer token
**响应**

| 码 | 说明 |
|---|---|
| `200` | ingest capability manifest (accepted/rejected file types, ocr engine availability) |


**示例**

```bash
curl -X GET \
  -H 'Authorization: Bearer $EHR_API_TOKEN' \
  'http://127.0.0.1:8443/api/v1/capabilities'
```

### `PUT /api/v1/documents/{sha}/extraction`

AI agent 结构化发现(引用不替代，字段级 evidence 锚点)（stub→501）

> ⚠️ **本端点当前返回 501（尚未实装）**——下方是已固定的契约形状，实现后语义不再变。

- 鉴权：需要 Bearer token
**响应**

| 码 | 说明 |
|---|---|
| `200` | planned |
| `501` | not implemented yet (stage-2 in progress) |


**示例**

```bash
curl -X PUT \
  -H 'Authorization: Bearer $EHR_API_TOKEN' \
  'http://127.0.0.1:8443/api/v1/documents/<sha256>/extraction'
```

### `GET /api/v1/healthz`

健康检查

- 鉴权：公开（免 token）
**响应**

| 码 | 说明 |
|---|---|
| `200` | ok |


**示例**

```bash
curl -X GET \
  -H 'Authorization: Bearer $EHR_API_TOKEN' \
  'http://127.0.0.1:8443/api/v1/healthz'
```

### `GET /api/v1/persons`

人员列表（读 registry/persons.jsonl 全量折叠；MVP 规模足够，规模化后换派生索引——真相层不变）

- 鉴权：需要 Bearer token
**响应**

| 码 | 说明 |
|---|---|
| `200` | person list |


**示例**

```bash
curl -X GET \
  -H 'Authorization: Bearer $EHR_API_TOKEN' \
  'http://127.0.0.1:8443/api/v1/persons'
```

### `POST /api/v1/persons`

建档 —— person registry 是本项目唯一新增真相层（append-only JSONL，与内核事件日志同哲学）

- 鉴权：需要 Bearer token

**请求体**（content-type: application/json）

```json
PersonNew
```

**响应**

| 码 | 说明 |
|---|---|
| `201` | created person |


**示例**

```bash
curl -X POST \
  -H 'Authorization: Bearer $EHR_API_TOKEN' \
  'http://127.0.0.1:8443/api/v1/persons' \
  -H 'content-type: application/json' \
  -d '<JSON body>'
```

### `GET /api/v1/persons/{pid}/conditions`

病程泳道——与 labs/medications 同一批文档、同一次 parser::aggregate 的第三块输出（D-027）。 诊断原文**原样透传不归一**（上游无术语归一字典），ICD 码仅当原件印了才有； 证据回链 docref（CAS sha）可一路溯源到原件字节（D2）。

- 鉴权：需要 Bearer token

**参数**

| 名 | 位置 | 必填 | 说明 |
|---|---|---|---|
| `pid` | path | 是 | person uuid |
**响应**

| 码 | 说明 |
|---|---|
| `200` | aggregated condition lane (raw diagnosis text + onset + evidence docrefs) |
| `404` | person not found |


**示例**

```bash
curl -X GET \
  -H 'Authorization: Bearer $EHR_API_TOKEN' \
  'http://127.0.0.1:8443/api/v1/persons/<person_id>/conditions'
```

### `GET /api/v1/persons/{pid}/documents`

文档列表（documents.json 折叠视图，docref=sha256）。timeline 的扩展列版。

- 鉴权：需要 Bearer token

**参数**

| 名 | 位置 | 必填 | 说明 |
|---|---|---|---|
| `pid` | path | 是 | person uuid |
**响应**

| 码 | 说明 |
|---|---|
| `200` | documents with CAS sha256 refs |
| `404` | person not found |


**示例**

```bash
curl -X GET \
  -H 'Authorization: Bearer $EHR_API_TOKEN' \
  'http://127.0.0.1:8443/api/v1/persons/<person_id>/documents'
```

### `POST /api/v1/persons/{pid}/documents`

MVP 直传同步入库(锁=每人串行占位)；OCR/DICOM worker 化后置。见 vaults::ingest_upload。

- 鉴权：需要 Bearer token

**参数**

| 名 | 位置 | 必填 | 说明 |
|---|---|---|---|
| `pid` | path | 是 | person uuid |

**请求体**（content-type: multipart/form-data）

```json
UploadReply
```

**响应**

| 码 | 说明 |
|---|---|
| `200` | ingest result (deduped flag tells idempotent re-upload) |
| `400` | missing file field / bad filename extension |
| `404` | person not found |
| `501` | extension needs dicom/ocr worker |


**示例**

```bash
curl -X POST \
  -H 'Authorization: Bearer $EHR_API_TOKEN' \
  'http://127.0.0.1:8443/api/v1/persons/<person_id>/documents' \
  -F 'file=@<原件路径>'
```

### `GET /api/v1/persons/{pid}/documents/{sha}`

GET /persons/{pid}/documents/{sha} —— document + OCR文本层 + extraction JSON(API-DESIGN)。

- 鉴权：需要 Bearer token

**参数**

| 名 | 位置 | 必填 | 说明 |
|---|---|---|---|
| `pid` | path | 是 | person uuid |
| `sha` | path | 是 | sha256 content hash(64位小写hex) |
**响应**

| 码 | 说明 |
|---|---|
| `200` | document detail with text layer & extraction |
| `400` | malformed sha256 |
| `404` | person or indexed document not found |


**示例**

```bash
curl -X GET \
  -H 'Authorization: Bearer $EHR_API_TOKEN' \
  'http://127.0.0.1:8443/api/v1/persons/<person_id>/documents/<sha256>'
```

### `GET /api/v1/persons/{pid}/documents/{sha}/original`

GET .../documents/{sha}/original —— CAS read_object **每次核 sha256**(内核内建防投毒)后流字节。 dedup/raw-only 件都能取到（CAS 按 hash 寻址，不经索引）——这是「原件永不丢」的兑现端点。

- 鉴权：需要 Bearer token

**参数**

| 名 | 位置 | 必填 | 说明 |
|---|---|---|---|
| `pid` | path | 是 | person uuid |
| `sha` | path | 是 | sha256 content hash |
**响应**

| 码 | 说明 |
|---|---|
| `200` | raw bytes (integrity-verified) |
| `400` | malformed sha256 |
| `404` | person or CAS object not found |
| `500` | CAS integrity mismatch — poisoned object (D5 red flag) |


**示例**

```bash
curl -X GET \
  -H 'Authorization: Bearer $EHR_API_TOKEN' \
  'http://127.0.0.1:8443/api/v1/persons/<person_id>/documents/<sha256>/original'
```

### `POST /api/v1/persons/{pid}/entries/note`

写一条纯文本笔记。原文即内容（不解析、不关联用药/诊断）。

- 鉴权：需要 Bearer token

**参数**

| 名 | 位置 | 必填 | 说明 |
|---|---|---|---|
| `pid` | path | 是 | person uuid |

**请求体**（content-type: application/json）

```json
NoteIn
```

**响应**

| 码 | 说明 |
|---|---|
| `201` | note recorded |
| `400` | empty text / bad taken_at |
| `404` | person not found |


**示例**

```bash
curl -X POST \
  -H 'Authorization: Bearer $EHR_API_TOKEN' \
  'http://127.0.0.1:8443/api/v1/persons/<person_id>/entries/note' \
  -H 'content-type: application/json' \
  -d '<JSON body>'
```

### `POST /api/v1/persons/{pid}/entries/profile-event`

写一条病程动作日志（如「开启病程档案」「停药」）。`at` 是**事件发生的那天** （YYYY-MM-DD，用户说的那天），`payload` 必须是 JSON 对象。

- 鉴权：需要 Bearer token

**参数**

| 名 | 位置 | 必填 | 说明 |
|---|---|---|---|
| `pid` | path | 是 | person uuid |

**请求体**（content-type: application/json）

```json
ProfileEventIn
```

**响应**

| 码 | 说明 |
|---|---|
| `201` | profile event recorded |
| `400` | bad at / payload not an object |
| `404` | person not found |


**示例**

```bash
curl -X POST \
  -H 'Authorization: Bearer $EHR_API_TOKEN' \
  'http://127.0.0.1:8443/api/v1/persons/<person_id>/entries/profile-event' \
  -H 'content-type: application/json' \
  -d '<JSON body>'
```

### `POST /api/v1/persons/{pid}/entries/self-measurement`

写一条自测记录（结构化、append-only）。血压是**两个值共享一条记录**。 生理学不可能的值（138388 mmHg / 收缩压≤舒张压）⇒ 400 并给出原因。

- 鉴权：需要 Bearer token

**参数**

| 名 | 位置 | 必填 | 说明 |
|---|---|---|---|
| `pid` | path | 是 | person uuid |

**请求体**（content-type: application/json）

```json
SelfMeasurementIn
```

**响应**

| 码 | 说明 |
|---|---|
| `201` | self-measurement recorded |
| `400` | empty values / implausible value / bad taken_at |
| `404` | person not found |


**示例**

```bash
curl -X POST \
  -H 'Authorization: Bearer $EHR_API_TOKEN' \
  'http://127.0.0.1:8443/api/v1/persons/<person_id>/entries/self-measurement' \
  -H 'content-type: application/json' \
  -d '<JSON body>'
```

### `GET /api/v1/persons/{pid}/labs`

指标趋势——一人一箱下 vault 全集即档案；证据回链 point.docref(CAS sha)可溯原件。见 vaults::labs_aggregate。

- 鉴权：需要 Bearer token

**参数**

| 名 | 位置 | 必填 | 说明 |
|---|---|---|---|
| `pid` | path | 是 | person uuid |
**响应**

| 码 | 说明 |
|---|---|
| `200` | aggregated lab series (day precision + H/L flags + evidence docrefs) |
| `404` | person not found |


**示例**

```bash
curl -X GET \
  -H 'Authorization: Bearer $EHR_API_TOKEN' \
  'http://127.0.0.1:8443/api/v1/persons/<person_id>/labs'
```

### `GET /api/v1/persons/{pid}/measurements`

自测值/笔记/动作日志的**回放投影**（写侧见同批实装的三个 POST 端点）。 载荷用上游 `parse_*_payload` 逐字读回；家庭参考区间用上游 `home_ref_range` （**查不到出处就不给区间**）；`kind`/`from`/`to` 真过滤（非法日期 400 带原因）。

- 鉴权：需要 Bearer token

**参数**

| 名 | 位置 | 必填 | 说明 |
|---|---|---|---|
| `pid` | path | 是 | person uuid |
| `kind` | query | 否 | 只看某一类：self_measurement|note|profile_event |
| `from` | query | 否 | 起始日期(含) YYYY-MM-DD |
| `to` | query | 否 | 结束日期(含) YYYY-MM-DD |
**响应**

| 码 | 说明 |
|---|---|
| `200` | replayed manual entries (self-measurement / note / profile-event) |
| `400` | bad kind / bad date / from>to |
| `404` | person not found |


**示例**

```bash
curl -X GET \
  -H 'Authorization: Bearer $EHR_API_TOKEN' \
  'http://127.0.0.1:8443/api/v1/persons/<person_id>/measurements?kind=<kind>&from=<from>&to=<to>'
```

### `GET /api/v1/persons/{pid}/medications`

用药跨度——与 labs 同一批文档、同一次 parser::aggregate 的另一块输出（D-026）。 证据回链 mention.docref（CAS sha）可一路溯源到原件字节；剂量/日期**原样透传不改写**（D2）。

- 鉴权：需要 Bearer token

**参数**

| 名 | 位置 | 必填 | 说明 |
|---|---|---|---|
| `pid` | path | 是 | person uuid |
**响应**

| 码 | 说明 |
|---|---|
| `200` | aggregated medication spans (with per-mention evidence docrefs) |
| `404` | person not found |


**示例**

```bash
curl -X GET \
  -H 'Authorization: Bearer $EHR_API_TOKEN' \
  'http://127.0.0.1:8443/api/v1/persons/<person_id>/medications'
```

### `GET /api/v1/persons/{pid}/search`

FTS5+jieba 全文检索(中文分词 POC步骤06实证)。q 必填；limit 缺省20、钳 [1,100]。

- 鉴权：需要 Bearer token

**参数**

| 名 | 位置 | 必填 | 说明 |
|---|---|---|---|
| `pid` | path | 是 | person uuid |
| `q` | query | 是 | 检索词 |
| `limit` | query | 否 | 命中上限(默认20) |
**响应**

| 码 | 说明 |
|---|---|
| `200` | search hits with [highlighted] snippets |
| `400` | missing/blank q |
| `404` | person not found |


**示例**

```bash
curl -X GET \
  -H 'Authorization: Bearer $EHR_API_TOKEN' \
  'http://127.0.0.1:8443/api/v1/persons/<person_id>/search?q=<检索词>&limit=<limit>'
```

### `GET /api/v1/persons/{pid}/timeline`

健康事件时间线 —— Vault::timeline 直出；按 doc_date DESC、无日期件排在最后(内核 ORDER BY 语义)。

- 鉴权：需要 Bearer token

**参数**

| 名 | 位置 | 必填 | 说明 |
|---|---|---|---|
| `pid` | path | 是 | person uuid |
**响应**

| 码 | 说明 |
|---|---|
| `200` | timeline entries |
| `404` | person not found |


**示例**

```bash
curl -X GET \
  -H 'Authorization: Bearer $EHR_API_TOKEN' \
  'http://127.0.0.1:8443/api/v1/persons/<person_id>/timeline'
```

## 6. 数据模型（components.schemas）

### `AuditEntryOut`

审计条目（两层真相层合并后的统一视图）。

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `detail` | object | 否 | 概要（文件名 / 动作细节 / 变更后的主数据）。**可选键**（registry 的 delete 墓碑无 data）。 |
| `op` | string | 是 | 动作名。registry 侧 = `create`/`update`/`delete`；vault 侧 = 上游中文动作名 （`导入`/`导出`/`分享`/`删除`）——**原样透传，不归一**（归一会让上游口径失真）。 |
| `person_id` | string | 是 | 归属的人（person_id）。vault 侧由「哪个人的 vault」推出。 |
| `sha256` | object | 否 | 内容 sha256（可核验、防篡改）。**可选键**（registry 事件没有内容哈希）。 |
| `source` | string | 是 | 真相层来源：`registry`（本项目的人员主数据）/ `vault`（上游内核事件日志）。 |
| `ts` | string | 是 | 事件时刻（RFC3339 UTC）。 |

### `AuditQuery`

审计查询参数。

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `op` | object | 否 | 只看某个动作（大小写不敏感）。registry 侧 `create`/`update`/`delete`； vault 侧上游中文动作名 `导入`/`导出`/`分享`/`删除`。 |
| `person` | object | 否 | 只看某个人（person_id）。缺省 = 全部人。 |

### `AuditReply`

审计回执。

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `count` | integer | 是 | 命中的条目数。 |
| `entries` | array<AuditEntryOut> | 是 | 命中的审计条目（**新est first**，与上游 `audit_log()` 的展示口径一致）。 |
| `persons_scanned` | integer | 是 | 参与审计的人数（registry 侧有 vault 的人 ∪ registry 里出现过的人）。 |

### `ConditionOut`

一条诊断（跨文档合并后仍保留全部出处）。

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `docrefs` | array<string> | 是 | 提及该诊断的全部原件 docref（= content_hash，去重升序） |
| `icd_code` | object | 否 | 原文括号里印着的 ICD 码（如 E11.9）——**仅当原件印了**才有； 上游不校验、不臆造，此处原样透传（不据此改写展示名） |
| `onset` | object | 否 | 最早一次提及的日期；来源文档无日期时 None |
| `raw_text` | string | 是 | 诊断原文写法（上游**不做**术语归一，故原样透传） |

### `ConditionsReply`

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `condition_count` | integer | 是 | 诊断条数 |
| `conditions` | array<ConditionOut> | 是 |  |
| `documents_scanned` | integer | 是 | 参与聚合的文档数（仅 text 层齐备者；同 labs/medications 判据） |
| `person_id` | string | 是 |  |

### `DocumentDetail`

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `byte_size` | integer(int64) | 是 |  |
| `content_hash` | string | 是 |  |
| `doc_date` | object | 否 |  |
| `doc_date_end` | object | 否 |  |
| `doc_type` | string | 是 |  |
| `document_id` | integer(int64) | 是 |  |
| `encounter_id` | object | 否 |  |
| `extraction` | null | ExtractionPass | 否 |  |
| `imported_at` | string | 是 |  |
| `language` | object | 否 |  |
| `mime_type` | string | 是 |  |
| `ocr_meta` | null | TextLayerMeta | 否 |  |
| `original_name` | string | 是 |  |
| `page_count` | integer(int32) | 是 |  |
| `text` | object | 否 |  |
| `text_indexed` | boolean | 是 |  |
| `text_truncated_bytes` | integer | 是 |  |
| `title` | object | 否 |  |

### `DocumentListItem`

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `byte_size` | integer(int64) | 是 |  |
| `content_hash` | string | 是 |  |
| `doc_date` | object | 否 |  |
| `doc_date_end` | object | 否 |  |
| `doc_type` | string | 是 |  |
| `document_id` | integer(int64) | 是 |  |
| `mime_type` | string | 是 |  |
| `original_name` | object | 否 |  |
| `text_indexed` | boolean | 是 |  |
| `title` | object | 否 |  |

### `EntryReply`

写侧统一回执（三类条目同形，便于 agent 一套代码处理）。

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `content_hash` | string | 是 | 该条目的合成文本在 CAS 里的 sha256 —— **可回链到原件字节** （`GET /persons/{pid}/documents/{sha}/original`），与上传件同一套寻址。 |
| `deduped` | boolean | 是 | true = 内容与既有条目逐字相同（CAS 去重）⇒ **没有新增记录**。 正常路径恒 false（正文带毫秒记录时间）；true 时必须让调用方知道「没写进去」。 |
| `doc_type` | string | 是 | doc_type（`self_measurement`/`note`/`profile_event`）。 |
| `document_id` | integer(int64) | 是 | 新建（或复用）的 document id。 |
| `taken_at` | string | 是 | 事件/记录时刻（RFC3339 UTC）。 |
| `title` | object | 否 | 标题（时间线展示用）。 |

### `ExtractionPass`

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `json` | any | 否 |  |
| `parseable` | boolean | 是 |  |
| `raw` | object | 否 |  |

### `LabPointOut`

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `date` | object | 否 | 日精度 RFC3339 日期(YYYY-MM-DD);来源文档无日期时 None(内核排序保证 None 殿后) |
| `docref` | string | 是 | 证据 docref=该数值出处原件的 sha256 → GET /documents/{sha} 可查全文并 rehash |
| `flag` | object | 否 | "H"/"L" 异常标志(原样透传) |
| `unit` | object | 否 |  |
| `unverified` | boolean | 是 | true=云端抽取且本地校验器未能在原文确认(图表安全标记;正则路径恒 false) |
| `value` | number(double) | 是 | 显示基准值 —— 与所在序列 ref_low/ref_high 同单位(上游不变量,D2 引用不改写) |
| `value_canonical` | object | 否 | 规范单位换算值(跨院比较只许用它;无可换算时 None) |

### `LabSeriesOut`

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `analyte_key` | object | 否 | 词典归一后的分析物键;未识别项=None 且绝不与已识别项混组(内核分组语义) |
| `any_abnormal` | boolean | 是 |  |
| `group_name` | string | 是 |  |
| `loinc` | object | 否 |  |
| `points` | array<LabPointOut> | 是 |  |
| `ref_high` | object | 否 |  |
| `ref_high_canonical` | object | 否 |  |
| `ref_low` | object | 否 | 参考区间(与点同单位;无法保证则成对 None——不给错单位的一对数) |
| `ref_low_canonical` | object | 否 |  |
| `self_measured` | boolean | 是 | true=这条序列来自**手动录入的自测值**（家庭测量），false=来自医院报告。 ⚠️ 上游用 `GroupKey::SelfMeasured` 把两者**结构上分开**（家庭自测血压与 门诊血压参考区间不同，永不画成一条线）——但**不告诉消费方**的话， 消费方会把家庭值当医院值解读（参考区间就是错的）。故此处如实透传上游 的 `self_measured` 标志（D-028：不说=粉饰）。 |
| `unit_canonical` | object | 否 |  |
| `values_converted` | boolean | 是 | true=序列混过不同印刷单位已统一换算(纸上找不到这个数字,渲染必须言明) |

### `LabsReply`

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `documents_scanned` | integer | 是 | 参与聚合的文档数(仅 text 层齐备者;raw-only 无文本可析,MVP 不计入 notes_withheld) |
| `person_id` | string | 是 |  |
| `series` | array<LabSeriesOut> | 是 |  |
| `series_count` | integer | 是 |  |
| `total_points` | integer | 是 |  |

### `MeasurementOut`

一条手动记录（自测值 或 动作事件）。

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `docref` | string | 是 | 证据 docref = 合成文本的 sha256（可走 `documents/{sha}/original` 取回字节） |
| `document_id` | integer(int64) | 是 |  |
| `event_at` | object | 否 | 事件发生日期（`YYYY-MM-DD`，**用户说的那天**，不是记录那天） |
| `event_kind` | object | 否 | 动作事件字段（`profile_event` 才有）：kind / 病种 / **事件发生的那天** / 载荷 |
| `event_package` | object | 否 |  |
| `event_payload` | any | 否 |  |
| `kind` | string | 是 | 记录类型（`self_measurement`/`note`/`profile_event`） |
| `payload_readable` | boolean | 是 | true = 结构化载荷**读不出来**（哨兵缺失/JSON 坏了/版本不符）。 此时 `values` 为空、原文仍在 `text` 里 —— **如实标注，不半猜**。 |
| `taken_at` | string | 是 | 记录时刻（RFC3339 UTC）——文档日期 |
| `text` | object | 否 | 笔记正文（`note` 才有；原样透传，**不解析**它是什么） |
| `title` | object | 否 | 标题（时间线展示用） |
| `values` | array<MeasurementValueOut> | 否 | 自测值（`self_measurement` 才有；一条记录可含多个值——血压是两个值） |

### `MeasurementValueOut`

一条自测值（从合成载荷逐字读回）。

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `analyte_key` | string | 是 | 规范分析物键（如 `bp_systolic`） |
| `flag` | object | 否 | 相对家庭区间的标志（H/L/N）。**无区间时恒为 None**（不猜「正常」）。 |
| `ref_high` | object | 否 |  |
| `ref_low` | object | 否 | 家庭参考区间（上游 `home_ref_range`：**查不到出处就不给区间**，绝不臆造阈值） |
| `ref_source` | object | 否 | 区间出处（上游写明的来源文本；随区间一起给，不单独出现） |
| `unit` | string | 是 | 单位（**原样**，写入方恒发规范单位，本层不换算） |
| `value` | number(double) | 是 |  |

### `MeasurementsQuery`

measurements 投影的查询参数。

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `from` | object | 否 | 起始日期（含），`YYYY-MM-DD`。缺省 = 不限。 |
| `kind` | object | 否 | 只看某一类（`self_measurement`/`note`/`profile_event`）。缺省 = 全看。 |
| `to` | object | 否 | 结束日期（含），`YYYY-MM-DD`。缺省 = 不限。 |

### `MeasurementsReply`

measurements 投影的整个回执。

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `count` | integer | 是 | 投影到的记录数 |
| `measurements` | array<MeasurementOut> | 是 |  |
| `person_id` | string | 是 |  |

### `MedMentionOut`

一次用药提及（跨文档合并后仍逐条保留出处）。

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `date` | object | 否 | 日精度日期；来源文档无日期时 None |
| `docref` | string | 是 | 证据 docref = 该次提及出处原件的 sha256 → GET /documents/{sha} 可查全文并 rehash |
| `dose` | object | 否 | 原件里写的剂量字符串（如 "0.5g bid"）；只提了药名没写剂量时 None |
| `origin` | string | 是 | "llm"（云抽取 JSON）或 "regex"（正文正则）——上游原生标注，透传不改写 |
| `raw_name` | string | 是 | 该次提及里的药名**原文写法**（不做归一） |
| `unverified` | boolean | 是 | true=云抽取且未在原文核对上（上游语义，透传） |

### `MedSpanOut`

一个药品在全部提及上的跨度。

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `atc` | object | 否 | ATC 码（如可得） |
| `docrefs` | array<string> | 是 | 提及该药的全部原件 docref（去重、升序） |
| `drug_key` | object | 否 | 词典归一后的药品键；未识别（按原文分组）时 None |
| `end` | object | 否 |  |
| `latest_dose` | object | 否 | 最近一次提及里的剂量（如 "0.5g bid"） |
| `latest_raw_name` | object | 否 | 与 latest_dose 同一条提及里的**原文药名**（剂型/给药途径判定的唯一依据） |
| `mentions` | array<MedMentionOut> | 是 | 逐次提及（按日期升序，无日期的排最后） |
| `name` | string | 是 | 归一后的规范名；未识别时为原文名 |
| `raw_names` | array<string> | 是 | 全部**原文写法**（去重排序）——归一会丢掉剂型（地塞米松注射液/片），故并列保留 |
| `start` | object | 否 | 最早/最晚的带日期提及 |
| `status` | string | 是 | 上游恒为 "active"（不推断停药）——原样透传，**不擅自改写成"已停"** |
| `unverified` | boolean | 是 | 任一来源是云抽取未核对的（上游语义） |

### `MedicationsReply`

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `documents_scanned` | integer | 是 | 参与聚合的文档数（仅 text 层齐备者） |
| `med_count` | integer | 是 | 药品跨度数 |
| `medications` | array<MedSpanOut> | 是 |  |
| `mention_count` | integer | 是 | 逐次提及总数 |
| `person_id` | string | 是 |  |

### `NoteIn`

纯文本笔记请求体。原文即内容（上游 `add_note`）。

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `taken_at` | object | 否 | 记录时刻（RFC3339）。缺省 = 写入时刻。 |
| `text` | string | 是 |  |

### `Person`

对外人员视图（折叠后的当前态）。id=UUIDv4 字符串形式（schema 保持纯 string，不依赖可选 feature —— D1）

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `birthday` | object | 否 |  |
| `created_at` | string | 是 |  |
| `gender` | object | 否 |  |
| `id` | string | 是 |  |
| `id_note` | object | 否 |  |
| `name` | string | 是 |  |
| `updated_at` | string | 是 |  |

### `PersonNew`

建档请求体（PATCH/DELETE 端点在阶段2后半接入，复用此信封的 update/tombstone op）

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `birthday` | object | 否 |  |
| `gender` | object | 否 |  |
| `id_note` | object | 否 |  |
| `name` | string | 是 |  |

### `ProfileEventIn`

病程动作日志请求体（上游 `parser::ProfileEvent`）。

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `at` | string | 是 | 事件发生日期 `YYYY-MM-DD`（**用户说的那天**，不是记录那天）——形状不对会悄悄排错， 故在这道边界上就挡住（上游同款校验）。 |
| `kind` | string | 是 | 动作种类（如 `enable`/`disable`/`confirm_dx`/`flare`…）。**原样透传**， 不认得的 kind 照样落库（上游：将来能读它 ⇒ 「永远可重算」的前提）。 |
| `package` | string | 是 | 病种包 id（如 `sle`）。 |
| `payload` | object | 否 | 载荷 JSON **对象**（规则只按键取值；`null`/数组/裸数字对任何规则都只是噪音）。 |

### `SearchItem`

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `document_id` | integer(int64) | 是 |  |
| `snippet` | string | 是 | FTS5 snippet：[高亮]标记为内核原生产出(jieba预分词+unicode61 中文路径 POC步骤06实证) |
| `title` | object | 否 |  |

### `SelfMeasuredValueIn`

一个自测值（上游 `parser::SelfMeasuredValue` 的 HTTP 形态）。
`analyte_key` 是**规范键**（如 `bp_systolic`），不是显示名；`unit` 原样落库。

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `analyte_key` | string | 是 | 规范分析物键（如 `bp_systolic`/`bp_diastolic`/`heart_rate`/`body_weight`/ `body_temperature`/`glucose`）。未知键不报错（上游刻意不做白名单）， 但也不出参考区间/异常标志（上游 `home_ref_range` 的 `_ => None` 分支）。 |
| `unit` | string | 是 |  |
| `value` | number(double) | 是 |  |

### `SelfMeasurementIn`

自测记录请求体。血压是**两个值共享一条记录**（上游 §5.3：一次测量是最小编辑单元）。

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `taken_at` | object | 否 | 测量时刻（RFC3339）。缺省 = 写入时刻。 |
| `values` | array<SelfMeasuredValueIn> | 是 | 至少一个值；血压请同时给收缩压与舒张压。 |

### `TextLayerMeta`

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `backend` | object | 否 |  |
| `min_confidence` | object | 否 |  |
| `model_version` | object | 否 |  |
| `pages_with_text` | array<integer(int32)> | 是 |  |

### `TimelineItem`

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `doc_date` | object | 否 |  |
| `doc_date_end` | object | 否 |  |
| `doc_type` | string | 是 |  |
| `document_id` | integer(int64) | 是 |  |
| `title` | object | 否 |  |

### `UploadReply`

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `content_hash` | string | 是 |  |
| `deduped` | boolean | 是 |  |
| `doc_type` | object | 否 |  |
| `document_id` | object | 否 |  |
| `messages` | array<string> | 否 |  |
| `stored_raw_only` | boolean | 否 |  |
| `text_indexed` | boolean | 否 |  |

## 7. 尚未实装（501）的端点

| 方法 | 路径 | 计划语义 |
|---|---|---|
| `PUT` | `/api/v1/documents/{sha}/extraction` | AI agent 结构化发现(引用不替代，字段级 evidence 锚点)（stub→501） |

> 端点形状**已固定**（路由已挂、已进 OpenAPI），实现后语义不变；调用它们现在只会得到 501。
