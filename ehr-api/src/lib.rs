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
use serde::Serialize;
use serde_json::{json, Value};
use std::sync::Arc;
use utoipa::OpenApi;

pub mod auth; // T4：静态 Bearer token 鉴权（env 注入，ro/rw 分权，零新依赖）
mod persons;
pub use persons::{Person, PersonNew, PersonStore};
pub mod vaults;
pub mod rebuild; // 目标验收④：派生索引全量重建（删 medme.db → 从事件日志回放），显式命令 + 逐人计数证据
pub mod rebuildlock; // D-024：重建期互斥（锁文件+陈旧锁回收）——把「建议停服重建」从口头约定变成机制
pub mod apidoc; // T8：docs/API.md 人类可读接口文档的生成器（从运行时 OpenAPI 渲染，漂移门禁 api-doc-check.sh）
pub mod capabilities; // D-034：ingest 能力清单的**单一事实来源**（路由判据 + 派生清单，默认构建如实拒图片）
pub mod entries; // D-028：写侧手动条目（self-measurement / note / profile-event）——与上游同一条「合成文本当文件过一遍 import」路径
pub mod audit; // D-030：审计视图（vault 内核事件日志 + person registry 两层真相层，口径分层不假装只有一层）
mod b64; // base64 解码（写侧工具传二进制原件用；零新依赖，模块内自带单测）
pub mod mcp; // 阶段4：MCP server（零新依赖 JSON-RPC over POST /mcp，薄适配层复用 vaults）
pub use vaults::UploadReply; // 供 utoipa path!宏体解析(同 PersonNew 惯例)
use axum::extract::{Path as AxPath, Query};
use axum::extract::FromRequest; // mcp_endpoint 手动解 Json 以便先取 scope

pub struct AppState {
    pub home: std::path::PathBuf,
    pub store: PersonStore,
    /// T4+T46：token 管理面（env 种子 + registry/tokens.jsonl 回放，运行期可变——Web 端生成/吊销）
    pub auth: Arc<auth::TokenManager>,
    /// **写侧单件上传上限**（字节）——REST / MCP / 能力清单**三处共用的唯一一份**（T39-④）。
    ///
    /// ⚠️ 为什么**存在状态里**而不是各处各读 env：
    ///   上限被**三处**读（①路由的 body limit（建路由时）、②handler/MCP 工具的判据（请求时）、
    ///   ③能力清单的公布值）⇒ 若各读各的 env，则「启动后改了 env」会让三处看到**不同的值**
    ///   ⇒ 清单可能**说谎**（说 64MB，实际 32MB 就 413）⇒ 又一处「同一事实多份实现」。
    ///   ⇒ 纪律：**env 是启动期配置**——解析一次存进状态，之后**全进程读这一处**；
    ///     要改就**重启**（部署语义清晰，且不制造「半程改参」这种无法被测到的分叉）。
    ///   ⇒ 附带好处：**门禁可在进程内用小上限构造状态来测**（不必起子进程、不必手工编 rustc）。
    pub max_upload_bytes: usize,
}

impl AppState {
    /// 测试/hook 用构造（bin 走 prepare_home_and_state，带 canonicalize+校验）
    pub fn from_home(home: impl Into<std::path::PathBuf>) -> Arc<AppState> {
        let home = home.into();
        Arc::new(AppState {
            store: PersonStore::new(home.clone()),
            home,
            auth: Arc::new(auth::TokenManager::empty()),
            max_upload_bytes: capabilities::max_upload_bytes_from_env(),
        })
    }

    /// 带鉴权表构造（测试/门禁用）。非空 token 表 ⇒ 开启鉴权。
    /// T46：TokenSet 经 in-memory manager 包一层（不落文件——测试表不持久化）。
    pub fn from_home_with_auth(
        home: impl Into<std::path::PathBuf>,
        auth: auth::TokenSet,
    ) -> Arc<AppState> {
        let home = home.into();
        let mgr = if auth.is_open() {
            auth::TokenManager::empty()
        } else {
            // 把测试表转成 manager：借 load() 的合成路径但喂内存——直接构造（字段私有⇒走 load 空文件+env 注入等价）
            auth::TokenManager::load(&home.join(".test-tokens"), auth)
                .unwrap_or_else(|_| auth::TokenManager::empty())
        };
        Arc::new(AppState {
            store: PersonStore::new(home.clone()),
            home,
            auth: Arc::new(mgr),
            max_upload_bytes: capabilities::max_upload_bytes_from_env(),
        })
    }

    /// **带显式上传上限**构造（门禁/测试用）。
    ///
    /// ⚠️ 存在的理由（T39-④）：要在**不启动子进程**的前提下测「大载荷」这一维
    ///   （传 32MB 真文件既慢又占内存）⇒ 允许**注入一个小上限**（如 1024 字节）。
    ///   ⇒ 注意：**只有上限**可注入，其它行为不变 ⇒ 这条构造**不会**掩盖别的缺陷。
    pub fn from_home_with_limit(
        home: impl Into<std::path::PathBuf>,
        max_upload_bytes: usize,
    ) -> Arc<AppState> {
        let home = home.into();
        Arc::new(AppState {
            store: PersonStore::new(home.clone()),
            home,
            auth: Arc::new(auth::TokenManager::empty()),
            max_upload_bytes,
        })
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
// ---------- healthz / capabilities 的响应体（补齐 OpenAPI 的 body 声明）----------
//
// ⚠️ D-038：此前这两个端点的 200 响应在 OpenAPI 里**没有 body 声明**
// （`responses((status = 200, description = "…"))`）⇒ 生成的客户端**拿不到类型**，
// 只能自己猜「里面有什么」⇒ 「文档在骗人」的第四种形态（不是形状错，是**根本没形状**）。
#[derive(Serialize, utoipa::ToSchema)]
pub struct HealthzReply {
    /// `ok` = EHR_HOME 可达；`no-home` = 不可达（服务活着但**不能干活**）
    pub status: String,
    /// EHR_HOME 的绝对路径
    pub home: String,
}

/// 健康检查
#[utoipa::path(get, path = "/api/v1/healthz", tag = "system",
    responses((status = 200, description = "ok", body = HealthzReply)))]
async fn healthz(State(st): State<Arc<AppState>>) -> Result<Json<HealthzReply>, ApiError> {
    let ok = tokio::fs::try_exists(&st.home)
        .await
        .map_err(|_| ApiError(StatusCode::SERVICE_UNAVAILABLE, "ehr-home not accessible".into()))?;
    Ok(Json(HealthzReply {
        status: if ok { "ok".into() } else { "no-home".into() },
        home: st.home.display().to_string(),
    }))
}

