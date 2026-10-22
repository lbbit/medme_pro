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
    // D-021：原先是 `tools.len() >= 6`（注释写「6 read-side tools」）——**快照不是判据**：
    // ①它对「tools/list 少公布了工具」不敏感（少公布也绿）；
    // ②注释里的「6」与断言的口径本身就对不上（断言比的是**总数**，不是读侧数）⇒
    //   这正是「手抄实现细节」的典型：数字过期了没人发现，因为它既不是判据也不影响绿/红。
    //
    // ⚠️ 关键：判据**不能**写成 `tools.len() == tool_manifest()["count"]`——那是**同义反复**
    //   （tools/list 与 tool_manifest 都从 tool_schemas() 派生 ⇒ 永远相等 ⇒ 永远绿，
    //    这正是 D-037「绿但测错对象」的最隐蔽形态）。
    //   真判据必须**跨面**：MCP 工具面 ↔ REST 面（两份独立实现）必须对得上。
    //
    // 跨面不变量（本项目的架构承诺，mcp.rs 文件头②③）：**不存在「MCP 里能查、REST 里查不到」的分叉**
    //   ——每个 MCP 工具都必须对应一个**真实存在的 REST operation**。
    //   映射规则（不手抄表，靠命名派生）：去掉 get_/list_/create_/upload_ 前缀后取首词，
    //   要求存在某个 REST 路径的**最后一段**包含该词（大小写不敏感）。
    //   例：get_labs→"labs"∈"labs"；get_document_detail→"document"∈"documents/{sha}"；
    //       search_records→"search"∈"search"；create_person→"person"∈"persons"。
    //   ⇒ 若有人加了一个「REST 侧并不存在」的 MCP 工具，本门立刻红（这在旧断言下永远绿）。
    let doc = ehr_api::openapi_json();
    let rest_last_segments: Vec<String> = doc["paths"]
        .as_object()
        .expect("openapi paths")
        .keys()
        .map(|p| {
            p.rsplit('/')
                .next()
                .unwrap_or("")
                .to_ascii_lowercase()
        })
        .collect();
    for t in &tools {
        let name = t["name"].as_str().unwrap();
        let rest = name
            .trim_start_matches("get_")
            .trim_start_matches("list_")
            .trim_start_matches("create_")
            .trim_start_matches("upload_")
            .split('_')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        assert!(
            rest_last_segments.iter().any(|seg| seg.contains(&rest)),
            "MCP tool `{name}` has no corresponding REST endpoint (no path segment contains \"{rest}\") \
             —— MCP 面与 REST 面已分叉（D-009 跨面判据）"
        );
    }
    // 读侧工具数必须与实现派生的 read 数一致（**方向性**判据：少公布工具会红）。
    // 注意：这一条只保证「不多不少于派生清单」；它的价值在与上面的**跨面**判据互补
    // （派生清单本身是同源，跨面判据才是真正能抓漂移的那一条）。
    let man = ehr_api::mcp::tool_manifest();
    assert_eq!(
        tools.len(),
        man["count"].as_u64().unwrap() as usize,
        "tools/list 公布的工具数必须等于实现派生清单（不是手抄快照）"
    );
    let published_read = tools
        .iter()
        .filter(|t| {
            let n = t["name"].as_str().unwrap_or("");
            !ehr_api::mcp::WRITE_TOOLS.contains(&n)
        })
        .count();
    assert_eq!(
        published_read,
        man["read"].as_u64().unwrap() as usize,
        "tools/list 的读侧工具数必须等于实现派生清单的 read 数（不是手抄快照）"
    );
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
    // ⚠️ D-029：此前的探针用的是 `get_medications`（当时它「恰好」是未公布的名字 ⇒ 断言 -32601）。
    //    D-029 把它实装成真工具后，同一探针会返回 -32001 ⇒ **断言探针本身过期**。
    //    教训（D-011 同族）：门禁探针依赖「某名字不存在」这一事实时，加功能要连带改探针。
    let unknown = call_tool(&app, "get_no_such_tool", json!({"person_id":"x"})).await;
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


// ============ D-029：medications / conditions / measurements 三个读侧工具 ============

