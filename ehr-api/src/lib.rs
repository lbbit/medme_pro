// medme-ehr · ehr-api —— axum 服务核心（lib target：供 bin 与 tests/contract.rs 复用）
// MVP：/healthz、/openapi.json(utoipa+stub 合成)、persons registry(create/list)。
// 其余域按 docs/API-DESIGN.md v0 挂 OpenAPI 契约 + 501 stub：契约先行，实现随阶段推进。
use anyhow::{Context, Result};
use axum::{
    extract::{Json, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, patch, post, put},
    Router,
};
use serde_json::{json, Value};
use std::sync::Arc;
use utoipa::OpenApi;

mod persons;
pub use persons::{Person, PersonNew, PersonStore};
pub mod vaults;
pub use vaults::UploadReply; // 供 utoipa path!宏体解析(同 PersonNew 惯例)
use axum::extract::{Path as AxPath, Query};

pub struct AppState {
    pub home: std::path::PathBuf,
    pub store: PersonStore,
}

impl AppState {
    /// 测试/hook 用构造（bin 走 prepare_home_and_state，带 canonicalize+校验）
    pub fn from_home(home: impl Into<std::path::PathBuf>) -> Arc<AppState> {
        let home = home.into();
        Arc::new(AppState { store: PersonStore::new(home.clone()), home })
    }
}

// ---------- errors ----------
pub struct ApiError(pub StatusCode, pub String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}
impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        ApiError(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}"))
    }
}

// ---------- handlers（已实现域）----------
/// 健康检查
#[utoipa::path(get, path = "/api/v1/healthz", tag = "system",
    responses((status = 200, description = "ok")))]
async fn healthz(State(st): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    let ok = tokio::fs::try_exists(&st.home)
        .await
        .map_err(|_| ApiError(StatusCode::SERVICE_UNAVAILABLE, "ehr-home not accessible".into()))?;
    Ok(Json(json!({
        "status": if ok { "ok" } else { "no-home" },
        "home": st.home.display().to_string(),
    })))
}

/// 人员列表（读 registry/persons.jsonl 全量折叠；MVP 规模足够，规模化后换派生索引——真相层不变）
#[utoipa::path(get, path = "/api/v1/persons", tag = "persons",
    responses((status = 200, description = "person list", body = Vec<Person>)))]
async fn list_persons(State(st): State<Arc<AppState>>) -> Result<Json<Vec<Person>>, ApiError> {
    let v = tokio::task::block_in_place(|| st.store.list()).map_err(ApiError::from)?;
    Ok(Json(v))
}

/// 建档 —— person registry 是本项目唯一新增真相层（append-only JSONL，与内核事件日志同哲学）
#[utoipa::path(post, path = "/api/v1/persons", tag = "persons",
    request_body = PersonNew,
    responses((status = 201, description = "created person", body = Person)))]
async fn create_person(
    State(st): State<Arc<AppState>>,
    Json(req): Json<PersonNew>,
) -> Result<(StatusCode, Json<Person>), ApiError> {
    if req.name.trim().is_empty() {
        return Err(ApiError(StatusCode::BAD_REQUEST, "name is required".into()));
    }
    let p = tokio::task::block_in_place(|| st.store.create(&req)).map_err(ApiError::from)?;
    tracing::info!(id = %p.id, name = %p.name, "person created");
    Ok((StatusCode::CREATED, Json(p)))
}

// ---------- handlers（per-person vault 挂载域：timeline/search 已实现 D-010后半）----------
/// pid 必须存在于 registry 折叠列表且未归档——存在性校验即防路径注入的第一道闸（vaults.rs 注释）。
fn require_person(st: &AppState, pid: &str) -> Result<(), ApiError> {
    let found = st
        .store
        .list()
        .map_err(ApiError::from)?
        .into_iter()
        .any(|p| p.id == pid);
    if found {
        Ok(())
    } else {
        Err(ApiError(StatusCode::NOT_FOUND, format!("no such person (or archived): {pid}")))
    }
}

/// 健康事件时间线 —— Vault::timeline 直出；按 doc_date DESC、无日期件排在最后(内核 ORDER BY 语义)。
#[utoipa::path(get, path = "/api/v1/persons/{pid}/timeline", tag = "records", params(("pid" = String, Path, description = "person uuid")),
    responses((status = 200, description = "timeline entries", body = Vec<vaults::TimelineItem>),
               (status = 404, description = "person not found")))]