/// **ingest 能力清单**（D-034）：本构建**能吃哪些文件类型**、哪种类型走哪条路、
/// 哪种能力**没链接进来**（默认构建没开 OCR ⇒ 图片被如实拒）。
///
/// 为什么要有这个端点：「本构建支持什么」此前是**三份实现**（ingest_upload 的判据 /
/// OpenAPI 的 501 文案 / 门禁脚本里的手写文案）⇒ 改一处漏一处，且红得看不出是漏改还是真坏了。
/// 现在**真源 = 实现**（`capabilities::route_for_ext`），本端点与 `--print-capabilities`
/// **从它派生** ⇒ 客户端/agent 自己就能判断「我的文件能不能传」，不必猜、不必看日志。
///
/// ⚠️ 诚实边界（D5）：`ocr_available=true` 只说明**链了**引擎，**不保证模型已就绪**
/// （PP-OCRv5 权重是运行期下载到 `$OAR_HOME` 的）⇒ 真失败时回执会如实带
/// `stored_raw_only=true` + messages，不假装识别成功。
#[utoipa::path(get, path = "/api/v1/capabilities", tag = "meta",
    responses((status = 200, description = "ingest capability manifest (accepted/rejected file types, ocr engine availability)", body = capabilities::CapabilityManifest)))]
async fn get_capabilities(
    State(st): State<Arc<AppState>>,
) -> Result<Json<capabilities::CapabilityManifest>, ApiError> {
    // T39-④：清单里的上限 == `st.max_upload_bytes` == 本进程真正生效的值
    // ⇒ 调用方看到的永远是**真实**上限（不是「env 此刻的值」）。
    Ok(Json(capabilities::capability_manifest(st.max_upload_bytes)))
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
    responses((status = 200, description = "timeline entries", body = vaults::TimelineReply),
               (status = 404, description = "person not found")))]
async fn get_timeline(
    State(st): State<Arc<AppState>>,
    AxPath(pid): AxPath<String>,
) -> Result<Json<vaults::TimelineReply>, ApiError> {
    tokio::task::block_in_place(|| {
        require_person(&st, &pid)?; // rusqlite/Vault 非 Send，整段住 block_in_place（同 list/create 模式）
        let vault = vaults::open_person_vault(&st.home, &pid).map_err(ApiError::from)?;
        // Vault API 返回 Result<_, MedmeError>；`?`+From<anyhow::Error>… 需手工中转一次(MedmeError→ApiError 500)
        let tl = vault.timeline().map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        let items: Vec<vaults::TimelineItem> =
            tl.into_iter().map(vaults::TimelineItem::from).collect();
        Ok(Json(vaults::TimelineReply { count: items.len(), items, person_id: pid }))
    })
}

/// FTS5+jieba 全文检索(中文分词 POC步骤06实证)。q 必填；limit 缺省20、钳 [1,100]。
#[utoipa::path(get, path = "/api/v1/persons/{pid}/search", tag = "records",
    params(("pid" = String, Path, description = "person uuid"),
           ("q" = String, Query, description = "检索词"),
           ("limit" = Option<u32>, Query, description = "命中上限(默认20)")),
    responses((status = 200, description = "search hits with [highlighted] snippets", body = vaults::SearchReply),
               (status = 400, description = "missing/blank q"),
               (status = 404, description = "person not found")))]
async fn get_search(
    State(st): State<Arc<AppState>>,
    AxPath(pid): AxPath<String>,
    Query(qp): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<vaults::SearchReply>, ApiError> {
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
        Ok(Json(vaults::SearchReply { count: items.len(), items, person_id: pid, q }))
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

// ---------- handler（读侧·聚合域: medications = parser::aggregate() 的 meds 面，D-026 stub 毕业）----------
/// 用药跨度——与 labs 同一批文档、同一次 parser::aggregate 的另一块输出（D-026）。
/// 证据回链 mention.docref（CAS sha）可一路溯源到原件字节；剂量/日期**原样透传不改写**（D2）。
#[utoipa::path(get, path = "/api/v1/persons/{pid}/medications", tag = "records", params(("pid" = String, Path, description = "person uuid")),
    responses((status = 200, description = "aggregated medication spans (with per-mention evidence docrefs)", body = vaults::MedicationsReply),
               (status = 404, description = "person not found")))]
async fn get_medications(
    State(st): State<Arc<AppState>>,
    AxPath(pid): AxPath<String>,
) -> Result<Json<vaults::MedicationsReply>, ApiError> {
    tokio::task::block_in_place(|| {
        require_person(&st, &pid)?; // 同 timeline/search/labs：rusqlite/Vault 非 Send，整段住 block_in_place
        let vault = vaults::open_person_vault(&st.home, &pid).map_err(ApiError::from)?;
        let reply = vaults::meds_aggregate(&vault, &pid)
            .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        Ok(Json(reply))
    })
}

// ---------- handler（读侧·聚合域: conditions = parser::aggregate() 的 conditions 面，D-027 stub 毕业）----------
/// 病程泳道——与 labs/medications 同一批文档、同一次 parser::aggregate 的第三块输出（D-027）。
/// 诊断原文**原样透传不归一**（上游无术语归一字典），ICD 码仅当原件印了才有；
/// 证据回链 docref（CAS sha）可一路溯源到原件字节（D2）。
#[utoipa::path(get, path = "/api/v1/persons/{pid}/conditions", tag = "records", params(("pid" = String, Path, description = "person uuid")),
    responses((status = 200, description = "aggregated condition lane (raw diagnosis text + onset + evidence docrefs)", body = vaults::ConditionsReply),
               (status = 404, description = "person not found")))]