/// D-029：MCP 侧补齐 meds/conditions/measurements 三个读侧工具。
///
/// 判据（与 D-028 未销账②一致）：**必须复用同一 `meds_aggregate`/`conditions_aggregate`/
/// `measurements_projection`**，不得另写一份判据；因此本门不只断言「有数据」，
/// 而是断言 **MCP 返回的 docref 与 REST 返回的 docref 逐字相等**（同源的唯一硬证据）。
#[tokio::test(flavor = "multi_thread")]
async fn mcp_new_read_tools_match_rest_facets() {
    let app = fixture("d029");

    // 建档
    let created = app.clone().oneshot(
        Request::builder().method("POST").uri("/api/v1/persons")
            .header("content-type","application/json")
            .body(Body::from(json!({"name":"D029测试"}).to_string())).unwrap()
    ).await.unwrap();
    assert_eq!(created.status(), StatusCode::CREATED);
    let person: Value = serde_json::from_slice(&axum::body::to_bytes(created.into_body(), usize::MAX).await.unwrap()).unwrap();
    let pid = person["id"].as_str().unwrap().to_string();

    // 上传三件：化验单 / 处方 / 出院记录（覆盖 labs / meds / conditions 三个面）
    for (fname, body_txt) in [
        ("血常规.txt", "姓名：张三\n检查项目：血常规\n白细胞计数 11.2 10^9/L 偏高\n"),
        ("处方.txt", "姓名：张三\n处方\n阿莫西林胶囊 0.5g bid\n"),
        ("出院记录.txt", "姓名：张三\n出院记录\n主要诊断：2型糖尿病(E11.9)\n"),
    ] {
        let boundary = "X-BOUND";
        let body = format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{fname}\"\r\nContent-Type: text/plain\r\n\r\n{body_txt}\r\n--{boundary}--\r\n");
        let up = app.clone().oneshot(
            Request::builder().method("POST").uri(format!("/api/v1/persons/{pid}/documents"))
                .header("content-type", format!("multipart/form-data; boundary={boundary}"))
                .body(Body::from(body)).unwrap()
        ).await.unwrap();
        assert_eq!(up.status(), StatusCode::OK);
    }

    // 写一条自测值（measurements 面的数据源）
    let sm = app.clone().oneshot(
        Request::builder().method("POST").uri(format!("/api/v1/persons/{pid}/entries/self-measurement"))
            .header("content-type","application/json")
            .body(Body::from(json!({"values":[
                {"analyte_key":"bp_systolic","value":128,"unit":"mmHg"},
                {"analyte_key":"bp_diastolic","value":82,"unit":"mmHg"}
            ]}).to_string())).unwrap()
    ).await.unwrap();
    assert_eq!(sm.status(), StatusCode::CREATED);

    // REST 侧取基准（真相层同一份）
    let rest_meds: Value = serde_json::from_slice(&axum::body::to_bytes(
        app.clone().oneshot(Request::builder().uri(format!("/api/v1/persons/{pid}/medications")).body(Body::empty()).unwrap())
            .await.unwrap().into_body(), usize::MAX).await.unwrap()).unwrap();
    let rest_conds: Value = serde_json::from_slice(&axum::body::to_bytes(
        app.clone().oneshot(Request::builder().uri(format!("/api/v1/persons/{pid}/conditions")).body(Body::empty()).unwrap())
            .await.unwrap().into_body(), usize::MAX).await.unwrap()).unwrap();
    let rest_meas: Value = serde_json::from_slice(&axum::body::to_bytes(
        app.clone().oneshot(Request::builder().uri(format!("/api/v1/persons/{pid}/measurements")).body(Body::empty()).unwrap())
            .await.unwrap().into_body(), usize::MAX).await.unwrap()).unwrap();

    // MCP 侧三个新工具
    let m_meds = tool_data(&call_tool(&app, "get_medications", json!({"person_id":pid})).await);
    let m_conds = tool_data(&call_tool(&app, "get_conditions", json!({"person_id":pid})).await);
    let m_meas = tool_data(&call_tool(&app, "get_measurements", json!({"person_id":pid})).await);

    // 同源硬证据：MCP 与 REST 的 docref / 计数逐字相等（不是「看起来有数据」）
    assert_eq!(m_meds["med_count"], rest_meds["med_count"], "med_count must equal REST: {m_meds}");
    assert!(m_meds["med_count"].as_u64().unwrap_or(0) > 0, "no meds: {m_meds}");
    assert_eq!(m_meds["medications"][0]["docrefs"], rest_meds["medications"][0]["docrefs"],
               "MCP meds docrefs must equal REST (same aggregate, no second judgement)");
    assert_eq!(m_meds["medications"][0]["status"], "active", "status must be passed through verbatim");

    assert_eq!(m_conds["condition_count"], rest_conds["condition_count"], "condition_count must equal REST");
    assert!(m_conds["condition_count"].as_u64().unwrap_or(0) > 0, "no conditions: {m_conds}");
    assert_eq!(m_conds["conditions"][0]["docrefs"], rest_conds["conditions"][0]["docrefs"],
               "MCP conditions docrefs must equal REST");

    assert_eq!(m_meas["count"], rest_meas["count"], "measurements count must equal REST");
    assert_eq!(m_meas["count"], 1, "one self-measurement written: {m_meas}");
    assert_eq!(m_meas["measurements"][0]["kind"], "self_measurement");
    assert!(m_meas["measurements"][0]["docref"].as_str().unwrap_or("").len() == 64, "docref must be a sha256");
    // 家庭自测值与医院指标**结构上分开**（上游 GroupKey::SelfMeasured）：
    // 自测值只出现在 `self_measured=true` 的独立序列里，**绝不与医院指标同序列**。
    // ⚠️ 判据形状：不是「labs 里不该有自测值」（上游就是在 labs 里给的，只是分组不同），
    //    而是「医院序列（self_measured=false）里不得出现自测值的 analyte_key」——D-028 同款判据。
    let labs = tool_data(&call_tool(&app, "get_labs", json!({"person_id":pid})).await);
    let series = labs["series"].as_array().expect("series array");
    let self_series = series.iter().filter(|s| s["self_measured"] == true).collect::<Vec<_>>();
    assert!(!self_series.is_empty(), "self-measured series must exist and be marked: {labs}");
    let hospital_series = series.iter().filter(|s| s["self_measured"] == false).collect::<Vec<_>>();
    for h in &hospital_series {
        let key = h["analyte_key"].as_str().unwrap_or("");
        assert!(key != "bp_systolic" && key != "bp_diastolic",
                "home self-measured analyte must not appear in a hospital series: {labs}");
    }
    // 反向：自测序列必须真的带着 self_measured=true 这个标志（消费方才知道它是家庭值）
    assert!(self_series.iter().all(|s| s["self_measured"] == true));
}