async fn get_timeline(
    State(st): State<Arc<AppState>>,
    AxPath(pid): AxPath<String>,
) -> Result<Json<Value>, ApiError> {
    tokio::task::block_in_place(|| {
        require_person(&st, &pid)?; // rusqlite/Vault 非 Send，整段住 block_in_place（同 list/create 模式）
        let vault = vaults::open_person_vault(&st.home, &pid).map_err(ApiError::from)?;
        // Vault API 返回 Result<_, MedmeError>；`?`+From<anyhow::Error>… 需手工中转一次(MedmeError→ApiError 500)
        let tl = vault.timeline().map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        let items: Vec<vaults::TimelineItem> =
            tl.into_iter().map(vaults::TimelineItem::from).collect();
        Ok(Json(json!({ "person_id": pid, "count": items.len(), "items": items })))
    })
}

/// FTS5+jieba 全文检索(中文分词 POC步骤06实证)。q 必填；limit 缺省20、钳 [1,100]。
#[utoipa::path(get, path = "/api/v1/persons/{pid}/search", tag = "records",
    params(("pid" = String, Path, description = "person uuid"),
           ("q" = String, Query, description = "检索词"),
           ("limit" = Option<u32>, Query, description = "命中上限(默认20)")),
    responses((status = 200, description = "search hits with [highlighted] snippets", body = Vec<vaults::SearchItem>),
               (status = 400, description = "missing/blank q"),
               (status = 404, description = "person not found")))]
async fn get_search(
    State(st): State<Arc<AppState>>,
    AxPath(pid): AxPath<String>,
    Query(qp): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    let q = qp.get("q").map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let Some(q) = q else {
        return Err(ApiError(StatusCode::BAD_REQUEST, "query parameter q is required".into()));
    };
    let limit: usize = match qp.get("limit") {
        // D5：非法 limit=400，不静默回落到缺省——参数“传了没生效”是本项目的历史病根(D-010④同族防御)
        Some(v) => v.parse::<usize>()
            .map_err(|_| ApiError(StatusCode::BAD_REQUEST, format!("bad limit: {v}")))?,
        None => 20,
    }
    .clamp(1, 100);
    tokio::task::block_in_place(|| {
        require_person(&st, &pid)?;
        let vault = vaults::open_person_vault(&st.home, &pid).map_err(ApiError::from)?;
        let items: Vec<vaults::SearchItem> =
            vault.search(&q, limit).map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
                .into_iter().map(vaults::SearchItem::from).collect();
        Ok(Json(json!({ "person_id": pid, "q": q, "count": items.len(), "items": items })))
    })
}

// ---------- handler（读侧·聚合域: labs = parser::aggregate() 跨文档趋势，POC步骤07的HTTP版 D-011）----------
/// 指标趋势——一人一箱下 vault 全集即档案；证据回链 point.docref(CAS sha)可溯原件。见 vaults::labs_aggregate。
#[utoipa::path(get, path = "/api/v1/persons/{pid}/labs", tag = "records", params(("pid" = String, Path, description = "person uuid")),
    responses((status = 200, description = "aggregated lab series (day precision + H/L flags + evidence docrefs)", body = vaults::LabsReply),
               (status = 404, description = "person not found")))]
async fn get_labs(
    State(st): State<Arc<AppState>>,
    AxPath(pid): AxPath<String>,
) -> Result<Json<vaults::LabsReply>, ApiError> {
    tokio::task::block_in_place(|| {
        require_person(&st, &pid)?; // 同 timeline/search：rusqlite/Vault 非 Send，整段住 block_in_place
        let vault = vaults::open_person_vault(&st.home, &pid).map_err(ApiError::from)?;
        let reply = vaults::labs_aggregate(&vault, &pid)
            .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        Ok(Json(reply))
    })
}

// ---------- handler（写侧首端点：multipart 上传 = POC import 路径的 HTTP 版）----------
/// MVP 直传同步入库(锁=每人串行占位)；OCR/DICOM worker 化后置。见 vaults::ingest_upload。
#[utoipa::path(post, path = "/api/v1/persons/{pid}/documents", tag = "entries",
    params(("pid" = String, Path, description = "person uuid")),
    request_body(content = UploadReply, description = "multipart/form-data，file 字段=原件(txt/pdf；其余暂 501)", content_type = "multipart/form-data"),
    responses((status = 200, description = "ingest result (deduped flag tells idempotent re-upload)", body = UploadReply),
               (status = 400, description = "missing file field / bad filename extension"),
               (status = 404, description = "person not found"),
               (status = 501, description = "extension needs dicom/ocr worker")))]
