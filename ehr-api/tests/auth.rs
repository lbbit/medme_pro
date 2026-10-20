// T4 鉴权门禁：静态 Bearer token（ro/rw 分权）+ 统一中间件拦截 + OpenAPI securitySchemes。
// 验收口径（PLAN T4）：无 token 401、对 token 全绿、scope 不足 403、MCP 工具粒度分权。
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::Value;
use tower::ServiceExt;

fn fixture(scope_spec: &str) -> axum::Router {
    let tmp = std::env::temp_dir().join(format!(
        "ehr-api-auth-test-{}-{}",
        std::process::id(),
        scope_spec.replace([':', ',', '='], "_")
    ));
    std::fs::create_dir_all(tmp.join("vaults")).unwrap();
    let set = ehr_api::auth::TokenSet::parse(scope_spec).expect("token spec parses");
    let st = ehr_api::AppState::from_home_with_auth(&tmp, set);
    ehr_api::router(st)
}

async fn get(app: &axum::Router, path: &str, token: Option<&str>) -> StatusCode {
    let mut b = Request::builder().uri(path);
    if let Some(t) = token {
        b = b.header("Authorization", format!("Bearer {t}"));
    }
    let res = app.clone().oneshot(b.body(Body::empty()).unwrap()).await.unwrap();
    res.status()
}

async fn post(app: &axum::Router, path: &str, token: Option<&str>, body: Value) -> StatusCode {
    let mut b = Request::builder().method("POST").uri(path);
    if let Some(t) = token {
        b = b.header("Authorization", format!("Bearer {t}"));
    }
    let res = app
        .clone()
        .oneshot(b.header("content-type", "application/json").body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    res.status()
}

#[tokio::test(flavor = "multi_thread")]
async fn no_token_is_401_on_protected_paths() {
    let app = fixture("rw1:rw,ro1:ro");
    for p in [
        "/api/v1/persons",
        "/api/v1/persons/abc/timeline",
        "/api/v1/persons/abc/labs",
        "/mcp",
    ] {
        assert_eq!(get(&app, p, None).await, StatusCode::UNAUTHORIZED, "no token on {p} must be 401");
    }
    // 公开路径免鉴权（部署自检 + 文档 + Web 首页）
    assert_eq!(get(&app, "/api/v1/healthz", None).await, StatusCode::OK);
    assert_eq!(get(&app, "/openapi.json", None).await, StatusCode::OK);
    assert_eq!(get(&app, "/", None).await, StatusCode::OK);
}

#[tokio::test(flavor = "multi_thread")]
async fn wrong_token_is_401() {
    let app = fixture("rw1:rw");
    assert_eq!(get(&app, "/api/v1/persons", Some("nope")).await, StatusCode::UNAUTHORIZED);
    // 缺 Bearer 前缀（裸 token）⇒ 401
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/persons")
                .header("Authorization", "rw1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "raw token without Bearer prefix must be 401");
}

#[tokio::test(flavor = "multi_thread")]
async fn valid_tokens_pass() {
    let app = fixture("rw1:rw,ro1:ro");
    assert_eq!(get(&app, "/api/v1/persons", Some("rw1")).await, StatusCode::OK);
    assert_eq!(get(&app, "/api/v1/persons", Some("ro1")).await, StatusCode::OK);
    // 对 token 全绿：建档
    assert_eq!(
        post(&app, "/api/v1/persons", Some("rw1"), serde_json::json!({"name":"鉴权验证人"})).await,
        StatusCode::CREATED
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn ro_token_write_is_403_not_401() {
    let app = fixture("rw1:rw,ro1:ro");
    // 有效 token 但权限不足 ⇒ 403（agent 据此换 token，而不是补 token）
    assert_eq!(
        post(&app, "/api/v1/persons", Some("ro1"), serde_json::json!({"name":"只读不该建成"})).await,
        StatusCode::FORBIDDEN
    );
    // 读侧对 ro token 全绿
    assert_eq!(get(&app, "/api/v1/persons", Some("ro1")).await, StatusCode::OK);
}

#[tokio::test(flavor = "multi_thread")]
async fn mcp_tool_scope_is_enforced_per_tool() {
    let app = fixture("rw1:rw,ro1:ro");
    // ro token 调读侧工具 ⇒ 成功
    let req = Request::builder()
        .method("POST")
        .uri("/mcp")
        .header("Authorization", "Bearer ro1")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"list_persons","arguments":{}}}"#))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let v: Value = serde_json::from_slice(&axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert!(v.get("error").is_none(), "ro token must be able to call read tools: {v}");

    // ro token 调写侧工具 ⇒ JSON-RPC 错误 -32004（HTTP 仍是 200，错误在协议层）
    let req = Request::builder()
        .method("POST")
        .uri("/mcp")
        .header("Authorization", "Bearer ro1")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"create_person","arguments":{"name":"x"}}}"#))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let v: Value = serde_json::from_slice(&axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap()).unwrap();
    // 沿用本项目 tool_err 约定：错误码在 result._code（D-019 同形）
    assert_eq!(v["result"]["_code"], -32004, "ro token on write tool must be -32004: {v}");
    assert_eq!(v["result"]["isError"], true);

    // rw token 调写侧工具 ⇒ 成功
    let req = Request::builder()
        .method("POST")
        .uri("/mcp")
        .header("Authorization", "Bearer rw1")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"create_person","arguments":{"name":"rw 建档"}}}"#))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    let v: Value = serde_json::from_slice(&axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert!(v.get("error").is_none(), "rw token must create person: {v}");
}

#[tokio::test(flavor = "multi_thread")]
async fn open_mode_allows_everything_but_no_token_table() {
    // 未配置 token ⇒ open 模式（向后兼容；启动 [证据] 行会显式标 auth=open）
    let tmp = std::env::temp_dir().join(format!("ehr-api-auth-open-{}", std::process::id()));
    std::fs::create_dir_all(tmp.join("vaults")).unwrap();
    let app = ehr_api::router(ehr_api::AppState::from_home(&tmp));
    assert_eq!(get(&app, "/api/v1/persons", None).await, StatusCode::OK);
    assert_eq!(
        post(&app, "/api/v1/persons", None, serde_json::json!({"name":"open 模式"})).await,
        StatusCode::CREATED
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn openapi_declares_security_scheme() {
    let app = fixture("rw1:rw");
    let res = app
        .clone()
        .oneshot(Request::builder().uri("/openapi.json").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let doc: Value = serde_json::from_slice(&axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap()).unwrap();
    let scheme = &doc["components"]["securitySchemes"]["bearerAuth"];
    assert_eq!(scheme["type"], "http", "bearerAuth must be an http scheme");
    assert_eq!(scheme["scheme"], "bearer");
    assert!(doc["security"].as_array().expect("global security array").len() >= 1,
        "global security requirement must exist");
    // healthz 显式免鉴权
    assert_eq!(doc["paths"]["/api/v1/healthz"]["get"]["security"], serde_json::json!([]));
    // /mcp 仍不进 REST 契约
    assert!(doc["paths"].as_object().unwrap().get("/mcp").is_none());
}