async fn get_conditions(
    State(st): State<Arc<AppState>>,
    AxPath(pid): AxPath<String>,
) -> Result<Json<vaults::ConditionsReply>, ApiError> {
    tokio::task::block_in_place(|| {
        require_person(&st, &pid)?; // 同 timeline/search/labs：rusqlite/Vault 非 Send，整段住 block_in_place
        let vault = vaults::open_person_vault(&st.home, &pid).map_err(ApiError::from)?;
        let reply = vaults::conditions_aggregate(&vault, &pid)
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
    let max_bytes = st.max_upload_bytes;
    let mut file_field: Option<(String, Vec<u8>)> = None;
    let mut count_files = 0usize;
    let mut mp = mp;
    while let Some(part) = mp.next_field().await.map_err(|e| {
        // T39：**超限**与**格式坏**必须分开说。此前两者都报 400「bad multipart: Error parsing
        // `multipart/form-data` request」⇒ 调用方以为是自己 multipart 写错了（去改格式），
        // 实际是**文件太大**（该去压缩/分片）⇒ **错误语义误导**（D5：状态码不许粉饰）。
        // ⇒ 判据：axum 的 `MultipartError::status()` 把 body-limit 命中映射为 413 ⇒ 直接采信它。
        let st = e.status();
        ApiError(
            st,
            format!(
                "bad multipart: {e}{}",
                if st == StatusCode::PAYLOAD_TOO_LARGE {
                    format!("（上传超出单件上限 {max_bytes} 字节；上限见 GET /api/v1/capabilities 的 max_upload_bytes）")
                } else {
                    String::new()
                }
            ),
        )
    })? {
        if part.name() == Some("file") {
            count_files += 1;
            let name = part.file_name().unwrap_or("upload.bin").to_string();
            let bytes = part.bytes().await.map_err(|e| {
                let st = e.status();
                ApiError(
                    st,
                    format!(
                        "read field failed: {e}{}",
                        if st == StatusCode::PAYLOAD_TOO_LARGE {
                            format!("（上传超出单件上限 {max_bytes} 字节；上限见 GET /api/v1/capabilities 的 max_upload_bytes）")
                        } else {
                            String::new()
                        }
                    ),
                )
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
    // T39：**自己判单件上限**（判据与 MCP 同一处：`st.max_upload_bytes`）。
    // 路由上的 body limit 只比这个值**略宽**（留给 multipart 头部开销）⇒ 命中 body limit 时
    // 是「确实超了」，而**恰好超一点点**的情形由本行判据给出**精确**的 413（说明超了多少、
    // 上限是多少）⇒ 不再依赖「axum 默认 2MB」那个隐式上限。
    if bytes.len() > max_bytes {
        return Err(ApiError(
            StatusCode::PAYLOAD_TOO_LARGE,
            format!(
                "upload too large: {} bytes exceeds limit {} bytes（上限见 GET /api/v1/capabilities 的 max_upload_bytes）",
                bytes.len(),
                max_bytes
            ),
        ));
    }

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
    responses((status = 200, description = "documents with CAS sha256 refs", body = vaults::DocumentListReply),
               (status = 404, description = "person not found")))]
async fn list_documents(
    State(st): State<Arc<AppState>>,
    AxPath(pid): AxPath<String>,
) -> Result<Json<vaults::DocumentListReply>, ApiError> {
    tokio::task::block_in_place(|| {
        require_person(&st, &pid)?; // 读走 WAL：无需持写锁
        let vault = vaults::open_person_vault(&st.home, &pid).map_err(ApiError::from)?;
        let items: Vec<vaults::DocumentListItem> =
            vaults::list_document_items(&vault).map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        Ok(Json(vaults::DocumentListReply { count: items.len(), items, person_id: pid }))
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
    // ⚠️ D-039：content-type 声明为 **`*/*`（通配）**，不是 `application/octet-stream`。
    //   为什么：真响应的 Content-Type 是**动态的**——取自原件入库时嗅探/存储的 mime
    //   （`text/plain` / `application/pdf` / …），只有「索引行缺失」时才回落
    //   `application/octet-stream` ⇒ 此前把 OpenAPI 写死成 `application/octet-stream`
    //   是一句**对大多数文件都不成立的假话**（上传 .txt 后真回 `text/plain`）。
    //   OpenAPI 3.1 的 content map 允许 `*/*` 通配键 ⇒ 如实表达「任意二进制流，
    //   具体类型由原件的 mime 决定」；客户端按 `*/*` 处理（存字节）总是安全的。
    responses((status = 200, description = "raw bytes (integrity-verified); Content-Type = 原件存储的 mime（缺索引行时为 application/octet-stream）", content_type = "*/*"),
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

// ---------- handler（读侧·measurements 投影：D-028，写侧三类手动条目的回放）----------
/// 自测值/笔记/动作日志的**回放投影**（写侧见同批实装的三个 POST 端点）。
/// 载荷用上游 `parse_*_payload` 逐字读回；家庭参考区间用上游 `home_ref_range`
/// （**查不到出处就不给区间**）；`kind`/`from`/`to` 真过滤（非法日期 400 带原因）。
#[utoipa::path(get, path = "/api/v1/persons/{pid}/measurements", tag = "records",
    params(("pid" = String, Path, description = "person uuid"),
           ("kind" = Option<String>, Query, description = "只看某一类：self_measurement|note|profile_event"),
           ("from" = Option<String>, Query, description = "起始日期(含) YYYY-MM-DD"),
           ("to" = Option<String>, Query, description = "结束日期(含) YYYY-MM-DD")),
    responses((status = 200, description = "replayed manual entries (self-measurement / note / profile-event)", body = entries::MeasurementsReply),
               (status = 400, description = "bad kind / bad date / from>to"),
               (status = 404, description = "person not found")))]
async fn get_measurements(
    State(st): State<Arc<AppState>>,
    AxPath(pid): AxPath<String>,
    Query(qp): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<entries::MeasurementsReply>, ApiError> {
    let q = entries::MeasurementsQuery {
        kind: qp.get("kind").cloned(),
        from: qp.get("from").cloned(),
        to: qp.get("to").cloned(),
    };
    tokio::task::block_in_place(|| {
        require_person(&st, &pid)?;
        let vault = vaults::open_person_vault(&st.home, &pid).map_err(ApiError::from)?;
        let reply = entries::measurements_projection(&vault, &pid, &q)
            .map_err(entry_err_to_api)?;
        Ok(Json(reply))
    })
}

// ---------- handlers（写侧·手动条目：self-measurement / note / profile-event，D-028）----------
// 三类「没有原件的记录」在 core-model 里**没有新事件变体**：一律走「合成文本当文件
// 过一遍 Vault::import」的手法（上游 apps/mobile_flutter 的 add_synthetic_document）。
// 本层照抄**同一条**写入路径与**同一批**上游判据（parser::render_*_text /
// validate_self_measured_values），不另造格式 ⇒ 写入方与读取方是同一对代码。
// 判据分工见 src/entries.rs 顶部注释（D5：不推断 = 不粉饰）。

/// 写一条自测记录（结构化、append-only）。血压是**两个值共享一条记录**。
/// 生理学不可能的值（138388 mmHg / 收缩压≤舒张压）⇒ 400 并给出原因。
#[utoipa::path(post, path = "/api/v1/persons/{pid}/entries/self-measurement", tag = "entries",
    params(("pid" = String, Path, description = "person uuid")),
    request_body = entries::SelfMeasurementIn,
    responses((status = 201, description = "self-measurement recorded", body = entries::EntryReply),
               (status = 400, description = "empty values / implausible value / bad taken_at"),
               (status = 404, description = "person not found")))]
async fn post_self_measurement(
    State(st): State<Arc<AppState>>,
    AxPath(pid): AxPath<String>,
    Json(req): Json<entries::SelfMeasurementIn>,
) -> Result<(StatusCode, Json<entries::EntryReply>), ApiError> {
    tokio::task::block_in_place(|| {
        require_person(&st, &pid)?;
        let _serial = vaults::LockToken::acquire(&pid); // 写侧：与上传同一把「每人串行」锁
        let vault = vaults::open_person_vault(&st.home, &pid).map_err(ApiError::from)?;
        let reply = entries::add_self_measurement(&vault, &req).map_err(entry_err_to_api)?;
        tracing::info!(person = %pid, doc = reply.document_id, sha = %reply.content_hash, "self-measurement recorded");
        Ok((StatusCode::CREATED, Json(reply)))
    })
}

/// 写一条纯文本笔记。原文即内容（不解析、不关联用药/诊断）。
#[utoipa::path(post, path = "/api/v1/persons/{pid}/entries/note", tag = "entries",
    params(("pid" = String, Path, description = "person uuid")),
    request_body = entries::NoteIn,
    responses((status = 201, description = "note recorded", body = entries::EntryReply),
               (status = 400, description = "empty text / bad taken_at"),
               (status = 404, description = "person not found")))]
async fn post_note(
    State(st): State<Arc<AppState>>,
    AxPath(pid): AxPath<String>,
    Json(req): Json<entries::NoteIn>,
) -> Result<(StatusCode, Json<entries::EntryReply>), ApiError> {
    tokio::task::block_in_place(|| {
        require_person(&st, &pid)?;
        let _serial = vaults::LockToken::acquire(&pid);
        let vault = vaults::open_person_vault(&st.home, &pid).map_err(ApiError::from)?;
        let reply = entries::add_note(&vault, &req).map_err(entry_err_to_api)?;
        tracing::info!(person = %pid, doc = reply.document_id, sha = %reply.content_hash, "note recorded");
        Ok((StatusCode::CREATED, Json(reply)))
    })
}

/// 写一条病程动作日志（如「开启病程档案」「停药」）。`at` 是**事件发生的那天**
/// （YYYY-MM-DD，用户说的那天），`payload` 必须是 JSON 对象。
#[utoipa::path(post, path = "/api/v1/persons/{pid}/entries/profile-event", tag = "entries",
    params(("pid" = String, Path, description = "person uuid")),
    request_body = entries::ProfileEventIn,
    responses((status = 201, description = "profile event recorded", body = entries::EntryReply),
               (status = 400, description = "bad at / payload not an object"),
               (status = 404, description = "person not found")))]
async fn post_profile_event(
    State(st): State<Arc<AppState>>,
    AxPath(pid): AxPath<String>,
    Json(req): Json<entries::ProfileEventIn>,
) -> Result<(StatusCode, Json<entries::EntryReply>), ApiError> {
    tokio::task::block_in_place(|| {
        require_person(&st, &pid)?;
        let _serial = vaults::LockToken::acquire(&pid);
        let vault = vaults::open_person_vault(&st.home, &pid).map_err(ApiError::from)?;
        let reply = entries::add_profile_event(&vault, &req).map_err(entry_err_to_api)?;
        tracing::info!(person = %pid, doc = reply.document_id, sha = %reply.content_hash, kind = %req.kind, "profile event recorded");
        Ok((StatusCode::CREATED, Json(reply)))
    })
}

/// 写侧错误 → HTTP 码：**400 带原因**（参数不合法，agent 知道怎么改），
/// **500**（内部故障）。不把 400 混报成 500（D-022 的 401/403 分家同款纪律）。
fn entry_err_to_api(e: entries::EntryError) -> ApiError {
    match e {
        entries::EntryError::BadRequest(m) => ApiError(StatusCode::BAD_REQUEST, m),
        entries::EntryError::Internal(e) => ApiError(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")),
    }
}

// ---------- handler（审计视图：D-030）----------
/// 审计视图：两层真相层合并（**口径分层**，不假装只有一层）。
/// - `source="registry"`：person registry 的 append-only 原始事件（create/update/delete，**不折叠**）。
/// - `source="vault"`：上游 `Vault::audit_log()` 的投影（导入/导出/分享/删除）——**复用上游判据，不另写一份**。
/// 参数 `person`/`op` 是**真过滤**（真进判据，不是摆设）。
#[utoipa::path(get, path = "/api/v1/audit", tag = "system",
    params(("person" = Option<String>, Query, description = "只看某人（person_id）"),
           ("op" = Option<String>, Query, description = "只看某动作：create|update|delete（registry 侧）或 导入|导出|分享|删除（vault 侧）")),
    responses((status = 200, description = "audit entries (newest first)", body = audit::AuditReply)))]
async fn get_audit(
    State(st): State<Arc<AppState>>,
    Query(qp): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<audit::AuditReply>, ApiError> {
    let q = audit::AuditQuery {
        person: qp.get("person").cloned(),
        op: qp.get("op").cloned(),
    };
    tokio::task::block_in_place(|| audit::audit_view(&st.home, &q).map_err(ApiError::from)).map(Json)
}

// ---------- stubs（契约先行，随阶段替换实现）----------
macro_rules! not_implemented {
    ($name:ident, $msg:expr) => {
        async fn $name() -> Response {
            ApiError(StatusCode::NOT_IMPLEMENTED, $msg.to_string()).into_response()
        }
    };
}
/// D-023 实装（此前 stub→501 毕业）：LLM 结构化抽取回写。
/// result_json 必须是 deid schema v1（字段级 evidence = 原文逐字子串，上游 deid::verify 红线）。
/// 落库的是**校验后**的 extraction（判假字段已被上游清空/标 unverified），不是原样交来的 JSON。
#[derive(utoipa::IntoParams)]
#[into_params(parameter_in = Path)]
pub struct ShaPath { pub sha: String }

#[utoipa::path(
    put, path = "/api/v1/persons/{pid}/documents/{sha}/extraction",
    tag = "records", request_body = vaults::ExtractionNew, responses(
        (status = 200, description = "extraction stored (verified=字段级逐字闸全过)", body = vaults::ExtractionReply),
        (status = 400, description = "malformed sha / bad mode / result_json fails deid schema / document has no text layer"),
        (status = 404, description = "no person or no document for sha"),
    )
)]
async fn put_document_extraction(
    State(st): State<Arc<AppState>>,
    AxPath((pid, sha)): AxPath<(String, String)>,
    body: String,
) -> Result<Json<vaults::ExtractionReply>, ApiError> {
    // 手动反序列化(与 mcp_endpoint 同理：泛型 Json extractor 与本路由组合 Handler bound 冲突)
    let body: vaults::ExtractionNew = serde_json::from_str(&body)
        .map_err(|e| ApiError(StatusCode::BAD_REQUEST, format!("bad json body: {e}")))?;
    if body.result_json.trim().is_empty() {
        return Err(ApiError(StatusCode::BAD_REQUEST, "result_json must not be empty".into()));
    }
    let _serial = vaults::LockToken::acquire(&pid); // 写侧：与上传同一把「每人串行」锁
    tokio::task::block_in_place(|| {
        require_person(&st, &pid)?;
        let vault = vaults::open_person_vault(&st.home, &pid).map_err(ApiError::from)?;
        // 领域错误 → HTTP 状态码的映射**只此一处**（靠错误**类型**，不靠错误字符串内容）。
        let reply = vaults::add_extraction(&vault, &sha, body).map_err(|e| match e {
            vaults::ExtractionError::NoDocument(_) => ApiError(StatusCode::NOT_FOUND, e.to_string()),
            other => ApiError(StatusCode::BAD_REQUEST, other.to_string()),
        })?;
        tracing::info!(person = %pid, sha = %sha, verified = reply.verified, "extraction stored");
        Ok(Json(reply))
    })
}
// 人员域三 stub：路由即契约(tests MOUNTED 表守卫)。刻意不进 CONTRACT/openapi——{pid} 模板与已文档化的
// /persons(get+post)同 path 不同 op，utoipa derive 无法拆分挂载；待 GET/PATCH/DELETE 实装时一并入文档。
// ---- T46：token 管理面（Web 端生成/查询/吊销）——整体要求 rw scope（ro 管理 token = 提权）----
#[derive(serde::Deserialize, utoipa::ToSchema)]
pub struct TokenCreateReq {
    pub name: String,
    #[serde(default = "default_rw")]
    pub scope: String, // "ro"|"rw"
}
fn default_rw() -> String { "rw".into() }