/// D-029：三个新工具的过滤器与错误码语义（参数真进判据，不是摆设）。
#[tokio::test(flavor = "multi_thread")]
async fn mcp_new_read_tools_validate_filters_and_errors() {
    let app = fixture("d029err");
    let created = app.clone().oneshot(
        Request::builder().method("POST").uri("/api/v1/persons")
            .header("content-type","application/json")
            .body(Body::from(json!({"name":"D029错误"}).to_string())).unwrap()
    ).await.unwrap();
    let person: Value = serde_json::from_slice(&axum::body::to_bytes(created.into_body(), usize::MAX).await.unwrap()).unwrap();
    let pid = person["id"].as_str().unwrap().to_string();

    // 缺 person_id ⇒ -32602（参数问题，不是重试）
    let r = call_tool(&app, "get_conditions", json!({})).await;
    assert_eq!(r["_code"], -32602, "missing person_id must be INVALID_PARAMS: {r}");

    // 未知人 ⇒ -32001（换人，不是重试）
    let r = call_tool(&app, "get_medications", json!({"person_id":"nope"})).await;
    assert_eq!(r["_code"], -32001, "unknown person must be its own code: {r}");

    // 非法 kind ⇒ -32602（参数非法 ⇒ agent 改参数）
    let r = call_tool(&app, "get_measurements", json!({"person_id":pid,"kind":"nonsense"})).await;
    assert_eq!(r["_code"], -32602, "bad kind must be INVALID_PARAMS: {r}");

    // 非法日期 ⇒ -32602
    let r = call_tool(&app, "get_measurements", json!({"person_id":pid,"from":"not-a-date"})).await;
    assert_eq!(r["_code"], -32602, "bad date must be INVALID_PARAMS: {r}");

    // kind 过滤器真过滤（本例只有自测值 ⇒ kind=note 必为 0 条）
    let sm = app.clone().oneshot(
        Request::builder().method("POST").uri(format!("/api/v1/persons/{pid}/entries/self-measurement"))
            .header("content-type","application/json")
            .body(Body::from(json!({"values":[{"analyte_key":"heart_rate","value":72,"unit":"bpm"}]}).to_string())).unwrap()
    ).await.unwrap();
    assert_eq!(sm.status(), StatusCode::CREATED);
    let all = tool_data(&call_tool(&app, "get_measurements", json!({"person_id":pid})).await);
    assert_eq!(all["count"], 1);
    let notes = tool_data(&call_tool(&app, "get_measurements", json!({"person_id":pid,"kind":"note"})).await);
    assert_eq!(notes["count"], 0, "kind filter must really filter: {notes}");
}

