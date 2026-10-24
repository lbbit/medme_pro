// D-038 · T21：**运行时响应形状** vs **OpenAPI 声明的响应形状** —— 端点级漂移门禁。
//
// 为什么要有这道门（D-037 的「第三种形态」续集）：
//   D-037 治的是「**端点在不在** stub 清单里」；D-025 治的是「**文档有没有漏端点**」；
//   但**两者都没问**：「OpenAPI 说的这个 200 响应长什么样，**和真回的是不是同一个东西**？」
//   ⇒ 本轮实测抓到三个**真缺陷**（都是「文档在骗人」）：
//     ① `GET /persons/{pid}/timeline`   OpenAPI 说 200 = **数组** `[TimelineItem]`
//                                     真回的是**对象** `{person_id,count,items}`；
//     ② `GET /persons/{pid}/search`     同上；
//     ③ `GET /persons/{pid}/documents`  同上；
//     ④ `GET /capabilities` / `GET /healthz`  OpenAPI **根本没声明 body**（客户端拿不到类型）。
//   照 OpenAPI 生成的客户端会把响应当数组解 ⇒ **直接解不出 / 静默拿到空**。
//
// 判据形状（D-009「判据要钉住语义，不是钉住快照」）：
//   **不是**「响应是对象」这种单点断言，也**不是**「字段数 == N」的快照；
//   而是**逐条把 OpenAPI 声明的 schema 拿来校验真响应**——
//   ①顶层 JSON 类型必须与 schema 的 `type` 一致（object / array）；
//   ②schema 声明的每个 `required` 字段必须**真的出现在**响应里；
//   ③**响应里的每个键都必须在 schema 的 `properties` 里声明过**
//     （反向方向：「文档没说、实际有」也是漂移——客户端按文档生成的类型会静默丢掉这个键）。
//   ⇒ ④schema 里**非 required** 的属性**允许在响应里不存在**——因为
//     `Option` + `#[serde(skip_serializing_if = "Option::is_none")]` 的语义是
//     「无值时**整个键不存在**，不是空值」（rest-contracts.md 不变量 4 已文档化）。
//
// ⚠️ 关键纪律（照抄 D-037 的教训）：判据**不能是同义反复**。
//   这里比对的是**两份独立产物**：①OpenAPI 渲染出的 schema（utoipa derive 的产物）
//   vs ②**真跑一次 handler 拿到的响应体**。二者不同源 ⇒ 漂移必红。
//
// ⚠️ 探针自身的坑（D-011 家族）：本测试**自己起真服务**（oneshot 打真 router），
//   不解析源码、**不手抄端点清单** ⇒ 探针本身没有可漂移的「第二份实现」。
//
// ⚠️⚠️ T22（D-039）修掉本文件**自己的一道假门**：
//   本文件此前在注释里声称「覆盖**全部**端点（不手抄清单）——清单从 OpenAPI 派生」，
//   **但代码里其实是一份手写的 12 条 `probes` 数组** ⇒ **只覆盖了 19 个 operation 里的 12 个**，
//   7 个写侧 operation（POST persons / POST entries×3 / PUT extraction / POST upload / GET original）
//   **完全没被断言**。⇒ 「宣称覆盖全部」与「实际覆盖」分叉 = 又一道**一直在测错对象的门**
//   （D-037 的同族病，且这次在**自己新写的门**上复发）。
//   本轮修法：**覆盖率本身变成判据**——
//     ①探针表 `probes` 显式登记**每个 operation**（METHOD + path + 怎么造出 2xx）；
//     ②新门 `every_openapi_operation_is_covered_by_a_probe`：**从 OpenAPI 派生的 operation 集合
//       ⊆ 探针覆盖的 operation 集合**——少一个就红（⇒ 「加端点忘了加探针」会立刻红，
//       不再靠注释里的口头承诺）。
//   ⇒ 判据形状从「手抄清单 + 注释承诺」变成「**从实现派生的集合 ⊆ 探针集合**」。
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::Value;
use tower::ServiceExt;

/// 从 OpenAPI 的 `components.schemas` 里取一个 schema（支持 `$ref` 跳转）。
fn resolve<'a>(doc: &'a Value, schema: &'a Value) -> &'a Value {
    if let Some(r) = schema.get("$ref").and_then(|x| x.as_str()) {
        let name = r.rsplit('/').next().unwrap();
        return &doc["components"]["schemas"][name];
    }
    schema
}