fn extract_bearer(headers: &axum::http::HeaderMap) -> String {
    headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).unwrap_or("").to_string()
}

fn require_rw(st: &AppState, bearer: &str) -> Result<(), ApiError> {
    let Some((_name, scope)) = st.auth.authenticate(bearer) else {
        return Err(ApiError(StatusCode::UNAUTHORIZED, "invalid bearer token".into()));
    };
    if scope != auth::Scope::Rw {
        return Err(ApiError(StatusCode::FORBIDDEN, "token management requires rw scope".into()));
    }
    Ok(())
}

#[utoipa::path(post, path = "/api/v1/tokens", tag = "tokens",
    request_body = TokenCreateReq, responses(
        (status = 200, description = "token created; 明文只在本次回执出现", body = TokenCreated),
        (status = 401), (status = 403, description = "ro token cannot manage tokens")))]
async fn create_token(
    State(st): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    body: String,
) -> Result<Json<TokenCreated>, ApiError> {
    let bearer = extract_bearer(&headers);
    require_rw(&st, &bearer)?;
    let req: TokenCreateReq = serde_json::from_str(&body)
        .map_err(|e| ApiError(StatusCode::BAD_REQUEST, format!("bad json: {e}")))?;
    let name = req.name.trim();
    if name.is_empty() || name.len() > 64 {
        return Err(ApiError(StatusCode::BAD_REQUEST, "name must be 1..=64 chars".into()));
    }
    let scope = auth::Scope::parse(&req.scope)
        .map_err(|e| ApiError(StatusCode::BAD_REQUEST, e))?;
    if st.auth.list().iter().any(|t| t.name == name) {
        return Err(ApiError(StatusCode::CONFLICT, format!("token name {name:?} already exists")));
    }
    let (token, entry) = st.auth.create(name, scope)
        .map_err(|e| ApiError(StatusCode::CONFLICT, e))?;
    tracing::info!(token_name = %name, "token created via API");
    Ok(Json(TokenCreated { token, name: entry.name, scope: entry.scope, at: entry.at }))
}

