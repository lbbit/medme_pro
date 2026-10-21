// medme-ehr · MCP (Model Context Protocol) 服务器 —— 阶段4 第一步。
//
// 设计取舍（先把不变量钉死，D-004~D-007 通道纪律同源）：
// ① **零新依赖**：手写 JSON-RPC 2.0 over HTTP（POST /mcp），不引 rmcp/mcp-sdk。
//    理由与 D-016 同形——本项目最值钱的不变量是「构建期零外网 / 依赖面最小」；
//    MCP 的协议面（initialize / tools/list / tools/call）小到可以自己钉死并被测试锁住。
//    将来若换官方 SDK，只需保持本文件的 tools 契约与 tests/mcp.rs 的断言不变。
// ② **无状态**：请求即完整 JSON-RPC 消息，响应即一条（Streamable HTTP 的无 session 模式）。
//    MCP 客户端每次 initialize 后可反复调 tools/call；服务端不持有 per-session 状态。
// ③ **薄适配层**：tools/call 直接复用 ehr_api::vaults 领域函数，与 REST handlers 同一份真相层。
//    **不存在「MCP 里能查、REST 里查不到」的分叉**（工具集与已实装域一一对应）。
// ④ **错误诚实**：领域错误 → JSON-RPC error（-32602 非法参数 / -32001 未知人 / -32002 内部），
//    D5：不把 501/404 混报成成功空结果。
use serde_json::{json, Value};
use std::sync::Arc;

use crate::{audit, auth::Scope, b64, entries, vaults, ApiError, AppState, PersonNew, StatusCode};

pub const PROTOCOL_VERSION: &str = "2025-06-18";

// ---------- JSON-RPC 错误码 ----------
const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;
/// 应用层错误（本项目自定义区间 -32000..-32099）
const ERR_NO_SUCH_PERSON: i64 = -32001;
const ERR_INTERNAL: i64 = -32002;
/// 写入侧的类型不支持（如 .dcm 需 ocr/dicom worker）——与「服务坏了」区分开，agent 据此换格式。
const ERR_UNSUPPORTED_TYPE: i64 = -32003;
/// 权限不足（只读 token 调写侧工具）——与「参数错了」区分开，agent 据此换 token 而不是改参数。
const ERR_FORBIDDEN: i64 = -32004;

/// 写侧单件上限（解码后字节数）。防止 agent 一口气塞 500MB 把内存打爆。
/// 32MB 覆盖绝大多数病历 PDF/影像元数据；超出即 -32602（改参数，不重试）。
const MAX_UPLOAD_BYTES: usize = 32 * 1024 * 1024;

fn err_obj(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn ok_obj(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

/// MCP 工具返回值一律包一层 `content: [{type:"text", text:<json>}]`（MCP 规范形状），
/// `structuredContent` 同载机读体——让「人读」与「agent 精确解析」各有其面（D7）。
fn tool_ok(id: Value, data: Value) -> Value {
    let text = serde_json::to_string_pretty(&data).unwrap_or_else(|_| data.to_string());
    ok_obj(id, json!({
        "content": [{ "type": "text", "text": text }],
        "structuredContent": data,
        "isError": false,
    }))
}

fn tool_err(id: Value, code: i64, message: String) -> Value {
    ok_obj(id, json!({
        "content": [{ "type": "text", "text": message }],
        "isError": true,
        "_code": code, // 非规范字段：便于 e2e 断言错误码（D4：断言要能看见真因）
    }))
}

/// 单个 JSON-RPC 消息 → 可选响应（notification 无 id ⇒ 返回 None）。
pub fn handle_message(st: &Arc<AppState>, msg: &Value) -> Option<Value> {
    handle_message_with_scope(st, msg, Scope::Rw)
}

/// 带请求作用域的消息处理。scope 来自鉴权中间件（ro token 只能调读侧工具）。
pub fn handle_message_with_scope(
    st: &Arc<AppState>,
    msg: &Value,
    scope: Scope,
) -> Option<Value> {
    let id = msg.get("id").cloned().unwrap_or(Value::Null);
    let is_notification = msg.get("id").is_none();
    let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let params = msg.get("params").cloned().unwrap_or(json!({}));

    let resp = match method {
        // 握手：声明能力（tools）。不宣告 resources/prompts —— 本项目暂不提供。
        "initialize" => ok_obj(id.clone(), json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": { "tools": { "listChanged": false } },
            "serverInfo": { "name": "medme-ehr", "version": env!("CARGO_PKG_VERSION") },
            "instructions": "多人电子健康档案工具集。读侧：person_id 为 UUID；先用 list_persons 拿人，再用 search_records / get_labs / get_medications / get_conditions / get_measurements / get_timeline / get_audit 查档。写侧：create_person 建档、upload_document 传原件（二进制用 base64）。所有内容来自原件 CAS+事件日志，text/extraction 透传不猜。",
        })),
        "ping" => ok_obj(id.clone(), json!({})),
        "notifications/initialized" | "initialized" => ok_obj(id.clone(), json!({})),
        "tools/list" => ok_obj(id.clone(), json!({ "tools": tool_schemas() })),
        "tools/call" => {
            let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            // 工具粒度权限：ro token 调写侧工具 ⇒ -32004（明确是权限问题，不是参数问题）
            if !scope.allows_tool(&name) {
                return Some(tool_err(
                    id.clone(),
                    ERR_FORBIDDEN,
                    format!(
                        "tool `{name}` requires a read-write token (current scope: {})",
                        scope.as_str()
                    ),
                ));
            }
            call_tool(st, id.clone(), &name, &args)
        }
        "" => err_obj(id.clone(), INVALID_REQUEST, "missing method"),
        other => err_obj(id.clone(), METHOD_NOT_FOUND, &format!("unknown method: {other}")),
    };
    if is_notification {
        None
    } else {
        Some(resp)
    }
}