async fn upload_document(
    State(st): State<Arc<AppState>>,
    AxPath(pid): AxPath<String>,
    mp: axum::extract::Multipart,
) -> Result<(StatusCode, Json<vaults::UploadReply>), ApiError> {
    // 先在 async 侧收齐字节(axum multipart 是 Future 世界的事)，再进同步临界区。
    let mut file_field: Option<(String, Vec<u8>)> = None;
    let mut count_files = 0usize;
    let mut mp = mp;
    while let Some(part) = mp.next_field().await.map_err(|e| {
        ApiError(StatusCode::BAD_REQUEST, format!("bad multipart: {e}"))
    })? {
        if part.name() == Some("file") {
            count_files += 1;
            let name = part.file_name().unwrap_or("upload.bin").to_string();
            let bytes = part.bytes().await.map_err(|e| {
                ApiError(StatusCode::PAYLOAD_TOO_LARGE, format!("read field failed (size cap?): {e}"))
            })?;
            file_field = Some((name, bytes.to_vec()));
        }
        // 其余字段 MVP 忽略（未来:doc_type/title 覆盖等）
    }
    if count_files > 1 {
        return Err(ApiError(StatusCode::BAD_REQUEST, "exactly one `file` field expected".into()));
    }
    let Some((filename, bytes)) = file_field else {
        return Err(ApiError(StatusCode::BAD_REQUEST, "multipart must carry a `file` field".into()));
    };

    tokio::task::block_in_place(|| {
        require_person(&st, &pid)?;
        let _serial = vaults::LockToken::acquire(&pid); // 「每人串行」在此生效(RAII,multi-return忘解锁不可能;worker化后锁点语义不变)
        let vault = vaults::open_person_vault(&st.home, &pid).map_err(ApiError::from)?;
        if bytes.is_empty() {
            return Err(ApiError(StatusCode::BAD_REQUEST, "empty upload".into()));
        }
        let reply = vaults::ingest_upload(&vault, &filename, &bytes)
            .map_err(|e| {
                let es = format!("{e:#}");
                // D5:不支持的扩展名是契约内状态码(501)，不能混报成服务器内部错误(500会误导"服务坏了")。
                if es.contains("unsupported extension") {
                    ApiError(StatusCode::NOT_IMPLEMENTED, es)
                } else {
                    ApiError(StatusCode::INTERNAL_SERVER_ERROR, es)
                }
            })?;
        tracing::info!(person = %pid, hash = %reply.content_hash, deduped = reply.deduped, indexed = reply.text_indexed, stored_raw = reply.stored_raw_only, "document ingested");
        Ok((StatusCode::OK, Json(reply)))
    })
}

/// 文档列表（documents.json 折叠视图，docref=sha256）。timeline 的扩展列版。
#[utoipa::path(get, path = "/api/v1/persons/{pid}/documents", tag = "records", params(("pid" = String, Path, description = "person uuid")),
    responses((status = 200, description = "documents with CAS sha256 refs", body = Vec<vaults::DocumentListItem>),
               (status = 404, description = "person not found")))]
async fn list_documents(
    State(st): State<Arc<AppState>>,
    AxPath(pid): AxPath<String>,
) -> Result<Json<Value>, ApiError> {
    tokio::task::block_in_place(|| {
        require_person(&st, &pid)?; // 读走 WAL：无需持写锁
        let vault = vaults::open_person_vault(&st.home, &pid).map_err(ApiError::from)?;
        let items: Vec<vaults::DocumentListItem> =
            vaults::list_document_items(&vault).map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        Ok(Json(json!({ "person_id": pid, "count": items.len(), "items": items })))
    })
}

// ---------- handlers（hash docref 定位域：详情 + CAS 原件流）----------
/// GET /persons/{pid}/documents/{sha} —— document + OCR文本层 + extraction JSON(API-DESIGN)。
#[utoipa::path(get, path = "/api/v1/persons/{pid}/documents/{sha}", tag = "records",
    params(("pid" = String, Path, description = "person uuid"),
           ("sha" = String, Path, description = "sha256 content hash(64位小写hex)")),
    responses((status = 200, description = "document detail with text layer & extraction", body = vaults::DocumentDetail),
               (status = 400, description = "malformed sha256"),
               (status = 404, description = "person or indexed document not found")))]
