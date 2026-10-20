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

use crate::{vaults, ApiError, AppState, StatusCode};

pub const PROTOCOL_VERSION: &str = "2025-06-18";

// ---------- JSON-RPC 错误码 ----------
const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;
/// 应用层错误（本项目自定义区间 -32000..-32099）
const ERR_NO_SUCH_PERSON: i64 = -32001;
const ERR_INTERNAL: i64 = -32002;

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
            "instructions": "多人电子健康档案读侧工具集。person_id 为 UUID；先用 list_persons 拿人，再用 search_records / get_labs / get_timeline 查档。所有内容来自原件 CAS+事件日志，text/extraction 透传不猜。",
        })),
        "ping" => ok_obj(id.clone(), json!({})),
        "notifications/initialized" | "initialized" => ok_obj(id.clone(), json!({})),
        "tools/list" => ok_obj(id.clone(), json!({ "tools": tool_schemas() })),
        "tools/call" => {
            let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
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
    let out: Vec<Value> = match body {
        Value::Array(items) => items.iter().filter_map(|m| handle_message(st, m)).collect(),
        single @ Value::Object(_) => handle_message(st, single).into_iter().collect(),
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

fn str_arg<'a>(args: &'a Value, k: &str) -> Result<&'a str, i64> {
    args.get(k)
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .ok_or(INVALID_PARAMS)
}

fn u64_arg(args: &Value, k: &str, default: u64) -> Result<u64, i64> {
    match args.get(k) {
        None | Some(Value::Null) => Ok(default),
        Some(v) => v.as_u64().ok_or(INVALID_PARAMS),
    }
}

/// 领域错误 → JSON-RPC 错误码。404 人档不存在单独成码（agent 需据此改选人，而非重试）。
fn domain_code(e: &ApiError) -> i64 {
    match e.0 {
        StatusCode::NOT_FOUND => ERR_NO_SUCH_PERSON,
        _ => ERR_INTERNAL,
    }
}

/// 单个工具的执行体（在 block_in_place 临界区里跑：rusqlite/Vault 非 Send，同 REST 模式）。
fn call_tool(st: &Arc<AppState>, id: Value, name: &str, args: &Value) -> Value {
    // 参数校验放在 async 之外，先于任何 IO —— 非法参数不该打开任何文件。
    let prepared = match prepare(name, args) {
        Ok(p) => p,
        Err(code) => return tool_err(id, code, format!("invalid arguments for tool `{name}`")),
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
    Documents { pid: String },
    DocumentDetail { pid: String, sha: String },
}

fn prepare(name: &str, args: &Value) -> Result<Prepared, i64> {
    Ok(match name {
        "list_persons" => Prepared::Persons,
        "get_timeline" => Prepared::Timeline { pid: str_arg(args, "person_id")?.to_string() },
        "search_records" => Prepared::Search {
            pid: str_arg(args, "person_id")?.to_string(),
            q: str_arg(args, "q")?.to_string(),
            limit: u64_arg(args, "limit", 20)?.clamp(1, 100) as usize,
        },
        "get_labs" => Prepared::Labs { pid: str_arg(args, "person_id")?.to_string() },
        "list_documents" => Prepared::Documents { pid: str_arg(args, "person_id")?.to_string() },
        "get_document_detail" => Prepared::DocumentDetail {
            pid: str_arg(args, "person_id")?.to_string(),
            sha: str_arg(args, "sha256")?.to_string(),
        },
        _ => return Err(METHOD_NOT_FOUND), // 未知工具名走 METHOD_NOT_FOUND（非参数问题）
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
    ]
}