/// D-031：MCP 侧补齐审计工具 `get_audit`。
///
/// 判据（与 D-030 未销账②一致）：**必须复用同一 `audit_view()`**，不得另写一份判据；
/// 硬证据不是「两边都有数据」，而是 **MCP 与 REST 的审计回执逐字相等**（count / entries / sha256）。
#[tokio::test(flavor = "multi_thread")]
async fn mcp_audit_matches_rest_audit_view() {
    let app = fixture("d031");

    // 建档 + 传一份原件（⇒ registry 侧有 create、vault 侧有导入）
    let created = app.clone().oneshot(
        Request::builder().method("POST").uri("/api/v1/persons")
            .header("content-type","application/json")
            .body(Body::from(json!({"name":"D031审计"}).to_string())).unwrap()
    ).await.unwrap();
    assert_eq!(created.status(), StatusCode::CREATED);
    let person: Value = serde_json::from_slice(&axum::body::to_bytes(created.into_body(), usize::MAX).await.unwrap()).unwrap();
    let pid = person["id"].as_str().unwrap().to_string();

    let sample = "姓名：审计甲\n检查项目：血常规\n白细胞计数 11.2 10^9/L 偏高\n";
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

    // REST 基准
    let rest: Value = serde_json::from_slice(&axum::body::to_bytes(
        app.clone().oneshot(Request::builder().uri("/api/v1/audit").body(Body::empty()).unwrap())
            .await.unwrap().into_body(), usize::MAX).await.unwrap()).unwrap();

    // MCP 侧
    let m = tool_data(&call_tool(&app, "get_audit", json!({})).await);

    // 同源硬证据：MCP 与 REST 的回执**逐字相等**（同一个 audit_view()，不是两套判据）
    assert_eq!(m, rest, "MCP audit must equal REST audit (same audit_view()): {m} vs {rest}");
    assert!(rest["count"].as_u64().unwrap_or(0) >= 2,
            "two truth layers must both be present (registry create + vault import): {rest}");

    // 两层真相层都在
    let sources: Vec<&str> = m["entries"].as_array().unwrap().iter()
        .map(|e| e["source"].as_str().unwrap_or("")).collect();
    assert!(sources.contains(&"registry"), "registry layer missing: {m}");
    assert!(sources.contains(&"vault"), "vault layer missing: {m}");

    // vault 侧的导入条目带内容哈希，且逐字等于上传原件的 sha256
    let vault_entry = m["entries"].as_array().unwrap().iter()
        .find(|e| e["source"] == "vault").expect("a vault entry");
    assert_eq!(vault_entry["sha256"], sha, "vault import entry must carry the original's sha256");

    // registry 侧**不臆造**内容哈希（键缺省，不是空串也不是编的）
    let reg_entry = m["entries"].as_array().unwrap().iter()
        .find(|e| e["source"] == "registry").expect("a registry entry");
    assert!(reg_entry.get("sha256").is_none(), "registry events have no content hash: {reg_entry}");

    // 参数真进判据：person 过滤 ⇒ 只剩该人
    let by_person = tool_data(&call_tool(&app, "get_audit", json!({"person": pid})).await);
    assert!(by_person["count"].as_u64().unwrap_or(0) > 0);
    for e in by_person["entries"].as_array().unwrap() {
        assert_eq!(e["person_id"], pid, "person filter must really filter: {by_person}");
    }
    // 未知人 ⇒ 0 条（**不是**错误码：审计是全局视图）
    let unknown = tool_data(&call_tool(&app, "get_audit", json!({"person":"00000000-0000-0000-0000-000000000000"})).await);
    assert_eq!(unknown["count"], 0, "unknown person must yield 0 entries, not an error: {unknown}");

    // op 过滤真进判据：op=create 只剩 registry 侧 create
    let by_op = tool_data(&call_tool(&app, "get_audit", json!({"op":"create"})).await);
    assert!(by_op["count"].as_u64().unwrap_or(0) > 0);
    for e in by_op["entries"].as_array().unwrap() {
        assert_eq!(e["op"], "create", "op filter must really filter: {by_op}");
    }
    // 大小写不敏感（与 REST 同语义）
    let upper = tool_data(&call_tool(&app, "get_audit", json!({"op":"CREATE"})).await);
    assert_eq!(upper["count"], by_op["count"], "op filter must be case-insensitive: {upper}");
}