/// POST /mcp 入口：批量(Batch)或单条；非法 JSON → -32700。
pub fn handle_http(st: &Arc<AppState>, body: &Value) -> Value {
    handle_http_with_scope(st, body, Scope::Rw)
}

/// 带作用域的 HTTP 入口（鉴权中间件把 token scope 传进来）。
pub fn handle_http_with_scope(st: &Arc<AppState>, body: &Value, scope: Scope) -> Value {
    let out: Vec<Value> = match body {
        Value::Array(items) => items.iter().filter_map(|m| handle_message_with_scope(st, m, scope)).collect(),
        single @ Value::Object(_) => handle_message_with_scope(st, single, scope).into_iter().collect(),
        _ => return err_obj(Value::Null, INVALID_REQUEST, "body must be a JSON-RPC object or array"),
    };
    if out.is_empty() {
        // 全是 notification ⇒ 202 无内容语义的等价物：空对象
        json!({})
    } else if out.len() == 1 {
        out.into_iter().next().expect("len==1")
    } else {
        Value::Array(out)
    }
}

/// 校验失败 = (错误码, 原因)。原因要能指导 agent 改参数，而不是让它盲目重试（D5）。
type ArgErr = (i64, String);

fn str_arg<'a>(args: &'a Value, k: &str) -> Result<&'a str, ArgErr> {
    args.get(k)
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .ok_or((INVALID_PARAMS, format!("`{k}` is required and must be a non-empty string")))
}

fn u64_arg(args: &Value, k: &str, default: u64) -> Result<u64, ArgErr> {
    match args.get(k) {
        None | Some(Value::Null) => Ok(default),
        Some(v) => v
            .as_u64()
            .ok_or((INVALID_PARAMS, format!("`{k}` must be a non-negative integer"))),
    }
}

/// 领域错误 → JSON-RPC 错误码。404 人档不存在单独成码（agent 需据此改选人，而非重试）。
fn domain_code(e: &ApiError) -> i64 {
    match e.0 {
        StatusCode::NOT_FOUND => ERR_NO_SUCH_PERSON,
        // 400 = 参数不合法（agent 该改参数，不是重试）——与 REST 面的 400 同义。
        StatusCode::BAD_REQUEST => INVALID_PARAMS,
        StatusCode::NOT_IMPLEMENTED => ERR_UNSUPPORTED_TYPE,
        _ => ERR_INTERNAL,
    }
}