async fn get_document_detail(
    State(st): State<Arc<AppState>>,
    AxPath((pid, sha)): AxPath<(String, String)>,
) -> Result<Json<vaults::DocumentDetail>, ApiError> {
    if !vaults::is_sha(&sha) {
        return Err(ApiError(StatusCode::BAD_REQUEST, "sha must be 64-char lowercase hex".into()));
    }
    tokio::task::block_in_place(|| {
        require_person(&st, &pid)?; // 读走 WAL：不持写锁(同 list_documents)
        let vault = vaults::open_person_vault(&st.home, &pid).map_err(ApiError::from)?;
        let d = vaults::document_detail(&vault, &sha)
            .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")))?
            .ok_or_else(|| ApiError(StatusCode::NOT_FOUND,
                format!("no indexed document for this hash ({sha})")))?;
        Ok(Json(d))
    })
}

/// GET .../documents/{sha}/original —— CAS read_object **每次核 sha256**(内核内建防投毒)后流字节。
/// dedup/raw-only 件都能取到（CAS 按 hash 寻址，不经索引）——这是「原件永不丢」的兑现端点。
#[utoipa::path(get, path = "/api/v1/persons/{pid}/documents/{sha}/original", tag = "records",
    params(("pid" = String, Path, description = "person uuid"),
           ("sha" = String, Path, description = "sha256 content hash")),
    responses((status = 200, description = "raw bytes (integrity-verified)", content_type = "application/octet-stream"),
               (status = 400, description = "malformed sha256"),
               (status = 404, description = "person or CAS object not found"),
               (status = 500, description = "CAS integrity mismatch — poisoned object (D5 red flag)")))]