/// MCP 面不污染 REST 契约：paths 数与**派生清单**一致（不再硬编码 16），且未把 /mcp 塞进 OpenAPI。
/// T16 · D-032 未销账⑩：硬编码的 16 是「快照不是判据」——加一条路由要连带改这里，漏改就红，
/// 而红得看不出是漏改还是真坏了。改为从实现取值后，漏改根本不会发生。
#[tokio::test(flavor = "multi_thread")]
async fn mcp_route_does_not_disturb_rest_contract() {    let app = fixture("contract");
    let res = app.clone().oneshot(Request::builder().uri("/openapi.json").body(Body::empty()).unwrap()).await.unwrap();
    let doc: Value = serde_json::from_slice(&axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap()).unwrap();
    let n = doc["paths"].as_object().unwrap().len();
    let expected = ehr_api::rest_manifest()["paths"].as_u64().unwrap() as usize;
    assert_eq!(n, expected, "openapi paths must equal the derived manifest (not a hardcoded snapshot)");
    assert!(doc["paths"].as_object().unwrap().get("/mcp").is_none(), "/mcp must stay out of the REST contract doc");
}

mod ehr_api_mcp_test {
    pub const METHOD_NOT_FOUND: i64 = -32601;
}

// ============ 写侧（D-019）：建档 / 传原件 ============