/// 校验「真响应」是否符合「schema 声明」。
///
/// 规则（与上面的判据形状说明一一对应）：
/// - 顶层：`schema.type` = "object" ⇒ 响应必须是 JSON 对象；= "array" ⇒ 必须是数组。
/// - `required` 里的每个键必须**出现在**响应里（缺了就是漂移）。
/// - **响应里的每个键都必须在 schema 的 `properties` 里声明**（反向方向：
///   「实际有、文档没说」也是漂移——客户端按文档生成的类型会静默丢掉这个键）。
/// - schema 里**非 required** 的属性**允许在响应里不存在**——因为
///   `Option` + `#[serde(skip_serializing_if = "Option::is_none")]` 的语义是
///   「无值时**整个键不存在**，不是空值」（rest-contracts.md 不变量 4 已文档化）。
///   ⇒ 这条不能用「schema 属性 ⊆ 响应键」来判，否则会把**合法的可选键**判成漂移
///   （本轮第一次跑就撞到了：`extraction` 无值时整个键不存在 ⇒ 报「文档与现实脱节」，
///    而它其实**是**文档化过的正常语义 —— **门自己红得不对**，D-011 家族：先怀疑门）。
fn assert_conforms(doc: &Value, schema: &Value, body: &Value, where_: &str) {
    let s = resolve(doc, schema);
    let styp = s.get("type").and_then(|x| x.as_str());
    match styp {
        Some("object") => assert!(
            body.is_object(),
            "{where_}: OpenAPI 声明 200 是 object，真响应却是 {}",
            if body.is_array() { "array".to_string() } else { body.to_string() }
        ),
        Some("array") => assert!(
            body.is_array(),
            "{where_}: OpenAPI 声明 200 是 array，真响应却是 object —— 客户端按数组解会直接失败"
        ),
        _ => {} // 无 type 声明（如 oneOf/自由对象）⇒ 本门不判
    }
    if !body.is_object() {
        return;
    }
    let obj = body.as_object().unwrap();
    if let Some(req) = s.get("required").and_then(|x| x.as_array()) {
        for r in req {
            let k = r.as_str().unwrap();
            assert!(
                obj.contains_key(k),
                "{where_}: OpenAPI 声明 required 含 `{k}`，真响应里没有这个键（响应键={:?}）",
                obj.keys().collect::<Vec<_>>()
            );
        }
    }
    if let Some(props) = s.get("properties").and_then(|x| x.as_object()) {
        // 反向方向：**响应里多出来的键**必须被 schema 声明过。
        // （「文档没说、实际有」⇒ 按文档生成的客户端类型会静默丢掉这个键。）
        for k in obj.keys() {
            assert!(
                props.contains_key(k),
                "{where_}: 真响应有键 `{k}`，但 OpenAPI 的 schema **没声明**它 \
                 （响应键={:?}）—— 文档与现实脱节（D5：形状不许粉饰）",
                obj.keys().collect::<Vec<_>>()
            );
        }
    }
}