#[derive(serde::Serialize, utoipa::ToSchema)]
pub struct TokenCreated {
    pub token: String,
    pub name: String,
    pub scope: auth::Scope,
    pub at: String,
}

#[derive(serde::Serialize, utoipa::ToSchema)]
pub struct TokenListReply { pub tokens: Vec<auth::TokenListEntry> }

#[utoipa::path(get, path = "/api/v1/tokens", tag = "tokens", responses(
    (status = 200, body = TokenListReply),
    (status = 401), (status = 403, description = "ro token cannot list tokens")))]
async fn list_tokens(
    State(st): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
) -> Result<Json<TokenListReply>, ApiError> {
    let bearer = extract_bearer(&headers);
    require_rw(&st, &bearer)?;
    Ok(Json(TokenListReply { tokens: st.auth.list() }))
}

#[utoipa::path(delete, path = "/api/v1/tokens/{name}", tag = "tokens",
    params(("name" = String, Path, description = "token name")),
    responses((status = 200, description = "revoked (or already absent)"),
        (status = 401), (status = 403)))]
async fn revoke_token(
    State(st): State<Arc<AppState>>,
    AxPath(name): AxPath<String>,
    headers: axum::http::HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    let bearer = extract_bearer(&headers);
    let (_caller, scope) = st.auth.authenticate(&bearer)
        .ok_or_else(|| ApiError(StatusCode::UNAUTHORIZED, "invalid bearer token".into()))?;
    if scope != auth::Scope::Rw {
        return Err(ApiError(StatusCode::FORBIDDEN, "token management requires rw scope".into()));
    }
    let existed = st.auth.revoke(&name).map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    tracing::info!(token_name = %name, existed, "token revoked via API");
    Ok(Json(serde_json::json!({"revoked": existed, "name": name})))
}

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
    paths(healthz, get_capabilities, list_persons, create_person, get_timeline, get_search, get_labs, get_medications, get_conditions, upload_document, list_documents,
          get_document_detail, get_document_original, put_document_extraction, create_token, list_tokens, revoke_token,
          get_measurements, post_self_measurement, post_note, post_profile_event,
          get_audit),
    components(schemas(Person, PersonNew, HealthzReply, capabilities::CapabilityManifest, capabilities::CapabilityRouteOut,
                       vaults::TimelineItem, vaults::TimelineReply,
                       vaults::SearchItem, vaults::SearchReply,
                       vaults::DocumentListItem, vaults::DocumentListReply,
                       vaults::UploadReply,
                       vaults::DocumentDetail, vaults::TextLayerMeta, vaults::ExtractionPass,
                       vaults::ExtractionNew, vaults::ExtractionReply,
                       vaults::LabPointOut, vaults::LabSeriesOut, vaults::LabsReply,
                       vaults::MedMentionOut, vaults::MedSpanOut, vaults::MedicationsReply,
                       vaults::ConditionOut, vaults::ConditionsReply,
                       entries::SelfMeasuredValueIn, entries::SelfMeasurementIn,
                       entries::NoteIn, entries::ProfileEventIn, entries::EntryReply,
                       entries::MeasurementsQuery, entries::MeasurementsReply,
                       entries::MeasurementOut, entries::MeasurementValueOut,
                       audit::AuditQuery, audit::AuditReply, audit::AuditEntryOut))
)]
struct ApiDoc;

/// 已注册但未实现的端点契约：(method, path, tag, summary) —— router() 必须逐条覆盖此表
///
/// ⚠️ **入表即宣称「未实装（501）」**。端点一旦实装（有自己的 handler + utoipa path），
/// 必须**从本表移除**，否则 OpenAPI 会把它标成 stub→501，而实际返回 200 —— 文档与现实脱节，
/// 正是本项目最贵的漏账形态（D5：状态码不许粉饰）。反向保险见 tests/contract.rs
/// （逐条断言本表里的端点真返回 501）。
/// stub 端点在 OpenAPI summary 里的**唯一标记**（D-016/D-025 纪律：同一事实只留一处实现）。
/// 改这一处 ⇒ 文档渲染、派生清单、门禁脚本的判定口径同时跟着变，不会各说各话。
pub const STUB_MARK: &str = "stub→501";

/// ⚠️ **当前为空**：D-023 实装了最后一个 stub 端点（`PUT /persons/{pid}/documents/{sha}/extraction`）
/// ⇒ 它已从本表移除（入表即宣称 501，而它真返回 200/400/404）。
/// **表空不是「没人管」**：反向保险是 tests/contract.rs + tests/apidoc.rs 两条门禁
/// （「CONTRACT 里的端点必须真返回 501」+「OpenAPI 每个 operation 必须真挂路由」）——
/// 端点实装后忘删表 ⇒ 那两条门禁立刻红（不是「怎么都绿」）。
pub const CONTRACT: &[(&str, &str, &str, &str)] = &[];

/// 已挂 router 但**刻意不进 OpenAPI** 的人员域端点（理由见 stub_person_* 注释）。
/// 单一事实源：tests/contract.rs 与 rest_manifest() 都读这张表，不再各写一份（D2：一个语义只允许一处实现）。
pub const MOUNTED_UNDOCUMENTED: &[(&str, &str)] = &[
    ("get", "/api/v1/persons/{pid}"),
    ("patch", "/api/v1/persons/{pid}"),
    ("delete", "/api/v1/persons/{pid}"),
];