/// 单个工具的执行体（在 block_in_place 临界区里跑：rusqlite/Vault 非 Send，同 REST 模式）。
fn call_tool(st: &Arc<AppState>, id: Value, name: &str, args: &Value) -> Value {
    // 参数校验放在 async 之外，先于任何 IO —— 非法参数不该打开任何文件。
    // 校验失败必须**带原因**（agent 要知道是名字空、base64 坏、还是超限），不能只丢一个码（D5）。
    let prepared = match prepare(name, args) {
        Ok(p) => p,
        Err((code, why)) => return tool_err(id, code, format!("invalid arguments for tool `{name}`: {why}")),
    };
    let outcome = tokio::task::block_in_place(|| match prepared {
        Prepared::Persons => st.store.list().map(|v| json!({ "count": v.len(), "persons": v })).map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}"))),
        Prepared::Timeline { ref pid } => {
            require_person(st, pid)?;
            let vault = vaults::open_person_vault(&st.home, pid)?;
            let items: Vec<vaults::TimelineItem> = vault
                .timeline()
                .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
                .into_iter()
                .map(vaults::TimelineItem::from)
                .collect();
            Ok(json!({ "person_id": pid, "count": items.len(), "items": items }))
        }
        Prepared::Search { ref pid, ref q, limit } => {
            require_person(st, pid)?;
            let vault = vaults::open_person_vault(&st.home, pid)?;
            let items: Vec<vaults::SearchItem> = vault
                .search(q, limit)
                .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
                .into_iter()
                .map(vaults::SearchItem::from)
                .collect();
            Ok(json!({ "person_id": pid, "q": q, "count": items.len(), "items": items }))
        }
        Prepared::Labs { ref pid } => {
            require_person(st, pid)?;
            let vault = vaults::open_person_vault(&st.home, pid)?;
            let reply = vaults::labs_aggregate(&vault, pid)
                .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            serde_json::to_value(reply).map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
        }
        Prepared::Medications { ref pid } => {
            require_person(st, pid)?;
            let vault = vaults::open_person_vault(&st.home, pid)?;
            let reply = vaults::meds_aggregate(&vault, pid)
                .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            serde_json::to_value(reply).map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
        }
        Prepared::Conditions { ref pid } => {
            require_person(st, pid)?;
            let vault = vaults::open_person_vault(&st.home, pid)?;
            let reply = vaults::conditions_aggregate(&vault, pid)
                .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            serde_json::to_value(reply).map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
        }
        Prepared::Measurements { ref pid, ref q } => {
            require_person(st, pid)?;
            let vault = vaults::open_person_vault(&st.home, pid)?;
            let reply = entries::measurements_projection(&vault, pid, q)
                .map_err(|e| ApiError(StatusCode::BAD_REQUEST, e.to_string()))?;
            serde_json::to_value(reply).map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
        }
        Prepared::Audit { ref q } => {
            // ⚠️ 审计是**全局视图**：不按 person 判存在（问「某人有哪些审计条目」答「没有」是合法答案，
            //    与 REST 面口径一致：未知人过滤 ⇒ 0 条，不是 404/500）。
            let reply = audit::audit_view(&st.home, q)
                .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")))?;
            serde_json::to_value(reply).map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
        }
        Prepared::Documents { ref pid } => {
            require_person(st, pid)?;
            let vault = vaults::open_person_vault(&st.home, pid)?;
            let items: Vec<vaults::DocumentListItem> = vaults::list_document_items(&vault)
                .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            Ok(json!({ "person_id": pid, "count": items.len(), "items": items }))
        }
        Prepared::DocumentDetail { ref pid, ref sha } => {
            require_person(st, pid)?;
            if !vaults::is_sha(sha) {
                return Err(ApiError(StatusCode::BAD_REQUEST, format!("malformed sha256: {sha}")));
            }
            let vault = vaults::open_person_vault(&st.home, pid)?;
            match vaults::document_detail(&vault, sha) {
                Ok(Some(d)) => serde_json::to_value(d)
                    .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
                Ok(None) => Err(ApiError(StatusCode::NOT_FOUND, format!("no indexed document: {sha}"))),
                Err(e) => Err(ApiError(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}"))),
            }
        }
        // ---------- 写侧（D-019）：建档 / 传原件 ----------
        Prepared::CreatePerson { ref req } => {
            let p = st.store.create(req).map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")))?;
            serde_json::to_value(p).map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
        }
        Prepared::UploadDocument { ref pid, ref filename, ref bytes } => {
            require_person(st, pid)?;
            let _serial = vaults::LockToken::acquire(pid); // 与 REST 上传同一把「每人写锁」
            let vault = vaults::open_person_vault(&st.home, pid)?;
            let reply = vaults::ingest_upload(&vault, filename, bytes).map_err(|e| {
                let es = format!("{e:#}");
                // 与 REST 面同语义：不支持的扩展名是契约内状态码(501)，不是 500（D5）
                if es.contains("unsupported extension") {
                    ApiError(StatusCode::NOT_IMPLEMENTED, es)
                } else {
                    ApiError(StatusCode::INTERNAL_SERVER_ERROR, es)
                }
            })?;
            serde_json::to_value(reply).map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
        }
    });
    match outcome {
        Ok(data) => tool_ok(id, data),
        Err(e) => tool_err(id, domain_code(&e), e.1),
    }
}

