# EHR API 设计稿 v0（阶段 2 输入，未实现）

身份约定：所有资源在 `/api/v1` 下；鉴权 = Bearer JWT(access)，refresh 换发照抄上游 services/api 双 token 模式。
文档外部 id 一律用 **sha256 内容哈希**(`DocRef`)——DB 自增 id 仅设备本地有效(D3:派生库可重建)。

## 人员域（我们新增的唯一"真相表"：person registry）

| Method | Path | 说明 |
|---|---|---|
| GET | /persons | 列表(分页) |
| POST | /persons | 建档 {name,birthday,gender,id_note,...} → person_id(uuid) |
| GET | /persons/{pid} | 详情+PatientProfile 摘要(profile::materialize) |
| PATCH | /persons/{pid} | 修订主数据(事件化进独立 audit log) |
| DELETE | /persons/{pid}?purge=true | 归档/物理删除(purge 需二次确认参数) |

## 健康信息读侧（一人一 vault 聚合;全部带原件锚点 source_file_hash→可下载 CAS 原件）

| Method | Path | 内核映射 |
|---|---|---|
| GET | /persons/{pid}/timeline | Vault::timeline() + parser::aggregate 分组 |
| GET | /persons/{pid}/search?q=&limit= | Vault::search (FTS5+jieba) |
| GET | /persons/{pid}/documents | 文档列表(doc_type/date/title/docref) |
| GET | /persons/{pid}/documents/{sha256} | document_by_source_file hash + OCR文本层 + extraction JSON |
| GET | /persons/{pid}/documents/{sha256}/original | CAS 原件流(Content-Disposition) |
| GET | /persons/{pid}/labs?analyte=&from=&to= | **parser::aggregate().series**(LabPoint 日精度+H/L flag+evidence,不丢字段——对齐 mobile vault_projections 先例) |
| GET | /persons/{pid}/medications | aggregate meds(MedSpan) |
| GET | /persons/{pid}/conditions | aggregate conditions(AggregatedCondition 病程泳道) |
| GET | /persons/{pid}/measurements?type=BPM&from.. | SelfMeasurement/ProfileEvent 事件回放投影 |
| GET | /persons/{pid}/imaging/studies…slices/{id}.dcm | dicom crate study/slice API(阶段3+) |

## 写侧（全部 append-only，永不改原件）

| Method | Path | 语义 |
|---|---|---|
| POST | /persons/{pid}/documents (multipart) | =pipeline::ingest。MVP:PDF/PNG带文本层直入；OCR worker(feature gate)后置异步补 OcrAdded。**每人一个串行队列**：vault SQLite 单写者(D2:此约束属运行时而非产品配置→代码内常量可配) |
| POST | /persons/{pid}/entries/self-measurement | {type,value,unit,taken_at} → 合成文本 import(SelfMeasurement) |
| POST | /persons/{pid}/entries/note | Note |
| POST | /persons/{pid}/entries/profile-event | ProfileEvent(parser::profile_event payload;如"确诊高血压""停药") |
| PUT(extraction) | documents/{sha256}/extraction | AI agent 结构化发现 → ExtractionAdded(result_json→CAS,**引用不替代**;agent 结论必须字段级带 evidence 行锚点——上游红线) |
| GET | /audit?person=&op= | audit_log() 聚合视图(内容哈希留痕) |

## MCP/SKILL 映射（薄封装，不改内核）

tools: list_persons/get_health_summary(pid)/query_timeline/search/query_labs/record_entry/upload_document/add_extraction。SKILL.md 里教 agent:"先 search 再 timeline、数值只引原件 hash 的字段、诊断建议要免责"。

## 非目标(v1)

多租户 SaaS、FHIR 导出(profile/deid 已有原料,放 roadmap)、端到端加密同步(与明文服务端互斥,保持上游移动端自带)。