/// 写侧闭环：MCP 建档 → MCP 传原件 → MCP 读侧查到同一 sha 与中文原文。
/// **不绕 REST**：agent 全程只走 MCP（PLAN 阶段4 第二步②的目标）。
#[tokio::test(flavor = "multi_thread")]
async fn mcp_write_side_roundtrip_is_self_contained() {
    let app = fixture("write");

    // 1) create_person
    let created = tool_data(&call_tool(&app, "create_person",
        json!({"name":"写侧测试","birthday":"1990-05-06","gender":"male","id_note":"CASE-1"})).await);
    let pid = created["id"].as_str().expect("created person has id").to_string();
    assert_eq!(created["name"], "写侧测试");
    assert_eq!(created["birthday"], "1990-05-06");
    assert_eq!(created["id_note"], "CASE-1");

    // 2) upload_document（base64 传原件）
    let sample = "姓名：李四\n检查项目：血常规\n白细胞计数 9.9 10^9/L 正常\n";
    let b64 = b64_encode(sample.as_bytes());
    let up = tool_data(&call_tool(&app, "upload_document",
        json!({"person_id":pid,"filename":"血常规.txt","content_base64":b64})).await);
    let sha = up["content_hash"].as_str().expect("content_hash returned").to_string();
    assert_eq!(sha.len(), 64);
    assert_eq!(up["deduped"], false);
    assert_eq!(up["text_indexed"], true, "txt must be indexed for search: {up}");

    // 3) 读侧立刻可见（同一真相层）
    let persons = tool_data(&call_tool(&app, "list_persons", json!({})).await);
    assert_eq!(persons["count"], 1);
    assert_eq!(persons["persons"][0]["id"], pid);

    let tl = tool_data(&call_tool(&app, "get_timeline", json!({"person_id":pid})).await);
    assert_eq!(tl["count"], 1, "uploaded doc must appear in timeline: {tl}");

    let sr = tool_data(&call_tool(&app, "search_records", json!({"person_id":pid,"q":"白细胞"})).await);
    assert_eq!(sr["count"], 1, "uploaded doc must be searchable: {sr}");

    let det = tool_data(&call_tool(&app, "get_document_detail", json!({"person_id":pid,"sha256":sha})).await);
    assert!(det["text"].as_str().unwrap_or("").contains("白细胞计数"), "text layer missing: {det}");

    // 4) 幂等：同字节重传 ⇒ deduped=true 且 sha 不变
    let up2 = tool_data(&call_tool(&app, "upload_document",
        json!({"person_id":pid,"filename":"血常规.txt","content_base64":b64})).await);
    assert_eq!(up2["deduped"], true, "re-upload of same bytes must dedupe: {up2}");
    assert_eq!(up2["content_hash"], sha);
}

/// 写侧参数校验必须先于任何 IO：坏 base64 / 空内容 / 缺参 ⇒ -32602，且不留半成品。
#[tokio::test(flavor = "multi_thread")]
async fn mcp_write_side_rejects_bad_arguments() {
    let app = fixture("writebad");

    // 坏 base64 ⇒ -32602（并带原因文本，agent 才知道怎么改）
    let r = call_tool(&app, "upload_document",
        json!({"person_id":"p","filename":"a.txt","content_base64":"!!!!"})).await;
    assert_eq!(r["_code"], -32602, "bad base64 must be rejected: {r}");
    assert!(r["content"][0]["text"].as_str().unwrap_or("").contains("base64"),
        "error text must say what's wrong: {r}");

    // 缺 content_base64 ⇒ -32602
    let r = call_tool(&app, "upload_document", json!({"person_id":"p","filename":"a.txt"})).await;
    assert_eq!(r["_code"], -32602);

    // 空名字 ⇒ -32602
    let r = call_tool(&app, "create_person", json!({"name":"   "})).await;
    assert_eq!(r["_code"], -32602);

    // 校验失败 ⇒ **没有建出任何档**（副作用不得先发生）
    let persons = tool_data(&call_tool(&app, "list_persons", json!({})).await);
    assert_eq!(persons["count"], 0, "rejected writes must leave no trace: {persons}");
}

/// 写侧对「不存在的人」必须报 -32001（agent 换人，不是重试）。
#[tokio::test(flavor = "multi_thread")]
async fn mcp_upload_to_unknown_person_is_32001() {
    let app = fixture("write404");
    let r = call_tool(&app, "upload_document",
        json!({"person_id":"00000000-0000-0000-0000-000000000000","filename":"a.txt",
               "content_base64":b64_encode(b"x")})).await;
    assert_eq!(r["isError"], true);
    assert_eq!(r["_code"], -32001, "unknown person must be -32001: {r}");
}

/// 不支持的扩展名 ⇒ -32003（区别于「服务坏了」-32002，agent 据此改格式）。
#[tokio::test(flavor = "multi_thread")]
async fn mcp_upload_unsupported_type_is_32003() {
    let app = fixture("write501");
    let created = tool_data(&call_tool(&app, "create_person", json!({"name":"类型门禁"})).await);
    let pid = created["id"].as_str().unwrap().to_string();
    let r = call_tool(&app, "upload_document",
        json!({"person_id":pid,"filename":"scan.dcm","content_base64":b64_encode(b"DICM")})).await;
    assert_eq!(r["isError"], true);
    assert_eq!(r["_code"], -32003, "unsupported extension must be its own code: {r}");
}