fn require_person(st: &AppState, pid: &str) -> Result<(), ApiError> {
    let found = st
        .store
        .list()
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")))?
        .into_iter()
        .any(|p| p.id == pid);
    if found {
        Ok(())
    } else {
        Err(ApiError(StatusCode::NOT_FOUND, format!("no such person (or archived): {pid}")))
    }
}

/// 参数已校验的执行计划（Prepared）：把「解析/校验」与「执行」分开，
/// 保证非法参数在任何 IO 之前就被拒（D4：先量后判，别让副作用先发生）。
enum Prepared {
    Persons,
    Timeline { pid: String },
    Search { pid: String, q: String, limit: usize },
    Labs { pid: String },
    /// D-029：用药跨度（与 REST 同源：同一 `meds_aggregate`）
    Medications { pid: String },
    /// D-029：病程泳道（与 REST 同源：同一 `conditions_aggregate`）
    Conditions { pid: String },
    /// D-029：手动条目回放投影（与 REST 同源：同一 `measurements_projection`）
    Measurements { pid: String, q: entries::MeasurementsQuery },
    /// D-031：审计视图（与 REST 同源：同一 `audit_view()`，不得另写一份判据）
    Audit { q: audit::AuditQuery },
    Documents { pid: String },
    DocumentDetail { pid: String, sha: String },
    /// 写侧：建档（person registry append-only 真相层）
    CreatePerson { req: PersonNew },
    /// 写侧：传原件（二进制以 base64 传，走与 REST 同一 ingest 路径）
    UploadDocument { pid: String, filename: String, bytes: Vec<u8> },
}

/// 字符串参数（可空）：缺省=None，非字符串⇒-32602。
fn opt_str_arg(args: &Value, k: &str) -> Result<Option<String>, ArgErr> {
    match args.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => {
            let s = v
                .as_str()
                .ok_or((INVALID_PARAMS, format!("`{k}` must be a string")))?;
            let t = s.trim();
            if t.is_empty() {
                Ok(None)
            } else {
                Ok(Some(t.to_string()))
            }
        }
    }
}

fn prepare(name: &str, args: &Value) -> Result<Prepared, ArgErr> {
    Ok(match name {
        "list_persons" => Prepared::Persons,
        "get_timeline" => Prepared::Timeline { pid: str_arg(args, "person_id")?.to_string() },
        "search_records" => Prepared::Search {
            pid: str_arg(args, "person_id")?.to_string(),
            q: str_arg(args, "q")?.to_string(),
            limit: u64_arg(args, "limit", 20)?.clamp(1, 100) as usize,
        },
        "get_labs" => Prepared::Labs { pid: str_arg(args, "person_id")?.to_string() },
        // D-029：与 REST 同名的读侧工具（同一真相层、同一判据，不另写一份）
        "get_medications" => Prepared::Medications { pid: str_arg(args, "person_id")?.to_string() },
        "get_conditions" => Prepared::Conditions { pid: str_arg(args, "person_id")?.to_string() },
        "get_measurements" => Prepared::Measurements {
            pid: str_arg(args, "person_id")?.to_string(),
            q: entries::MeasurementsQuery {
                kind: opt_str_arg(args, "kind")?,
                from: opt_str_arg(args, "from")?,
                to: opt_str_arg(args, "to")?,
            },
        },
        "list_documents" => Prepared::Documents { pid: str_arg(args, "person_id")?.to_string() },
        // D-031：审计视图——**与 REST 同一 `audit_view()`**（同一个语义只允许一处实现，D2）。
        // `person`/`op` 都是**可选**（缺省 = 全部人 / 全部动作）；空串按缺省处理。
        "get_audit" => Prepared::Audit {
            q: audit::AuditQuery {
                person: match opt_str_arg(args, "person")? {
                    Some(p) => Some(p),
                    // 别名：与其它工具的 `person_id` 同名习惯一致（同一语义两个键名，不是两份判据）
                    None => opt_str_arg(args, "person_id")?,
                },
                op: opt_str_arg(args, "op")?,
            },
        },
        "get_document_detail" => Prepared::DocumentDetail {
            pid: str_arg(args, "person_id")?.to_string(),
            sha: str_arg(args, "sha256")?.to_string(),
        },
        // ---------- 写侧 ----------
        "create_person" => {
            let name = str_arg(args, "name")?.to_string();
            let req = PersonNew {
                name,
                birthday: opt_str_arg(args, "birthday")?,
                gender: opt_str_arg(args, "gender")?,
                id_note: opt_str_arg(args, "id_note")?,
            };
            Prepared::CreatePerson { req }
        }
        "upload_document" => {
            let pid = str_arg(args, "person_id")?.to_string();
            let filename = str_arg(args, "filename")?.to_string();
            let b64s = str_arg(args, "content_base64")?.to_string();
            let bytes = b64::decode(&b64s)
                .map_err(|e| (INVALID_PARAMS, format!("`content_base64` is not valid base64: {e}")))?;
            if bytes.is_empty() {
                return Err((INVALID_PARAMS, "`content_base64` decoded to 0 bytes".into()));
            }
            if bytes.len() > MAX_UPLOAD_BYTES {
                return Err((
                    INVALID_PARAMS,
                    format!("decoded size {} exceeds limit {} bytes", bytes.len(), MAX_UPLOAD_BYTES),
                ));
            }
            Prepared::UploadDocument { pid, filename, bytes }
        }
        _ => return Err((METHOD_NOT_FOUND, format!("unknown tool: {name}"))), // 未公布的工具名（非参数问题）
    })
}

