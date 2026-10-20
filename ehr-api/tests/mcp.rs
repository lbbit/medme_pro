// MCP server 契约门禁（阶段4 · D-017）。
// 关键不变量（D4 视角：每条断言都对应一个「测过就以为测了」的缝）：
//  1. tools/list 公布的每个工具名都能被 tools/call 认领（防「挂名不实现」）；
//  2. 未在 tools/list 的名字调 tools/call ⇒ isError + METHOD_NOT_FOUND（不是假成功）；
//  3. 缺参数/空串/坏 limit ⇒ -32602，且**在任何 IO 之前**拒绝；
//  4. 未知人 ⇒ -32001（与 404 领域语义对齐），agent 可据此换人而不是重试；
//  5. 真数据链路：建档 → 上传原件 → tools/call 查到同一 sha 与中文内容（与 REST 同源真相层）；
//  6. REST 面不被 MCP 改动破坏（openapi paths 仍 >=16，契约测试兜底）。
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;

async fn rpc(app: &axum::Router, msg: Value) -> Value {
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(Body::from(msg.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK, "/mcp must answer 200");
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&bytes).expect("json-rpc response is valid json")
}

fn result(v: &Value) -> &Value {
    assert_eq!(v["jsonrpc"], "2.0");
    assert!(v.get("error").is_none(), "unexpected error: {v}");
    &v["result"]
}

async fn call_tool(app: &axum::Router, name: &str, args: Value) -> Value {
    let r = rpc(app, json!({"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":name,"arguments":args}})).await;
    result(&r).clone()
}

fn tool_data(v: &Value) -> Value {
    assert_eq!(v["isError"], false, "tool call failed: {v}");
    // content[0].text 与 structuredContent 必须同源一致（人读面 vs 机读面不得分叉）
    let text = v["content"][0]["text"].as_str().expect("text content block").to_string();
    let sc = v["structuredContent"].clone();
    let parsed: Value = serde_json::from_str(&text).expect("text block is json");
    assert_eq!(parsed, sc, "content.text and structuredContent diverged");
    sc
}

/// 每个测试独立 EHR_HOME：cargo test 默认多线程并行，共用一个 pid 目录会互删（D4 又一次：先怀疑测量方法）。
fn fixture(tag: &str) -> axum::Router {
    let tmp = std::env::temp_dir().join(format!("ehr-api-mcp-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(tmp.join("vaults")).unwrap();
    ehr_api::router(ehr_api::AppState::from_home(&tmp))
}

#[tokio::test(flavor = "multi_thread")]
async fn mcp_initialize_and_tools_list_are_wellformed() {
    let app = fixture("init");

    let init = result(&rpc(&app, json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}})).await).clone();
    assert_eq!(init["protocolVersion"], ehr_api::mcp::PROTOCOL_VERSION);
    assert_eq!(init["serverInfo"]["name"], "medme-ehr");
    assert!(init["capabilities"]["tools"].is_object(), "tools capability must be declared");

    let tools = result(&rpc(&app, json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}})).await)["tools"]
        .as_array().expect("tools array").clone();
    assert!(tools.len() >= 6, "expected the 6 read-side tools, got {}", tools.len());
    for t in &tools {
        let name = t["name"].as_str().expect("tool name string");
        assert!(!t["description"].as_str().unwrap_or("").is_empty(), "{name} needs a description for agents");
        assert_eq!(t["inputSchema"]["type"], "object", "{name} inputSchema must be an object schema");
        // 挂名不实现检测：公布的每个名字都必须能被 call_tool 认领
        let r = rpc(&app, json!({"jsonrpc":"2.0","id":3,"method":"tools/call",
                                 "params":{"name":name,"arguments":{}}})).await;
        let e = &r["result"]["_code"];
        let code = e.as_i64().unwrap_or(0);
        assert_ne!(code, ehr_api_mcp_test::METHOD_NOT_FOUND, "tool `{name}` is advertised but not implemented");
    }

    // 未知方法 ⇒ -32601（不是假成功）
    let bad = rpc(&app, json!({"jsonrpc":"2.0","id":4,"method":"no/such"})).await;
    assert_eq!(bad["error"]["code"], -32601);
}

#[tokio::test(flavor = "multi_thread")]
async fn mcp_argument_validation_rejects_before_io() {
    let app = fixture("argval");

    // 未公布的名字 ⇒ -32601
    let unknown = call_tool(&app, "get_medications", json!({"person_id":"x"})).await;
    assert_eq!(unknown["isError"], true);
    assert_eq!(unknown["_code"], -32601);

    // 缺 person_id / 纯空串 ⇒ -32602。
    // ⚠️ 别把 "p" 当非法参数塞进这里：它是合法非空 id，只是查无此人 → 那是 -32001（见下一测试）。
    //   D-011 纪律第三次应验：先跑 DBG 再定断言，别拿错的探针判对的代码。
    for args in [json!({}), json!({"person_id":"  "})] {
        let r = call_tool(&app, "get_timeline", args).await;
        assert_eq!(r["_code"], -32602, "timeline arg validation: {r}");
    }
    let r = call_tool(&app, "search_records", json!({"person_id":"p"})).await;
    assert_eq!(r["_code"], -32602, "missing q must be rejected");
    let r = call_tool(&app, "search_records", json!({"person_id":"p","q":"血","limit":"many"})).await;
    assert_eq!(r["_code"], -32602, "non-integer limit must be rejected (no silent fallback)");
}