/// 写侧工具也必须被 tools/list 公布（挂名不实现检测自动覆盖：公布的名字都要能被 call 认领）。
#[tokio::test(flavor = "multi_thread")]
async fn mcp_write_tools_are_advertised() {
    let app = fixture("writelist");
    let tools = result(&rpc(&app, json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}})).await)["tools"]
        .as_array().expect("tools array").clone();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    for want in ["create_person", "upload_document"] {
        assert!(names.contains(&want), "{want} must be advertised, got {names:?}");
    }
    // T15（D-031 新坑②）：工具数**不再硬编码**——断言「tools/list 的公布清单 == 实现派生的清单」。
    // 硬编码数字是「快照」不是「判据」：加一个工具要同步改三处，漏一处就红且看不出原因。
    let derived = ehr_api::mcp::tool_names();
    assert_eq!(names, derived.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
        "tools/list must publish exactly the derived tool list: {names:?} vs {derived:?}");
}

/// T15：写侧工具名单**只有一处**（`mcp::WRITE_TOOLS`）⇒ 工具分权与工具公布不会漂移。
/// 判据形状：①ro scope 拒的**恰好**是 WRITE_TOOLS 全集；②ro scope 放行的**恰好**是读侧全集。
/// （不是「抽样几个写侧工具」——抽样会漏掉新增工具的越权。）
#[test]
fn write_tool_list_and_scope_split_agree() {
    let names = ehr_api::mcp::tool_names();
    for t in &names {
        let is_write = ehr_api::mcp::WRITE_TOOLS.contains(&t.as_str());
        assert_eq!(
            ehr_api::auth::Scope::Ro.allows_tool(t),
            !is_write,
            "scope split must agree with WRITE_TOOLS for tool {t}"
        );
    }
    // 反向：WRITE_TOOLS 里的每个名字都必须真的被公布（防止名单写了工具却没实现）
    for t in ehr_api::mcp::WRITE_TOOLS {
        assert!(names.iter().any(|n| n == t), "WRITE_TOOLS contains unpublished tool {t}");
    }
    // 读侧计数 = 总数 - 写侧计数
    let man = ehr_api::mcp::tool_manifest();
    assert_eq!(man["count"].as_u64().unwrap() as usize, names.len());
    assert_eq!(man["write"].as_u64().unwrap() as usize, ehr_api::mcp::WRITE_TOOLS.len());
    assert_eq!(man["read"].as_u64().unwrap() as usize, names.len() - ehr_api::mcp::WRITE_TOOLS.len());
}