async fn get_document_original(
    State(st): State<Arc<AppState>>,
    AxPath((pid, sha)): AxPath<(String, String)>,
) -> Result<Response, ApiError> {
    if !vaults::is_sha(&sha) {
        return Err(ApiError(StatusCode::BAD_REQUEST, "sha must be 64-char lowercase hex".into()));
    }
    tokio::task::block_in_place(|| {
        require_person(&st, &pid)?;
        let vault = vaults::open_person_vault(&st.home, &pid).map_err(ApiError::from)?;
        // /original 走纯 CAS：hash 即地址，无需 document 行(raw-only 件同样可下载)。
        match vaults::cas_read(&vault, &sha) {
            vaults::CasOutcome::Bytes(bytes) => {
                // mime/name 有索引行就用真值；没有就 honest default(D5:不猜——内核 import 时对非文本已嗅探，raw-only 必有正确 mime 的 doc 行走另一分支，这里仅剩理论缺口)。
                let meta = vaults::resolve_doc_ref(&vault, &sha)
                    .ok().flatten().map(|(_, sf)| (sf.mime_type.clone(), sf.original_name.clone()));
                let (mime, name) = match meta {
                    Some((m, n)) => (m, n),
                    None => ("application/octet-stream".to_string(), format!("{sha}.bin")),
                };
                let body_len = bytes.len();
                let headers = [
                    (header::CONTENT_TYPE, mime),
                    (header::CONTENT_LENGTH, body_len.to_string()),
                    (header::CONTENT_DISPOSITION, content_disposition(&name)),
                ];
                Ok((StatusCode::OK, headers, axum::body::Body::from(bytes)).into_response())
            }
            vaults::CasOutcome::MalformedHash =>
                Err(ApiError(StatusCode::BAD_REQUEST, "malformed sha256 hash".into())),
            vaults::CasOutcome::NotFound =>
                Err(ApiError(StatusCode::NOT_FOUND, "no CAS object with this hash in the vault".into())),
            vaults::CasOutcome::Integrity(m) => {
                tracing::error!(person = %pid, sha = %sha, "CAS INTEGRITY FAILURE: {m}");
                Err(ApiError(StatusCode::INTERNAL_SERVER_ERROR, m))
            }
            vaults::CasOutcome::Other(e) => Err(ApiError(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}"))),
        }
    })
}

/// Content-Disposition：`attachment; filename="ascii兜底"; filename*=UTF-8''百分号编码`(RFC6266/5987)。
fn content_disposition(name: &str) -> String {
    let ascii_fallback: String = name.chars()
        .map(|c| if c.is_ascii_graphic() && c != '"' && c != '\\' { c } else { '_' })
        .collect();
    const EXT_SAFE: &[u8] = b"!*-.0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ_abcdefghijklmnopqrstuvwxyz"; // RFC5987 attr-char(简写集,其余全部%XX)
    let mut enc = String::new();
    for b in name.bytes() {
        if EXT_SAFE.contains(&b) {
            enc.push(b as char);
        } else {
            enc.push('%');
            enc.push_str(&format!("{b:02X}")); // D4:大写hex是ext_val约定，逐字节转义不会裂多字节字符
        }
    }
    format!("attachment; filename=\"{ascii_fallback}\"; filename*=UTF-8''{enc}")
}

// ---------- stubs（契约先行，随阶段替换实现）----------
macro_rules! not_implemented {
    ($name:ident, $msg:expr) => {
        async fn $name() -> Response {
            ApiError(StatusCode::NOT_IMPLEMENTED, $msg.to_string()).into_response()
        }
    };
}
not_implemented!(stub_medications, "medications: pending");
not_implemented!(stub_conditions, "conditions: pending");
not_implemented!(stub_measurements, "measurements: pending");
not_implemented!(stub_self_measurement, "self-measurement entry: pending");
not_implemented!(stub_note, "note entry: pending");
not_implemented!(stub_profile_event, "profile-event entry: pending");
not_implemented!(stub_extraction, "extraction write: pending agent flow");
not_implemented!(stub_audit, "audit view: pending");
// 人员域三 stub：路由即契约(tests MOUNTED 表守卫)。刻意不进 CONTRACT/openapi——{pid} 模板与已文档化的
// /persons(get+post)同 path 不同 op，utoipa derive 无法拆分挂载；待 GET/PATCH/DELETE 实装时一并入文档。
not_implemented!(stub_person_detail, "person detail+profile summary: pending profile::materialize wiring");
not_implemented!(stub_person_update, "PATCH person: pending registry update event flow (same JSONL envelope, op=update)");
not_implemented!(stub_person_delete, "archive person(tombstone): pending registry delete event flow(op=delete)+purge二次确认");

// ---------- OpenAPI 合成（D1：CONTRACT 表与 router() 一一对应）----------
#[derive(OpenApi)]
#[openapi(
    info(
        title = "medme-ehr API",
        version = env!("CARGO_PKG_VERSION"),
        description = "多人电子健康档案服务。真相层=原件CAS+事件日志；派生索引可重建。设计稿见 docs/API-DESIGN.md"
    ),
    tags(
        (name = "system", description = "health/openapi/审计"),
        (name = "persons", description = "人员主数据（本项目唯一新增真相表）"),
        (name = "records", description = "读侧：timeline/search/documents/labs…（未实现端点先挂契约，现返回501）"),
        (name = "entries", description = "写侧：append-only 条目与文档上传（未实现端点先挂契约，现返回501）")
    ),
    paths(healthz, list_persons, create_person, get_timeline, get_search, upload_document, list_documents,
          get_document_detail, get_document_original),
    components(schemas(Person, PersonNew, vaults::TimelineItem, vaults::SearchItem, vaults::UploadReply,
                       vaults::DocumentListItem, vaults::DocumentDetail, vaults::TextLayerMeta, vaults::ExtractionPass,
                       vaults::LabPointOut, vaults::LabSeriesOut, vaults::LabsReply))
)]
struct ApiDoc;

/// 已注册但未实现的端点契约：(method, path, tag, summary) —— router() 必须逐条覆盖此表
const CONTRACT: &[(&str, &str, &str, &str)] = &[
    ("get", "/api/v1/persons/{pid}/labs", "records", "parser::aggregate().series(LabPoint 日精度+H/L flag+evidence)"),
    ("get", "/api/v1/persons/{pid}/medications", "records", "meds(MedSpan)聚合"),
    ("get", "/api/v1/persons/{pid}/conditions", "records", "conditions(AggregatedCondition 病程泳道)"),
    ("get", "/api/v1/persons/{pid}/measurements", "records", "SelfMeasurement/ProfileEvent 事件回放投影(type,from,to)"),
    ("post", "/api/v1/persons/{pid}/entries/self-measurement", "entries", "{type,value,unit,taken_at} → SelfMeasurement import"),
    ("post", "/api/v1/persons/{pid}/entries/note", "entries", "Note 条目"),
    ("post", "/api/v1/persons/{pid}/entries/profile-event", "entries", "ProfileEvent(parser::profile_event payload)"),
    ("put", "/api/v1/documents/{sha}/extraction", "entries", "AI agent 结构化发现(引用不替代，字段级 evidence 锚点)"),
    ("get", "/api/v1/audit", "system", "审计视图(person/op 过滤，内容哈希留痕)"),
];