/// tools/list 公布的全部工具名 —— **单一事实来源**（与 `tool_schemas()` 同源派生）。
///
/// 用途：把「MCP 有多少个工具」从**三处硬编码的数字**（mcp-e2e.sh / skill-contract-check.sh /
/// tests/mcp.rs）变成**一处派生的事实**（D-031 新坑②）。门禁仍要断言数字，但断言的是
/// 「实现 == 契约文档」，不是「实现 == 上次的快照」——新增工具时改文档即可，不该三处同步改数字。
pub fn tool_names() -> Vec<String> {
    tool_schemas()
        .iter()
        .filter_map(|t| t.get("name").and_then(|n| n.as_str()).map(|s| s.to_string()))
        .collect()
}

/// 写侧工具名（其余均为读侧）。用于文档「读侧 N + 写侧 M」分栏核对。
/// **判据只此一处**：`Scope::allows_tool` 与此表必须一致（由 tests/mcp.rs 交叉断言）。
pub const WRITE_TOOLS: [&str; 2] = ["create_person", "upload_document"];

/// 工具清单的机器可读形态（`--print-mcp-tools` 干跑输出）。
///
/// 为什么要有它（D-031 新坑②）：「MCP 有多少个工具」此前被**硬编码三处**
/// （mcp-e2e.sh / skill-contract-check.sh / tests/mcp.rs）⇒ 加一个工具要改三处，
/// 漏一处就红（且红得看不出是「漏改」还是「真坏了」）。本函数把「工具清单」变成
/// **单一事实来源**：脚本与测试都从它取值，于是「加一个工具」只需改**实现 + 契约文档**两处。
///
/// 输出形状（JSON）：
/// `{"count":N,"read":N1,"write":N2,"names":[...],"read_names":[...],"write_names":[...]}`
pub fn tool_manifest() -> Value {
    let names = tool_names();
    let write: Vec<String> = WRITE_TOOLS.iter().map(|s| s.to_string()).collect();
    let read: Vec<String> = names.iter().filter(|n| !write.contains(n)).cloned().collect();
    json!({
        "count": names.len(),
        "read": read.len(),
        "write": write.len(),
        "names": names,
        "read_names": read,
        "write_names": write,
    })
}