/// 测试侧 base64 编码器（生产侧只需解码；这里独立实现一遍，避免「用被测代码造输入」）。
fn b64_encode(data: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}
/// D-034 未销账⑫：MCP 侧补 `get_capabilities` 工具 —— **与 REST `GET /api/v1/capabilities` 逐字同源**。
///
/// 判据形状（D-009）：**不**断言「MCP 和 REST 各自自洽」，而是断言「两面的**同一份**清单逐字相等」
/// ⇒ 只吃 MCP 的宿主也能判断「我的文件能不能传」，且不会与 REST 面分叉。
#[tokio::test(flavor = "multi_thread")]
async fn mcp_capabilities_match_rest_capabilities_endpoint() {
    let app = fixture("caps");

    // MCP 面
    let data = tool_data(&call_tool(&app, "get_capabilities", json!({})).await);
    // REST 面
    let resp = app
        .clone()
        .oneshot(Request::get("/api/v1/capabilities").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20).await.unwrap();
    let rest: Value = serde_json::from_slice(&bytes).unwrap();

    assert_eq!(data, rest, "MCP capabilities must equal REST capabilities verbatim");
    // 关键事实对 agent 可见：能不能吃图片、有没有被拒的类型与原因。
    assert!(data["ocr_available"].is_boolean());
    assert!(data["accepted_exts"].is_array());
    assert!(data["rejected_exts"].is_array());
    // 拒绝原因在 `routes[]` 里（按路由给，不是按扩展名重复一份）⇒ 每条**未启用**的路由都必须带原因。
    let reasons_ok = data["routes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["enabled"].as_bool() == Some(false))
        .all(|r| r["reject_reason"].as_str().map(|s| !s.is_empty()).unwrap_or(false));
    assert!(reasons_ok, "every disabled route must carry a reject_reason");
    // 被拒的扩展名必须都在某条未启用路由的 exts 里（清单不是散点）
    let rejected: Vec<&str> = data["rejected_exts"].as_array().unwrap().iter().map(|s| s.as_str().unwrap()).collect();
    for e in rejected {
        let covered = data["routes"].as_array().unwrap().iter().any(|r| {
            r["enabled"].as_bool() == Some(false)
                && r["exts"].as_array().map(|xs| xs.iter().any(|x| x.as_str() == Some(e))).unwrap_or(false)
        });
        assert!(covered, "rejected ext {e} must belong to a disabled route with a reason");
    }
}

/// T35：resources 面的**协议形状**守卫（防止将来又改回不合规范的错误形状）。
///
/// 为什么需要它（实测抓到的真缺陷）：
///   `resources/read` 的失败此前回成 `result.isError=true` + `result._code`
///   （tools/call 的形状）⇒ 官方 MCP Python SDK 把它当 **ReadResourceResult** 校验
///   ⇒ 缺 `contents` ⇒ 抛 **ValidationError** ⇒ 宿主 agent 拿到的是「客户端崩溃」，
///   错误码（-32005/-32001/-32602）**白设计了**。
///   ⇒ MCP 规范：resources/read 的失败 = 协议级 error ⇒ `{"error":{code,message}}`。
///   ⇒ 本测试把这条钉死：失败**必须**走 error 信封，且**不得**带 isError 残留。
#[tokio::test(flavor = "multi_thread")]
async fn mcp_resources_read_failures_use_jsonrpc_error_envelope() {
    let app = fixture("res-err");

    for (uri, want_code, tag) in [
        ("ehr://nope", -32005i64, "unknown uri"),
        (
            "ehr://persons/00000000-0000-0000-0000-000000000000/labs",
            -32001,
            "unknown person",
        ),
    ] {
        let r = rpc(
            &app,
            json!({"jsonrpc":"2.0","id":9,"method":"resources/read","params":{"uri":uri}}),
        )
        .await;
        assert!(
            r.get("error").is_some(),
            "resources/read failure ({tag}) must be a JSON-RPC error envelope, got: {r}"
        );
        assert_eq!(r["error"]["code"], want_code, "resources/read {tag} code: {r}");
        assert!(
            r["error"]["message"].as_str().map(|s| !s.is_empty()).unwrap_or(false),
            "resources/read {tag} must carry a human-readable message: {r}"
        );
        assert!(
            r.get("result").is_none(),
            "resources/read failure ({tag}) must NOT carry a result envelope: {r}"
        );
    }

    // 缺 uri 参数 ⇒ -32602（改参数，不是重试）
    let r = rpc(&app, json!({"jsonrpc":"2.0","id":9,"method":"resources/read","params":{}})).await;
    assert_eq!(r["error"]["code"], -32602, "missing uri must be INVALID_PARAMS: {r}");

    // 形状纪律：失败响应里**不得**残留 isError（那是 tools/call 的形状，会让官方 SDK 崩）
    let r = rpc(
        &app,
        json!({"jsonrpc":"2.0","id":9,"method":"resources/read","params":{"uri":"ehr://nope"}}),
    )
    .await;
    assert!(
        r.get("result").and_then(|x| x.get("isError")).is_none(),
        "resources/read failure must not expose isError: {r}"
    );

    // 对照：**tools/call** 的失败仍是 isError 形状（两个面的错误信封**不同**，各有各的规范依据）
    let t = rpc(
        &app,
        json!({"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"get_labs","arguments":{"person_id":"00000000-0000-0000-0000-000000000000"}}}),
    )
    .await;
    assert_eq!(t["result"]["isError"], true, "tools/call failure stays isError: {t}");
    assert_eq!(t["result"]["_code"], -32001, "tools/call keeps _code convention: {t}");
}