fn tmp_home(tag: &str) -> std::path::PathBuf {
    let h = std::env::temp_dir().join(format!("ehr-shape-test-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&h);
    std::fs::create_dir_all(h.join("vaults")).unwrap();
    h
}

/// 造一个**有数据**的档（否则列表类端点回空数组，`required` 与 `properties` 的断言会落空）。
///
/// ⚠️ 为什么必须造数据：空响应下「对象 vs 数组」照样能判（顶层形状不依赖数据），
///   但「schema 声明的属性是否真在响应里」需要**至少一条记录**才有意义
///   ⇒ 顶层形状与内层形状要**分别**测（下面 `nested_*` 两门）。
fn fixture(tag: &str) -> (std::path::PathBuf, String) {
    let home = tmp_home(tag);
    let store = ehr_api::PersonStore::new(home.clone());
    let p = store
        .create(&ehr_api::PersonNew {
            name: "形状门禁·甲".into(),
            birthday: Some("1990-01-02".into()),
            gender: Some("male".into()),
            id_note: None,
        })
        .unwrap();
    let vault = ehr_api::vaults::open_person_vault(&home, &p.id).unwrap();
    ehr_api::vaults::ingest_upload(
        &vault,
        "血常规.txt",
        "XX医院检验科报告单\n姓名:形状门禁·甲\n白细胞计数 6.2 10^9/L 3.5-9.5\n血红蛋白 132 g/L 115-150\n"
            .as_bytes(),
    )
    .unwrap();
    (home, p.id)
}

async fn get_body(app: &axum::Router, uri: &str) -> (StatusCode, Value) {
    let (status, _ct, v) = get_raw(app, uri).await;
    (status, v)
}

/// 取**原始**响应：状态码 + `Content-Type` + 体（体按 JSON 尽力解析，失败则为 Null）。
///
/// ⚠️ 为什么要带 `Content-Type`：`GET .../original` 回的是 **`application/octet-stream`**
/// （CAS 原始字节），**不是 JSON** ⇒ 形状门对它的判据不是「JSON schema」，
/// 而是「**回的类型与声明的类型一致**」（D5：形状不许粉饰，二进制端点也不能被当成 JSON 端点）。
/// DELETE 探针（T46，带认证）：create 后表非空 ⇒ 管理/删除都需要 rw 明文。
async fn delete_json_auth(app: &axum::Router, uri: &str, token: &str) -> (StatusCode, Value) {
    let res = app
        .clone()
        .oneshot(Request::delete(uri)
            .header(axum::http::header::AUTHORIZATION, format!("Bearer {token}"))
            .body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 20).await.unwrap();
    let v: Value = if bytes.is_empty() { Value::Null } else { serde_json::from_slice(&bytes).unwrap_or(Value::Null) };
    (status, v)
}

/// DELETE 探针（T46）：回 JSON body。
async fn delete_json(app: &axum::Router, uri: &str) -> (StatusCode, Value) {
    let res = app
        .clone()
        .oneshot(Request::delete(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 20).await.unwrap();
    let v: Value = if bytes.is_empty() { Value::Null } else { serde_json::from_slice(&bytes).unwrap_or(Value::Null) };
    (status, v)
}

async fn get_raw(app: &axum::Router, uri: &str) -> (StatusCode, String, Value) {
    let res = app
        .clone()
        .oneshot(Request::get(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let ct = res
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 20).await.unwrap();
    let v: Value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, ct, v)
}

/// 发一个 JSON 请求（写侧端点用）。
async fn post_json(
    app: &axum::Router,
    uri: &str,
    body: Value,
) -> (StatusCode, Value) {
    let res = app
        .clone()
        .oneshot(
            Request::post(uri)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 20).await.unwrap();
    let v: Value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, v)
}

/// 发一个 multipart/form-data 上传（`POST /persons/{pid}/documents`）。
async fn post_multipart(
    app: &axum::Router,
    uri: &str,
    filename: &str,
    content: &str,
) -> (StatusCode, Value) {
    let boundary = "SHAPE-BOUND";
    let body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\n\
         Content-Type: text/plain\r\n\r\n{content}\r\n--{boundary}--\r\n"
    );
    let res = app
        .clone()
        .oneshot(
            Request::post(uri)
                .header(
                    "content-type",
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 20).await.unwrap();
    let v: Value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, v)
}

/// 探针结果：一个 operation 的「真响应」。
struct Probed {
    label: String,
    body: Value,
    /// 真响应的 `Content-Type`（**非 JSON 端点**用它对账——如 `/original` 的 octet-stream）。
    ct: String,
}

/// **全量探针**：对**每个** operation 真打一次，拿到 2xx 的响应体。
///
/// ⚠️ 关键设计（T22 / D-039）：**探针表必须覆盖 OpenAPI 的全部 operation**，
///   且「覆盖」这件事**由下一门机器核对**（`every_openapi_operation_is_covered_by_a_probe`），
///   **不靠注释里的口头承诺**——本文件上一版就栽在这（注释说「覆盖全部」，实际只覆盖 12/19）。
///
/// 探针的「怎么造出 2xx」各不相同（建档/上传/写条目…），所以这里必须有一份
/// **显式登记**；但登记的**完整性**由机器核对，而不是靠人记得。
async fn probe_all(app: &axum::Router, pid: &str) -> Vec<Probed> {
    let mut out: Vec<Probed> = Vec::new();

    // --- 建档（POST /api/v1/persons）→ 拿到 201 回执 ---
    let (st, v) = post_json(
        app,
        "/api/v1/persons",
        serde_json::json!({"name": "探针·乙", "gender": "female", "birthday": "1991-03-04"}),
    )
    .await;
    assert!(st == StatusCode::CREATED, "POST /persons 探针自身失败：{st}");
    out.push(Probed { label: "POST /api/v1/persons".into(), body: v, ct: "application/json".into() });

    // --- 上传原件（POST /api/v1/persons/{pid}/documents）→ 拿到 content_hash ---
    let (st, v) = post_multipart(
        app,
        &format!("/api/v1/persons/{pid}/documents"),
        "上传件.txt",
        "姓名:形状门禁·甲\n白细胞计数 7.7 10^9/L\n",
    )
    .await;
    assert!(st.is_success(), "POST /documents 探针自身失败：{st}");
    let uploaded_sha = v["content_hash"].as_str().expect("上传回执无 content_hash").to_string();
    out.push(Probed { label: "POST /api/v1/persons/{pid}/documents".into(), body: v, ct: "application/json".into() });

    // --- 三类条目（POST .../entries/*）→ 201 ---
    for (path, body, label) in [
        (
            "/api/v1/persons/{pid}/entries/self-measurement",
            serde_json::json!({"values":[{"analyte_key":"heart_rate","value":72.0,"unit":"bpm"}]}),
            "POST /api/v1/persons/{pid}/entries/self-measurement",
        ),
        (
            "/api/v1/persons/{pid}/entries/note",
            serde_json::json!({"text":"探针笔记：今天感觉良好。"}),
            "POST /api/v1/persons/{pid}/entries/note",
        ),
        (
            "/api/v1/persons/{pid}/entries/profile-event",
            serde_json::json!({"kind":"enable","package":"sle","at":"2025-01-02","payload":{"x":1}}),
            "POST /api/v1/persons/{pid}/entries/profile-event",
        ),
    ] {
        let (st, v) = post_json(app, &path.replace("{pid}", pid), body).await;
        assert!(st == StatusCode::CREATED, "{label} 探针自身失败：{st}");
        out.push(Probed { label: label.into(), body: v, ct: "application/json".into() });
    }

    // --- 读侧（GET）---
    let sha = {
        let (_s, v) = get_body(app, &format!("/api/v1/persons/{pid}/documents")).await;
        v["items"][0]["content_hash"].as_str().unwrap().to_string()
    };
    for (label, uri) in [
        ("GET /api/v1/healthz", "/api/v1/healthz".to_string()),
        ("GET /api/v1/capabilities", "/api/v1/capabilities".to_string()),
        ("GET /api/v1/persons", "/api/v1/persons".to_string()),
        ("GET /api/v1/audit", "/api/v1/audit".to_string()),
        ("GET /api/v1/persons/{pid}/timeline", format!("/api/v1/persons/{pid}/timeline")),
        ("GET /api/v1/persons/{pid}/search", format!("/api/v1/persons/{pid}/search?q=白细胞")),
        ("GET /api/v1/persons/{pid}/labs", format!("/api/v1/persons/{pid}/labs")),
        ("GET /api/v1/persons/{pid}/medications", format!("/api/v1/persons/{pid}/medications")),
        ("GET /api/v1/persons/{pid}/conditions", format!("/api/v1/persons/{pid}/conditions")),
        ("GET /api/v1/persons/{pid}/measurements", format!("/api/v1/persons/{pid}/measurements")),
        ("GET /api/v1/persons/{pid}/documents", format!("/api/v1/persons/{pid}/documents")),
        (
            "GET /api/v1/persons/{pid}/documents/{sha}",
            format!("/api/v1/persons/{pid}/documents/{sha}"),
        ),
        (
            "GET /api/v1/persons/{pid}/documents/{sha}/original",
            format!("/api/v1/persons/{pid}/documents/{sha}/original"),
        ),
    ] {
        let (status, ct, body) = get_raw(app, &uri).await;
        assert!(status.is_success(), "{label} 真请求失败：{status}（探针自身出错，D011 家族）");
        out.push(Probed { label: label.into(), body, ct });
    }

    // --- PUT extraction（200；需真 deid schema，否则 400）---
    // ⚠️ 这里用**能被 deid::verify 逐字核验**的最小抽取：evidence 必须是原文逐字子串。
    let text = "白细胞计数 7.7 10^9/L";
    let result_json = serde_json::json!({
        "schema": 1,
        "labs": [{
            "key": "wbc_count",
            "label": "白细胞计数",
            "value": "7.7",
            "unit": "10^9/L",
            "evidence": text
        }],
        "meds": [],
        "diagnoses": []
    })
    .to_string();
    let (st, v) = {
        let res = app
            .clone()
            .oneshot(
                Request::put(format!("/api/v1/persons/{pid}/documents/{uploaded_sha}/extraction"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "result_json": result_json,
                            "backend": "llm-http",
                            "model_version": "probe-1",
                            "mode": "text",
                            "schema": 1
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = res.status();
        let bytes = axum::body::to_bytes(res.into_body(), 1 << 20).await.unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, v)
    };
    assert!(
        st.is_success(),
        "PUT extraction 探针自身失败：{st}（体={}）",
        serde_json::to_string(&v).unwrap()
    );
    out.push(Probed { label: "PUT /api/v1/persons/{pid}/documents/{sha}/extraction".into(), body: v, ct: "application/json".into() });


    // --- T46 token 管理面三探针（POST/GET/DELETE /api/v1/tokens…）---
    // 管理面要求 rw —— 测试路由是 open 模式（无 token 表）⇒ 放行，直接打即可。
    let (st, v) = post_json(
        app,
        "/api/v1/tokens",
        serde_json::json!({"name": "probe-token", "scope": "rw"}),
    )
    .await;
    assert!(st.is_success(), "POST /tokens 探针自身失败：{st}");
    out.push(Probed { label: "POST /api/v1/tokens".into(), body: v.clone(), ct: "application/json".into() });

    // ⚠️ create 之后表非空 ⇒ 管理面立即要求 rw（open 模式被第一条 token 关闭——安全语义）。
    //    后续 GET/DELETE 必须带刚生成的明文 token（探针自证：生成→用→吊销全链）。
    let created_token = v["token"].as_str().expect("create 回执无 token").to_string();

    let (st, v) = {
        let res = app.clone()
            .oneshot(Request::get("/api/v1/tokens")
                .header(axum::http::header::AUTHORIZATION, format!("Bearer {created_token}"))
                .body(Body::empty()).unwrap())
            .await.unwrap();
        let status = res.status();
        let bytes = axum::body::to_bytes(res.into_body(), 1 << 20).await.unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, v)
    };
    assert!(st.is_success(), "GET /tokens 探针自身失败：{st}");
    out.push(Probed { label: "GET /api/v1/tokens".into(), body: v, ct: "application/json".into() });

    let (st, v) = delete_json_auth(app, "/api/v1/tokens/probe-token", &created_token).await;
    assert!(st.is_success(), "DELETE /tokens 探针自身失败：{st}");
    out.push(Probed { label: "DELETE /api/v1/tokens/{name}".into(), body: v, ct: "application/json".into() });

    out
}

/// 顶层形状门：**每个** OpenAPI operation 的 2xx 响应都必须符合它声明的 schema。
#[tokio::test(flavor = "multi_thread")]
async fn every_declared_response_body_matches_the_openapi_schema() {
    let (home, pid) = fixture("top");
    let app = ehr_api::router(ehr_api::AppState::from_home(home.clone()));
    let doc = ehr_api::openapi_json();

    let probed = probe_all(&app, &pid).await;
    let mut checked = 0usize;
    for p in &probed {
        let (method, path) = p.label.split_once(' ').unwrap();
        let op = &doc["paths"][path][method.to_lowercase()];
        let resp = &op["responses"];
        // 成功码可能是 200 或 201（写侧）——取**第一个带 content 的成功响应**。
        // 成功码可能是 200 或 201（写侧）——取**第一个带 content 的成功响应**。
        // ⚠️ 选 content-type 的规则：**优先 application/json**；
        //   没有 json 时才取任意一个（`GET .../original` 只有 `application/octet-stream`，
        //   它**本就没有** JSON schema——二进制端点不该被 JSON 判据套）。
        let (code, ctype, schema) = ["200", "201"]
            .iter()
            .find_map(|c| {
                let content = resp[*c]["content"].as_object()?;
                let all: Vec<(String, Value)> = content
                    .iter()
                    .map(|(ct, v)| (ct.clone(), v.get("schema").cloned().unwrap_or(Value::Null)))
                    .collect();
                let pick = all
                    .iter()
                    .find(|(ct, _)| ct == "application/json")
                    .or_else(|| all.first())?;
                Some(((*c).to_string(), pick.0.clone(), pick.1.clone()))
            })
            .unwrap_or_else(|| {
                panic!(
                    "{}: OpenAPI 的成功响应**没有 body 声明** ⇒ 生成的客户端拿不到类型 \
                     （本次修复的缺陷④：healthz / capabilities 此前就是如此）",
                    p.label
                )
            });
        // 缺陷④的门：声明了 `application/json` 却**没有 schema** ⇒ 客户端拿不到类型。
        if ctype == "application/json" {
            assert!(
                !schema.is_null(),
                "{}: OpenAPI 声明 `application/json` 却**没有 schema** ⇒ 生成的客户端拿不到类型 \
                 （缺陷④：healthz / capabilities 此前就是如此）",
                p.label
            );
        }
        // ⚠️ 声明的 content-type 与真响应的必须**一致**（D5：形状不许粉饰）。
        //   对 `application/octet-stream`（`GET .../original` 回 CAS 原始字节）**不适用 JSON schema**，
        //   但「回的类型 == 声明的类型」这条**适用**——二进制端点不能被当成 JSON 端点。
        let true_ct = p.ct.split(';').next().unwrap_or("").trim();
        // ⚠️ 声明可以是**通配** `*/*`（D-039）：`GET .../original` 的 Content-Type 是
        //   **动态的**（取自原件存储的 mime：text/plain / application/pdf / …），
        //   写死任何一个具体类型都是假话 ⇒ 声明为 `*/*`（OpenAPI 3.1 允许通配键）。
        //   判据形状：**声明必须覆盖真值**（通配 ⇒ 恒覆盖；否则必须逐字相等）。
        if ctype != "*/*" {
            assert_eq!(
                true_ct, ctype,
                "{}: 真响应 Content-Type=`{}` 与 OpenAPI 声明的 `{}` **不一致**",
                p.label, true_ct, ctype
            );
        }
        if ctype == "application/json" {
            assert_conforms(&doc, &schema, &p.body, &format!("{} ({})", p.label, code));
        } else {
            // 非 JSON 端点：body 解析成 Value 必然失败 ⇒ 不能拿 JSON schema 判；
            // 但**必须**确认它真的不是 JSON（否则说明 content-type 撒谎）。
            assert!(
                p.body.is_null(),
                "{}: 声明 `{}` 却解析出了 JSON ⇒ content-type 与实际体不符",
                p.label, ctype
            );
        }
        checked += 1;
    }
    assert!(checked >= 19, "只测了 {checked} 个 operation——探针覆盖不足");
}

/// **覆盖率门（T22 / D-039）**：OpenAPI 的 operation 集合 ⊆ 探针覆盖的 operation 集合。
///
/// 这是本文件**自己**的「不手抄清单」承诺的机器化版本：
///   上一版注释写「覆盖**全部**端点（不手抄清单）」，实际只有一份手写的 12 条探针
///   ⇒ 7 个写侧 operation 从未被断言，却**看起来**已被覆盖。
///   现在：加了新端点但忘了加探针 ⇒ **这一门立刻红**（operation 集合 ⊄ 探针集合）。
///
/// ⚠️ 判据形状：**集合包含**，不是数量比较（「≥ N」那种快照式断言）——
///   数量对了但端点名错了，照样是漏（D-009）。
#[tokio::test(flavor = "multi_thread")]
async fn every_openapi_operation_is_covered_by_a_probe() {
    let (home, pid) = fixture("cover");
    let app = ehr_api::router(ehr_api::AppState::from_home(home.clone()));
    let doc = ehr_api::openapi_json();

    // 真源：运行时 OpenAPI 的 operation 集合。
    let mut want: Vec<String> = Vec::new();
    for (p, item) in doc["paths"].as_object().unwrap() {
        for (m, op) in item.as_object().unwrap() {
            if op.is_object() && op.get("responses").is_some() {
                want.push(format!("{} {}", m.to_uppercase(), p));
            }
        }
    }
    want.sort();

    // 探针覆盖集合（由 probe_all 实际打过的端点登记）。
    let probed = probe_all(&app, &pid).await;
    let mut got: Vec<String> = probed.iter().map(|p| p.label.clone()).collect();
    got.sort();

    let missing: Vec<&String> = want.iter().filter(|w| !got.contains(w)).collect();
    let extra: Vec<&String> = got.iter().filter(|g| !want.contains(g)).collect();
    assert!(
        missing.is_empty(),
        "OpenAPI 里有 operation **没有探针** ⇒ 它们从未被形状门断言过：{missing:?}\n\
         （全量：{want:?}）"
    );
    assert!(
        extra.is_empty(),
        "探针里有 OpenAPI **不存在**的 operation（要么实现已删，要么探针写错了路径）：{extra:?}"
    );
    assert_eq!(got, want, "探针集合与 OpenAPI operation 集合必须**逐条相等**");
}

/// 内层形状门：`items[]` / `series[]` 等**数组元素**也必须符合 OpenAPI 声明的 item schema。
///
/// 上一门只管顶层（object vs array）；「顶层对、元素错」同样是漂移
/// （例如 items 声明是 `TimelineItem` 但真回的是 `DocumentListItem`）。
#[tokio::test(flavor = "multi_thread")]
async fn array_elements_match_their_declared_item_schema() {
    let (home, pid) = fixture("nested");
    let app = ehr_api::router(ehr_api::AppState::from_home(home.clone()));
    let doc = ehr_api::openapi_json();

    // (路径, 顶层属性名, item schema 名)
    let probes: Vec<(&str, &str, &str)> = vec![
        ("/api/v1/persons/{pid}/timeline", "items", "TimelineItem"),
        ("/api/v1/persons/{pid}/search", "items", "SearchItem"),
        ("/api/v1/persons/{pid}/documents", "items", "DocumentListItem"),
    ];
    let mut checked = 0usize;
    for (path, prop, item_schema) in probes {
        let uri = path.replace("{pid}", &pid).replace("?", "");
        let uri = if path.contains("search") { format!("/api/v1/persons/{pid}/search?q=白细胞") } else { uri };
        let (status, body) = get_body(&app, &uri).await;
        assert!(status.is_success(), "{path} 真请求失败：{status}");
        let items = body[prop].as_array().unwrap_or_else(|| panic!("{path}: {prop} 不是数组"));
        assert!(!items.is_empty(), "{path}: {prop} 为空——探针没造出数据，判据落空");
        let schema = doc["components"]["schemas"][item_schema].clone();
        for it in items {
            assert_conforms(&doc, &schema, it, &format!("{path} {prop}[]"));
        }
        checked += 1;
    }
    assert_eq!(checked, 3, "探针覆盖不足");
}

/// **反向保险**：若有人把某个端点的 200 声明改回 `Vec<T>`（裸数组），
/// 而真回的是对象，本门必须红（⇒ 证明它不是「同义反复」）。
///
/// 这门是**负向证据**的固化：把 `TimelineReply` 换回 `Vec<TimelineItem>` 声明，
/// 第一门立刻红在「OpenAPI 声明 200 是 array，真响应却是 object」。
#[tokio::test(flavor = "multi_thread")]
async fn envelope_endpoints_are_declared_as_objects_not_bare_arrays() {
    let doc = ehr_api::openapi_json();
    // 这三个端点的真回执是**对象信封**（person_id/count/items）⇒ OpenAPI 必须声明为 object。
    for path in [
        "/api/v1/persons/{pid}/timeline",
        "/api/v1/persons/{pid}/search",
        "/api/v1/persons/{pid}/documents",
    ] {
        let sch = &doc["paths"][path]["get"]["responses"]["200"]["content"]["application/json"]["schema"];
        let s = resolve(&doc, sch);
        assert_eq!(
            s.get("type").and_then(|x| x.as_str()),
            Some("object"),
            "{path} 的 200 声明不是 object（多半又写成裸数组了）⇒ 客户端按数组解会失败"
        );
        let props = s.get("properties").and_then(|x| x.as_object()).unwrap();
        assert!(props.contains_key("items"), "{path} 的信封里必须有 items 数组");
        assert!(props.contains_key("count"), "{path} 的信封里必须有 count");
    }
}