#[tokio::test(flavor = "multi_thread")]
async fn mcp_unknown_person_is_its_own_error_code() {
    let app = fixture("unknown");
    let r = call_tool(&app, "get_timeline", json!({"person_id":"00000000-0000-0000-0000-000000000000"})).await;
    assert_eq!(r["isError"], true);
    assert_eq!(r["_code"], -32001, "unknown person must be distinguishable from internal error");
    assert!(r["content"][0]["text"].as_str().unwrap_or("").contains("no such person"));
}

/// 真数据链路：REST 建档 + 上传原件 → MCP 工具读到同一 sha / 同一中文内容。
#[tokio::test(flavor = "multi_thread")]
async fn mcp_reads_same_truth_layer_as_rest() {
    let app = fixture("truth");

    let created = app.clone().oneshot(
        Request::builder().method("POST").uri("/api/v1/persons")
            .header("content-type","application/json")
            .body(Body::from(json!({"name":"MCP测试"}).to_string())).unwrap()
    ).await.unwrap();
    assert_eq!(created.status(), StatusCode::CREATED);
    let person: Value = serde_json::from_slice(&axum::body::to_bytes(created.into_body(), usize::MAX).await.unwrap()).unwrap();
    let pid = person["id"].as_str().unwrap().to_string();

    let sample = "姓名：张三\n检查项目：血常规\n白细胞计数 11.2 10^9/L 偏高\n";
    let boundary = "X-BOUND";
    let body = format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"血常规.txt\"\r\nContent-Type: text/plain\r\n\r\n{sample}\r\n--{boundary}--\r\n");
    let up = app.clone().oneshot(
        Request::builder().method("POST").uri(format!("/api/v1/persons/{pid}/documents"))
            .header("content-type", format!("multipart/form-data; boundary={boundary}"))
            .body(Body::from(body)).unwrap()
    ).await.unwrap();
    assert_eq!(up.status(), StatusCode::OK);
    let upv: Value = serde_json::from_slice(&axum::body::to_bytes(up.into_body(), usize::MAX).await.unwrap()).unwrap();
    let sha = upv["content_hash"].as_str().unwrap().to_string();
    assert_eq!(sha.len(), 64);

    // 1) list_persons
    let persons = tool_data(&call_tool(&app, "list_persons", json!({})).await);
    assert_eq!(persons["count"], 1);
    assert_eq!(persons["persons"][0]["name"], "MCP测试");

    // 2) get_timeline（真解析日期来自样本首行 姓名/项目？不依赖具体值，只断言 count=1）
    let tl = tool_data(&call_tool(&app, "get_timeline", json!({"person_id":pid})).await);
    assert_eq!(tl["count"], 1, "timeline must see the ingested doc: {tl}");

    // 3) search_records 命中「白细胞」并回显 q（参数生效回执 D-011 纪律）
    let sr = tool_data(&call_tool(&app, "search_records", json!({"person_id":pid,"q":"白细胞"})).await);
    assert_eq!(sr["q"], "白细胞");
    assert_eq!(sr["count"], 1, "search must hit: {sr}");
    assert!(sr["items"][0]["snippet"].as_str().unwrap_or("").contains("[白细胞]"), "no jieba highlight: {}", sr["items"][0]["snippet"]);
    // 罕见词零假阳性
    let none = tool_data(&call_tool(&app, "search_records", json!({"person_id":pid,"q":"zzqqww"})).await);
    assert_eq!(none["count"], 0);

    // 4) list_documents 里 docref = 同一 sha
    let docs = tool_data(&call_tool(&app, "list_documents", json!({"person_id":pid})).await);
    assert_eq!(docs["items"][0]["content_hash"], sha, "MCP must serve the same CAS ref as REST");

    // 5) get_document_detail 文本层含中文原文（透传不猜）
    let det = tool_data(&call_tool(&app, "get_document_detail", json!({"person_id":pid,"sha256":sha})).await);
    assert!(det["text"].as_str().unwrap_or("").contains("白细胞计数"), "text layer missing: {det}");

    // 6) get_labs 有 series（血常规样本应出指标）
    let labs = tool_data(&call_tool(&app, "get_labs", json!({"person_id":pid})).await);
    assert!(labs["series"].as_array().map(|s| !s.is_empty()).unwrap_or(false), "no lab series: {labs}");

    // 7) 坏 sha ⇒ 领域错误透出（不是空结果）
    let bad = call_tool(&app, "get_document_detail", json!({"person_id":pid,"sha256":"zz"})).await;
    assert_eq!(bad["isError"], true);
}

/// MCP 面不污染 REST 契约：paths 数不变（≥16），且未把 /mcp 塞进 OpenAPI。
#[tokio::test(flavor = "multi_thread")]
async fn mcp_route_does_not_disturb_rest_contract() {
    let app = fixture("contract");
    let res = app.clone().oneshot(Request::builder().uri("/openapi.json").body(Body::empty()).unwrap()).await.unwrap();
    let doc: Value = serde_json::from_slice(&axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap()).unwrap();
    let n = doc["paths"].as_object().unwrap().len();
    assert!(n >= 16, "openapi paths regressed: {n}");
    assert!(doc["paths"].as_object().unwrap().get("/mcp").is_none(), "/mcp must stay out of the REST contract doc");
}

mod ehr_api_mcp_test {
    pub const METHOD_NOT_FOUND: i64 = -32601;
}