fn stub_item(op: &str, tag: &str, summary: &str) -> Value {
    json!({ op.to_lowercase(): {
        "summary": format!("{summary}（{STUB_MARK}）"),
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

/// 合成 OpenAPI 时追加的 securitySchemes（T4）。
/// 刻意手写注入而非 utoipa derive 的 modifier：注入点在 openapi_json() 一处，
/// 可被 tests/auth.rs 直接断言（避免"文档声称有鉴权、实际没有"这类漏账，D5）。
fn inject_security_schemes(doc: &mut Value) {
    let comps = doc
        .get_mut("components")
        .and_then(|c| c.as_object_mut())
        .expect("components object");
    comps.insert(
        "securitySchemes".to_string(),
        json!({
            "bearerAuth": {
                "type": "http",
                "scheme": "bearer",
                "description": "静态 Bearer token（env 注入）。scope: ro=只读 / rw=读写。EHR_API_TOKENS 逗号分隔，格式 <token>[:ro|rw] 或 <name>=<token>[:ro|rw]。"
            }
        }),
    );
    // 全局安全要求：除公开路径（healthz/openapi.json/首页）外都需要 bearerAuth。
    // 公开路径在各自 operation 上用 security: [] 覆盖。
    doc["security"] = json!([{ "bearerAuth": [] }]);
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
    inject_security_schemes(&mut doc);
    // 公开路径显式声明 security: [] （免鉴权），否则全局 bearerAuth 会误导客户端去取 token
    {
        let paths = doc["paths"].as_object_mut().expect("paths object");
        if let Some(ops) = paths.get_mut("/api/v1/healthz").and_then(|o| o.as_object_mut()) {
            for (_, op) in ops.iter_mut() {
                if let Some(op) = op.as_object_mut() {
                    op.insert("security".to_string(), json!([]));
                }
            }
        }
    }
    doc
}

/// REST 面的**派生清单**（T16 · D-032 未销账⑩ 的治本）。
///
/// 为什么要它：此前「本服务有 16 条路径 / 18 个 operation」这个数字**硬编码在 5 处**
/// （ehtest.sh / mcp-e2e.sh / container-smoke.sh / tests/mcp.rs / 两份 skill 文档），
/// 而真源（运行时 OpenAPI）就在同一份代码里——**同一事实五份实现**（D-009 判据形状的老问题）。
/// 加一条路由要同步改五处，漏一处就红，且红得看不出是「漏改」还是「真坏了」。
///
/// 现在：实现是唯一事实源 ⇒ 派生 `rest_manifest()`；门禁与文档**从它取值**。
/// 数字仍在文档里（人读需要，「16 路径 / 18 operation」是读者要的上下文），
/// 但**由门禁核对**（照抄 D-032 决策③：核对 > 消除）。
///
/// 字段：
/// - `paths` / `operations`：OpenAPI 里的路径数与 operation 数；
/// - `stubs`：stub→501 的 operation 数（照抄 T16 真源 `STUB_MARK`）；
/// - `paths_list`：`METHOD path` 清单（**有序**，与 OpenAPI 一致）——门禁逐条比对，不只比数量；
/// - `mounted_undocumented`：已挂路由但刻意不进 OpenAPI 的端点（人员域三个 stub）。
pub fn rest_manifest() -> Value {
    let doc = openapi_json();
    let paths = doc["paths"].as_object().cloned().unwrap_or_default();
    let mut paths_list: Vec<String> = Vec::new();
    let mut operations = 0usize;
    let mut stubs = 0usize;
    // D-037：哪些 operation **仍被宣称 501**（逐条列出，不只是个数）——
    // 此前这个事实只能由门禁脚本**手写正则解析本文件源码**去猜
    // （`pub const CONTRACT[^=]*=\s*&\[(.*?)\n\];`）⇒ 同一事实两种实现，
    // 且那份正则**在 CONTRACT 变空后会跨表匹配到 MOUNTED_UNDOCUMENTED** ⇒
    // 「某端点不在 CONTRACT 表」这类门实际在查**错误的表**（假门）。
    // 现在：判据由实现派生并**公布具体清单**，门禁读清单即可（一份事实、一个来源）。
    let mut stub_paths: Vec<String> = Vec::new();
    for (p, item) in &paths {
        if let Some(obj) = item.as_object() {
            for (method, op) in obj {
                if !op.is_object() {
                    continue;
                }
                operations += 1;
                let summary = op.get("summary").and_then(|s| s.as_str()).unwrap_or("");
                if summary.contains(STUB_MARK) {
                    stubs += 1;
                    stub_paths.push(format!("{} {}", method.to_uppercase(), p));
                }
                paths_list.push(format!("{} {}", method.to_uppercase(), p));
            }
        }
    }
    paths_list.sort();
    stub_paths.sort();
    // D-021：CONTRACT 常量的**逐条清单**（源码真源），与上面的 `stub_paths`
    // （**渲染后 OpenAPI** 真源）逐条对账。集合判据，不是计数判据（理由见字段注释）。
    let mut contract_paths: Vec<String> = CONTRACT
        .iter()
        .map(|(m, p, _t, _s)| format!("{} {}", m.to_uppercase(), p))
        .collect();
    contract_paths.sort();
    json!({
        "paths": paths.len(),
        "operations": operations,
        "stubs": stubs,
        // 逐条列出「仍被宣称 501」的 operation ⇒ 门禁不必再手写正则解析源码（D-037）
        "stub_paths": stub_paths,
        // D-021：CONTRACT 表的**逐条清单**也公布出来 ⇒ 门禁不必再**手写正则解析 lib.rs 源码**
        // 去数这张表（D-037 当时保留了那半截正则做「两源对账」；现在两源在实现侧汇成一处：
        //   ①`contract_paths` = Rust 常量 CONTRACT 的 operation 清单（**源码真源**）；
        //   ②`stub_paths`     = 运行时 OpenAPI 里带 STUB_MARK 的 operation 清单（**渲染真源**）。
        // 这两个清单**来源不同**（一个是常量，一个是 OpenAPI 合成后的产物）⇒ 逐条相等是
        // **真判据**：若 CONTRACT 里登记了某端点但 merge_path 没把它合进 OpenAPI
        // （或合进去了却没带 STUB_MARK），两份清单立刻分叉 ⇒ 门禁红。
        //
        // ⚠️ 反面教材（本轮实撞的坑）：先写的是 `contract_len`（= CONTRACT.len()）与
        //   `stubs`（= OpenAPI 里数 STUB_MARK）两个**计数**并断言相等——**那是同义反复**
        //   （两者都从同一张 CONTRACT 表派生 ⇒ 永远相等 ⇒ 永远绿）。
        //   负向实验：往 CONTRACT 里加一条**合法**条目 ⇒ 两个计数**同步 +1** ⇒ 断言照样绿
        //   ⇒ 证明它测不出任何东西（D-037「绿但测错对象」的最隐蔽形态）。
        //   ⇒ **计数不是判据，集合才是**（D-009 判据形状原则的又一次应验）。
        "contract_paths": contract_paths,
        // 把标记字符串本身也公布出去 ⇒ 门禁脚本**不用再手抄**「stub→501」这个字面量
        // （同一事实两份实现的又一处：apidoc.rs 曾经也手抄过一份，现已改读 STUB_MARK）。
        "stub_mark": STUB_MARK,
        "paths_list": paths_list,
        "mounted_undocumented": MOUNTED_UNDOCUMENTED
            .iter()
            .map(|(m, p)| format!("{} {}", m.to_uppercase(), p))
            .collect::<Vec<_>>(),
    })
}
// 选型理由（PLAN 阶段3 待决项「Next 服务选型」在此钉死为“不引入 Node 工具链”）：
// 本机 node/npm 均不存在（实测 command not found），引入 Next/React 会让镜像构建依赖
// 外网 npm registry + 宿主工具链；最小版的管理需求（建档/选人/时间线/文档上传/指标/检索）
// 全部已有 JSON API，纯前端零依赖即可覆盖。将来要换正式前端，只需把 web/ 换成真站点，
// API 契约（/openapi.json, paths=16）不动。
const WEB_INDEX_HTML: &str = include_str!("../web/index.html");

async fn web_index() -> Response {
    (
        StatusCode::OK,
        [(
            axum::http::header::CONTENT_TYPE,
            "text/html; charset=utf-8",
        )],
        WEB_INDEX_HTML,
    )
        .into_response()
}


// ---------- T4 鉴权中间件（统一拦截层） ----------
/// 请求作用域（由 auth_layer 注入 request extensions；MCP 侧按工具粒度二次判定）
#[derive(Clone, Copy, Debug)]
pub struct AuthScope(pub auth::Scope);

/// 从 request extensions 取本次请求的 token 作用域；取不到 ⇒ open（未走中间件的直接调用场景）
pub fn scope_of(parts: &axum::http::request::Parts) -> auth::Scope {
    parts
        .extensions
        .get::<AuthScope>()
        .map(|s| s.0)
        .unwrap_or(auth::Scope::Rw)
}

/// 无 token ⇒ 401；token 无效 ⇒ 401；token 有效但 scope 不足 ⇒ 403。
/// D5：401（没凭证）与 403（凭证不够）必须分开，否则 agent 不知道该「补 token」还是「换 token」。
fn unauthorized(msg: &str) -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(header::WWW_AUTHENTICATE, "Bearer")],
        Json(json!({ "error": msg })),
    )
        .into_response()
}

fn forbidden(msg: &str) -> Response {
    (StatusCode::FORBIDDEN, Json(json!({ "error": msg }))).into_response()
}

/// 免鉴权路径：健康检查（部署自检用）、OpenAPI 文档、Web 管理端首页。
/// 其余全部要 token（含 /mcp）。
fn is_public_path(path: &str) -> bool {
    matches!(path, "/api/v1/healthz" | "/openapi.json" | "/")
}

/// 重建守卫中间件（D-024）：派生索引正在被重建时，**拒绝一切业务读写**（503 + Retry-After）。
///
/// 为什么连读也拒：重建会**删掉** medme.db 再回放重建，期间的读可能命中半写状态或
/// 瞬时"表还在但内容是旧的"。让客户端收到明确 503 并重试，比让它读到不一致的派生态更诚实（D5：不假装）。
/// 公开路径（healthz/openapi.json/首页）放行——它们不碰派生层。
async fn rebuild_guard(
    State(st): State<Arc<AppState>>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let path = req.uri().path().to_string();
    if !is_public_path(&path) {
        if let Some(reason) = rebuildlock::block_reason(&st.home) {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                [(header::RETRY_AFTER, "5")],
                Json(json!({ "error": reason })),
            )
                .into_response();
        }
    }
    next.run(req).await
}

