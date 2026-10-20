// D1 反向保险：CONTRACT 表登记的每个 (method,path) 必须真的挂在路由上并返回
// 非404（stub 允许501），且 /openapi.json 文档里必须有对应条目——防止"加了常量
// 忘了挂 router"或反之的漂移。（main.rs 是 bin 不导出符号，故此处走 HTTP 层验证。）
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::Value;
use tower::ServiceExt; // 无需真 bind 端口。注意:本测试是 CONTRACT 表的第二份手工副本，见下方交叉检查

#[tokio::test]
async fn contract_paths_are_routed_and_documented() {
    // 与 main.rs CONTRACT 保持手工同步的唯一副本；若两者漂移由下方 openapi 交叉检查兜底
    const CONTRACT: &[(&str, &str)] = &[
        ("get", "/api/v1/persons/{pid}/medications"),
        ("get", "/api/v1/persons/{pid}/conditions"),
        ("get", "/api/v1/persons/{pid}/measurements"),
        ("post", "/api/v1/persons/{pid}/entries/self-measurement"),
        ("post", "/api/v1/persons/{pid}/entries/note"),
        ("post", "/api/v1/persons/{pid}/entries/profile-event"),
        ("put", "/api/v1/documents/{sha}/extraction"),
        ("get", "/api/v1/audit"),
    ];

    let tmp = std::env::temp_dir().join(format!("ehr-api-contract-test-{}", std::process::id()));
    std::fs::create_dir_all(tmp.join("vaults")).unwrap();
    let st = ehr_api::AppState::from_home(&tmp);
    let app = ehr_api::router(st);

    // 1) 路由层：每个契约路径必须命中（405/501/2xx 都算挂上了；404=漏挂 router）
    for (method, path) in CONTRACT {
        let concrete = path.replace("{pid}", "p1").replace("{sha}", "aa");
        let req = Request::builder()
            .method(*method)
            .uri(concrete.clone())
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_ne!(res.status(), StatusCode::NOT_FOUND, "route missing for {method} {concrete}");
    }

    // 2) OpenAPI 文档层：模板路径（含 {var}）必须出现在 /openapi.json paths 中
    let doc: Value = {
        let res = app
            .clone()
            .oneshot(Request::builder().uri("/openapi.json").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK, "/openapi.json must be served");
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    };
    let paths = doc["paths"].as_object().expect("paths object in openapi");
    for (method, path_tpl) in CONTRACT {
        let entry = paths.get(*path_tpl)
            .unwrap_or_else(|| panic!("openapi missing template path {path_tpl}"));
        assert!(entry.get(method.to_lowercase().as_str()).is_some(),
            "openapi missing operation {method} on {path_tpl}");
    }

    // 3) 已挂路由但未进 OpenAPI 契约的人员域端点（理由见 lib.rs stub_person_* 注释）：只守卫挂载
    const MOUNTED_UNDOCUMENTED: &[(&str, &str)] = &[
        ("get", "/api/v1/persons/{pid}"),
        ("patch", "/api/v1/persons/{pid}"),
        ("delete", "/api/v1/persons/{pid}"),
        // timeline on unknown pid：必须走真 handler 的存在性校验(也返回404但来自业务层)——路由存在性守卫同前。
        ("get", "/api/v1/persons/no-such-person/timeline"),
    ];
    for (method, path) in MOUNTED_UNDOCUMENTED {
        let concrete = path.replace("{pid}", "0e9a2f6c-5305-4187-b283-864f6ac6f0d7");
        let req = Request::builder().method(*method).uri(concrete.clone()).body(Body::empty()).unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_ne!(res.status(), StatusCode::NOT_FOUND, "route missing for {method} {concrete}");
    }

    // 4) Web 管理端最小版（D-016）：GET / 必须回 HTML（内容非空 + content-type text/html）。
    //    守卫点：include_str! 的 web/index.html 若被移出构建上下文，编译期即失败；
    //    路由若被摘掉，下面断言失败——「页面在库里但没人能打开」是本项目 D5 类漏账。
    {
        let res = app
            .clone()
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK, "web index must be served at /");
        let ct = res.headers().get(axum::http::header::CONTENT_TYPE).cloned();
        let is_html = ct
            .as_ref()
            .map(|v| v.to_str().unwrap_or("").starts_with("text/html"))
            .unwrap_or(false);
        assert!(is_html, "web index content-type must be text/html, got {ct:?}");
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
        let html = String::from_utf8_lossy(&bytes).to_string();
        assert!(html.len() > 2000, "web index looks truncated: {} bytes", html.len());
        assert!(html.contains("/api/v1"), "web index must call the JSON API same-origin");
    }
}