/// tools/list 的 schema。**与 prepare() 一一对应**：新增工具必须同时改两处，
/// 且 tests/mcp.rs 断言 tools/list 的每个名字都能被 tools/call 认领（防「挂名不实现」）。
fn tool_schemas() -> Vec<Value> {
    let pid = json!({ "type": "string", "description": "person UUID (from list_persons)" });
    vec![
        json!({
            "name": "list_persons",
            "description": "列出全部在档人员（不含已归档）。返回 person_id/name/birthday/gender/id_note。",
            "inputSchema": { "type": "object", "properties": {}, "required": [] }
        }),
        json!({
            "name": "get_timeline",
            "description": "某人的健康事件时间线（文档级：日期/类型/标题），doc_date DESC。",
            "inputSchema": { "type": "object", "properties": { "person_id": pid }, "required": ["person_id"] }
        }),
        json!({
            "name": "search_records",
            "description": "在某人的箱内做中文全文检索（FTS5+jieba，命中片段带 [高亮]）。",
            "inputSchema": { "type": "object", "properties": {
                "person_id": pid,
                "q": { "type": "string", "description": "检索词（中文可用，会自动分词）" },
                "limit": { "type": "integer", "minimum": 1, "maximum": 100, "description": "命中上限，默认 20" }
            }, "required": ["person_id", "q"] }
        }),
        json!({
            "name": "get_labs",
            "description": "某人的检验指标跨文档趋势（series/points，日精度 + H/L 异常标记 + 证据 docref 可溯原件）。",
            "inputSchema": { "type": "object", "properties": { "person_id": pid }, "required": ["person_id"] }
        }),
        json!({
            "name": "get_medications",
            "description": "某人的用药跨度（药品/剂量/起止日 + 证据 docref 可溯原件）。status 原样透传上游（恒 active，不推断停药）。",
            "inputSchema": { "type": "object", "properties": { "person_id": pid }, "required": ["person_id"] }
        }),
        json!({
            "name": "get_conditions",
            "description": "某人的病程泳道（诊断原文写法 + onset + 原件印了的 ICD 码 + 证据 docref）。原文原样透传，不做术语归一。",
            "inputSchema": { "type": "object", "properties": { "person_id": pid }, "required": ["person_id"] }
        }),
        json!({
            "name": "get_measurements",
            "description": "某人的手动条目回放投影（自测值/笔记/动作日志）。家庭自测值与医院指标结构上分开；读不出结构化载荷时 payload_readable=false（原文仍在 text 里）。可选 kind/from/to 真过滤。",
            "inputSchema": { "type": "object", "properties": {
                "person_id": pid,
                "kind": { "type": "string", "description": "只看某一类：self_measurement|note|profile_event" },
                "from": { "type": "string", "description": "起始日期(含) YYYY-MM-DD" },
                "to": { "type": "string", "description": "结束日期(含) YYYY-MM-DD" }
            }, "required": ["person_id"] }
        }),
        json!({
            "name": "get_audit",
            "description": "审计视图（发生过什么，不是现在是什么）。两层真相层：source=registry（人员主数据：建档/改档/归档，不折叠）+ source=vault（上游内核事件日志：导入/导出/分享/删除，复用上游判据）。可选 person/op 真过滤；未知人过滤得 0 条（不是错误）。",
            "inputSchema": { "type": "object", "properties": {
                "person": { "type": "string", "description": "只看某人（person_id）。缺省 = 全部人" },
                "op": { "type": "string", "description": "只看某动作（大小写不敏感）：create|update|delete（registry 侧）或 导入|导出|分享|删除（vault 侧）" }
            }, "required": [] }
        }),
        json!({
            "name": "list_documents",
            "description": "某人的文档清单（docref=原件 sha256、类型、日期、字节数）。",
            "inputSchema": { "type": "object", "properties": { "person_id": pid }, "required": ["person_id"] }
        }),
        json!({
            "name": "get_document_detail",
            "description": "按 sha256 docref 取文档详情：OCR 文本层 + 抽取结果（原样透传，不猜）。",
            "inputSchema": { "type": "object", "properties": {
                "person_id": pid,
                "sha256": { "type": "string", "description": "64 位小写 hex 内容哈希" }
            }, "required": ["person_id", "sha256"] }
        }),
        // ---------- 写侧 ----------
        json!({
            "name": "create_person",
            "description": "建档：新建一个人并返回 person_id（写入 registry/persons.jsonl 真相层）。已有同名者请先 list_persons 核对，避免重复建档。",
            "inputSchema": { "type": "object", "properties": {
                "name": { "type": "string", "description": "姓名（必填，非空）" },
                "birthday": { "type": "string", "description": "出生日期，宽松格式如 1988-03-02 或 1988" },
                "gender": { "type": "string", "description": "性别（原文透传，不做归一推断）" },
                "id_note": { "type": "string", "description": "就诊卡号/病历号等外部锚点" }
            }, "required": ["name"] }
        }),
        json!({
            "name": "upload_document",
            "description": "传原件：把一份病历原件（txt/pdf）写入指定人的档案（原件入 CAS 永不丢，自动建立检索文本层）。二进制须 base64 编码。返回 content_hash(=docref)、deduped、text_indexed。",
            "inputSchema": { "type": "object", "properties": {
                "person_id": pid,
                "filename": { "type": "string", "description": "原始文件名，含扩展名（如 血常规.txt）" },
                "content_base64": { "type": "string", "description": "原件字节的标准 base64（可含换行）" }
            }, "required": ["person_id", "filename", "content_base64"] }
        }),
    ]
}