/// 鉴权中间件：open 模式直接放行；否则必须带合法 Bearer。
/// 写操作（POST/PUT/PATCH/DELETE）额外要求 rw scope（ro token ⇒ 403）。
/// /mcp 例外：HTTP 方法是 POST，但权限按 **工具** 判定（读侧工具 ro 即可）⇒ 交由 mcp.rs 内二次判定。
async fn auth_layer(
    State(st): State<Arc<AppState>>,
    mut req: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    if st.auth.is_open() {
        return next.run(req).await;
    }
    let path = req.uri().path().to_string();
    if is_public_path(&path) {
        return next.run(req).await;
    }
    let Some(bearer) = req.headers().get(header::AUTHORIZATION).and_then(|v| v.to_str().ok())
    else {
        return unauthorized("missing Authorization: Bearer <token>");
    };
    let Some((_name, scope)) = st.auth.authenticate(bearer) else {
        return unauthorized("invalid bearer token");
    };
    // /mcp：仅校验 token 有效，工具粒度权限在 mcp.rs 判定（ro 允许读侧工具）
    let is_mcp = path == "/mcp";
    if !is_mcp {
        let method = req.method().as_str();
        if !scope.allows(method) {
            return forbidden(&format!(
                "token scope '{}' does not allow {} (need rw)",
                scope.as_str(),
                method
            ));
        }
    }
    req.extensions_mut().insert(AuthScope(scope));
    next.run(req).await
}

// ---------- router（判据：新增路由必同时进 CONTRACT —— 反向保险见 tests/contract.rs）----------
async fn openapi_route() -> Json<Value> {
    Json(openapi_json())
}

/// POST /mcp —— MCP(Streamable HTTP, 无 session) 入口。刻意 **不进 /openapi.json**：
/// 它不是 REST 契约面而是 JSON-RPC 协议面；工具契约由 tools/list 自身公布 + tests/mcp.rs 守卫。
async fn mcp_endpoint(
    State(st): State<Arc<AppState>>,
    req: axum::extract::Request,
) -> Result<Json<Value>, ApiError> {
    // scope 从 request extensions 取（鉴权中间件注入）；open 模式 ⇒ Rw（放行一切，与旧行为一致）
    let (parts, body) = req.into_parts();
    let scope = scope_of(&parts);
    let Json(body) = Json::<Value>::from_request(
        axum::http::Request::from_parts(parts, body),
        &st,
    )
    .await
    .map_err(|e| {
        // T39：**超限**必须报 413，不能混进 400。
        //   实测缺陷：MCP 面 `upload_document` 宣称可传 32MB（工具自己的判据），
        //   但 JSON body 仍走 **axum 默认 2MB** ⇒ 实测 base64 长度 2_666_668 字节时
        //   HTTP 层直接 400「length limit exceeded」⇒ **宣称的上限根本不可达**
        //   （而且 400 会让 agent 以为是自己 JSON 写错了 ⇒ 改参数重试 ⇒ 永远失败）。
        //   ⇒ 判据：axum 的 `JsonRejection::status()` 对「body 超长」给 413（LengthLimitError）
        //   ⇒ 采信它；其余（语法错/缺 content-type）仍按 400（保持既有语义不变）。
        // ⚠️ 局部名用 `code`（不是 `st`）——`st` 已被 `State(st)` 占用（同名会遮蔽状态）。
        let code = e.status();
        if code == StatusCode::PAYLOAD_TOO_LARGE {
            let max_bytes = st.max_upload_bytes;
            ApiError(
                code,
                format!(
                    "request body too large（单件上传上限 {max_bytes} 字节；base64 编码后约 {} 字节；上限见 GET /api/v1/capabilities 的 max_upload_bytes）",
                    max_bytes * 4 / 3 + 1024 * 1024
                ),
            )
        } else {
            ApiError(StatusCode::BAD_REQUEST, format!("invalid JSON body: {e}"))
        }
    })?;
    // JSON 解析失败由 axum 的 Json 拒绝器回 400（非 JSON-RPC 形状）——协议层与 REST 层边界诚实：
    // 客户端发错 content-type/体时，2025-06-18 允许 HTTP 层先行拒绝。
    Ok(Json(mcp::handle_http_with_scope(&st, &body, scope)))
}