fn stub_item(op: &str, tag: &str, summary: &str) -> Value {
    json!({ op.to_lowercase(): {
        "summary": format!("{summary}（stub→501）"),
        "tags": [tag],
        "responses": {
            "200": { "description": "planned" },
            "501": { "description": "not implemented yet (stage-2 in progress)" }
        }
    }})
}

/// 把 item 的 operation 并进 paths[path]（同 path 多 method 共存）
fn merge_path(paths: &mut serde_json::Map<String, Value>, path: &str, item: Value) {
    let entry = paths.entry(path.to_string()).or_insert_with(|| json!({}));
    if let (Some(obj), Some(src)) = (entry.as_object_mut(), item.as_object()) {
        for (k, v) in src {
            obj.insert(k.clone(), v.clone());
        }
    }
}

pub fn openapi_json() -> Value {
    // utoipa OpenApi 结构自带 serde(Serialize)，经字符串中转落为通用 Value(便于并入手写契约 stub)
    let s = ApiDoc::openapi().to_json().expect("openapi serialize");
    let mut doc: Value = serde_json::from_str(&s).expect("openapi json is valid");
    {
        let paths = doc["paths"].as_object_mut().expect("paths object");
        for (op, pth, tag, summary) in CONTRACT {
            merge_path(paths, pth, stub_item(op, tag, summary));
        }
    } // 交还借用，doc 可被移动返回
    doc
}

// ---------- router（判据：新增路由必同时进 CONTRACT —— 反向保险见 tests/contract.rs）----------
async fn openapi_route() -> Json<Value> {
    Json(openapi_json())
}

pub fn router(st: Arc<AppState>) -> Router {
    let api = Router::new()
        .route("/healthz", get(healthz))
        .route("/persons", get(list_persons).post(create_person))
        .route("/persons/{pid}", get(stub_person_detail)).route("/persons/{pid}", patch(stub_person_update)).route("/persons/{pid}", delete(stub_person_delete)) // 人员详情/修订/归档: pending(stage-2后半)——契约以路由为准(见 tests/contract.rs MOUNTED)
        .route("/persons/{pid}/timeline", get(get_timeline))
        .route("/persons/{pid}/search", get(get_search))
        .route("/persons/{pid}/documents", get(list_documents).post(upload_document))
        // hash docref 域已实装(从 CONTRACT stub 毕业)。两条显式路由，不用前缀通配(D7:多余后缀应404而非静默命中)。
        .route("/persons/{pid}/documents/{sha}", get(get_document_detail))
        .route("/persons/{pid}/documents/{sha}/original", get(get_document_original))
        .route("/persons/{pid}/labs", get(get_labs)) // D-011 实装毕业(此前 stub→501；期间一个旧二进制+孤儿进程伪装成501教训见D-011)
        .route("/persons/{pid}/medications", get(stub_medications))
        .route("/persons/{pid}/conditions", get(stub_conditions))
        .route("/persons/{pid}/measurements", get(stub_measurements))
        .route("/persons/{pid}/entries/self-measurement", post(stub_self_measurement))
        .route("/persons/{pid}/entries/note", post(stub_note))
        .route("/persons/{pid}/entries/profile-event", post(stub_profile_event))
        .route("/documents/{sha}/extraction", put(stub_extraction))
        .route("/audit", get(stub_audit))
        .with_state(st);

    Router::new()
        .nest("/api/v1", api)
        .route("/openapi.json", get(openapi_route)) // 服务端每次现合成，与代码内 CONTRACT 常量必然同步（D1）
}

/// serve/dry-run 共用的 home 校验：canonicalize、要求已存在、建 vaults/ 派生目录。
pub fn prepare_home_and_state(home_in: &std::path::Path, mkdir_vaults: bool) -> Result<Arc<AppState>> {
    let home = match home_in.canonicalize() {
        Ok(p) => p,
        Err(_) => anyhow::bail!(
            "EHR_HOME not accessible: {} — mkdir it first or fix env.",
            home_in.display()
        ),
    };
    if mkdir_vaults {
        std::fs::create_dir_all(home.join("vaults")).context("mkdir vaults")?;
    }
    Ok(Arc::new(AppState { store: PersonStore::new(home.clone()), home }))
}