pub fn router(st: Arc<AppState>) -> Router {
    // T39-④：body limit **从状态里取**（不自己读 env）⇒ 路由层与 handler 层
    //   看到的**必然是同一个值** ⇒ 「清单说 32MB、实际 2MB 就 413」这种分叉**不可能发生**。
    let max_upload_bytes = st.max_upload_bytes;
    let api = Router::new()
        .route("/healthz", get(healthz))
        .route("/capabilities", get(get_capabilities)) // D-034：ingest 能力清单（真源=实现，从它派生）
        .route("/persons", get(list_persons).post(create_person))
        .route("/persons/{pid}", get(stub_person_detail)).route("/persons/{pid}", patch(stub_person_update)).route("/persons/{pid}", delete(stub_person_delete)) // 人员详情/修订/归档: pending(stage-2后半)——契约以路由为准(见 tests/contract.rs MOUNTED)
        .route("/persons/{pid}/timeline", get(get_timeline))
        .route("/persons/{pid}/search", get(get_search))
        .route(
            "/persons/{pid}/documents",
            get(list_documents).post(upload_document).layer(
                // T39：**显式**放宽 axum 默认的 ~2MB body limit（那不是本项目的上限，
                // 只是一个隐式默认值，且比 MCP 侧严 15 倍 ⇒ 同一动作两个上限）。
                // 这里设 = 单件上限 + multipart 头部开销余量；真正的**判据**仍是
                // `st.max_upload_bytes`（handler 内那行）⇒ 一处判据。
                axum::extract::DefaultBodyLimit::max(max_upload_bytes + 64 * 1024),
            ),
        )
        // hash docref 域已实装(从 CONTRACT stub 毕业)。两条显式路由，不用前缀通配(D7:多余后缀应404而非静默命中)。
        .route("/persons/{pid}/documents/{sha}", get(get_document_detail))
        .route("/persons/{pid}/documents/{sha}/original", get(get_document_original))
        .route("/persons/{pid}/labs", get(get_labs)) // D-011 实装毕业(此前 stub→501；期间一个旧二进制+孤儿进程伪装成501教训见D-011)
        .route("/persons/{pid}/medications", get(get_medications)) // D-026 实装毕业(此前 stub→501；入 CONTRACT 表前先实装，文档不粉饰状态码)
        .route("/persons/{pid}/conditions", get(get_conditions)) // D-027 实装毕业(此前 stub→501；与 labs/medications 同一次 aggregate)
        .route("/persons/{pid}/measurements", get(get_measurements)) // D-028 实装毕业(此前 stub→501；写侧已就位)
        .route("/persons/{pid}/entries/self-measurement", post(post_self_measurement)) // D-028 实装毕业(此前 stub→501)
        .route("/persons/{pid}/entries/note", post(post_note)) // D-028 实装毕业(此前 stub→501)
        .route("/persons/{pid}/entries/profile-event", post(post_profile_event)) // D-028 实装毕业(此前 stub→501)
        .route("/persons/{pid}/documents/{sha}/extraction", put(put_document_extraction)) // D-023 实装毕业(API-DESIGN 全路径含 pid)
        .route("/tokens", post(create_token).get(list_tokens)) // T46 token 管理面
        .route("/tokens/{name}", delete(revoke_token))
        .route("/audit", get(get_audit)) // D-030 实装毕业(此前 stub→501；vault 侧复用上游 audit_log()，registry 侧读原始事件)
        .with_state(st.clone());

    Router::new()
        .nest("/api/v1", api)
        .route("/openapi.json", get(openapi_route)) // 服务端每次现合成，与代码内 CONTRACT 常量必然同步（D1）
        .route("/", get(web_index)) // Web 管理端最小版：同源零依赖，避免跨域反代；刷新任意路由均回首页(D-016)
        // ⚠️ /mcp 必须挂在 nest("/api/v1") 之外：它是 MCP 协议面（客户端按规范打 /mcp），
        //   塞进 REST 命名空间会让 clients 走 /api/v1/mcp 找不到——本轮 404 实证。
        .route("/mcp", post(mcp_endpoint).layer(
            // T39：MCP 面要传 base64 化的原件（≈ 原始字节 × 4/3）⇒ body limit 必须
            // **按 base64 后的大小**给，否则「宣称 32MB 的上限」在 HTTP 层根本到不了
            // （实测：base64 长度 2_666_668 字节 ⇒ 400 length limit exceeded）。
            // ⇒ 这里给 = 单件上限的 base64 膨胀 + 余量；**判据**仍是
            // `st.max_upload_bytes`（工具层那行）⇒ 一处判据。
            axum::extract::DefaultBodyLimit::max(
                max_upload_bytes * 4 / 3 + 1024 * 1024,
            ),
        ))
        // 顶层 Router 无 state 参数 ⇒ 挂 /mcp 时须显式 with_state（内层 api 已消费 st，这里 clone 一个 Arc）
        .with_state(st.clone())
        // D-024：重建守卫层（重建进行中 ⇒ 业务路径 503）。放在鉴权层**内侧**：
        // 无 token 的请求仍先得 401（凭证问题优先），有 token 但正在重建才得 503。
        .layer(axum::middleware::from_fn_with_state(st.clone(), rebuild_guard))
        // T4：统一鉴权拦截层（open 模式下自动放行）。挂在最外层 ⇒ /mcp、/api/v1、/openapi.json 一次覆盖。
        .layer(axum::middleware::from_fn_with_state(st.clone(), auth_layer))
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
    Ok(Arc::new(AppState {
        store: PersonStore::new(home.clone()),
        home,
        auth: Arc::new(auth::TokenManager::empty()),
        max_upload_bytes: capabilities::max_upload_bytes_from_env(),
    }))
}

/// serve 用：home 校验 + 注入 env 里的 token 表。token 表格式错误 ⇒ 直接启动失败
/// （D5：配置错误必须在启动时暴露，不能默默退化成 open 模式）。
pub fn prepare_home_and_state_auth(
    home_in: &std::path::Path,
    mkdir_vaults: bool,
) -> Result<(Arc<AppState>, auth::TokenSet)> {
    let st = prepare_home_and_state(home_in, mkdir_vaults)?;
    let set = auth::TokenSet::from_env().map_err(|e| anyhow::anyhow!("bad token config: {e}"))?;
    // T46：env 种子 + registry/tokens.jsonl 回放（文件为准，运行期 Web 可变）
    let manager = auth::TokenManager::load(&st.home, set.clone())
        .map_err(|e| anyhow::anyhow!("token registry load: {e}"))?;
    let st = Arc::new(AppState {
        home: st.home.clone(),
        store: PersonStore::new(st.home.clone()),
        auth: Arc::new(manager),
        max_upload_bytes: capabilities::max_upload_bytes_from_env(),
    });
    Ok((st, set))
}
