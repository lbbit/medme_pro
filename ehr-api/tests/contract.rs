// D1 反向保险：**OpenAPI 里公布的每个 operation 都必须真挂在路由上**（防止"文档里有、router 里没有"），
// 且 CONTRACT 表登记的每个 (method,path) 必须真的返回 501（防止"实装了却还留着 stub 契约"）。
//
// T16 · D-032 未销账⑩：本测试原先**手抄了一份**端点清单（8 条）来断言「挂了路由」，
// 那是「同一事实两份实现」——加一条路由要记得改这张手抄表，漏改则**新端点完全不被守卫**
// （守卫看起来是绿的，实则漏了新端点）。现在改为**从运行时 OpenAPI 派生**：
// 文档里有的每个 operation，逐条真打一次请求 ⇒ 漏挂路由必红。
//
// 怎么区分「路由没挂（axum 默认 404）」与「业务层的 404（查无此人/查无此档）」？
// ⇒ 看响应体：业务 404 是 JSON {"error": ...}；axum 的路由未命中 404 是**空体**。
//   （这正是「结构判据」优于「状态码快照」的又一处同族示范，D-009 的老教训。）
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::Value;
use tower::ServiceExt; // 无需真 bind 端口

fn concrete(path: &str) -> String {
    path.replace("{pid}", "0e9a2f6c-5305-4187-b283-864f6ac6f0d7")
        .replace("{sha}", &"ab".repeat(32))
}

#[tokio::test(flavor = "multi_thread")]
async fn contract_paths_are_routed_and_documented() {
    let tmp = std::env::temp_dir().join(format!("ehr-api-contract-test-{}", std::process::id()));
    std::fs::create_dir_all(tmp.join("vaults")).unwrap();
    let st = ehr_api::AppState::from_home(&tmp);
    let app = ehr_api::router(st);

    // 1) 文档层：取运行时 OpenAPI（唯一事实源）
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
    assert!(!paths.is_empty(), "openapi must publish at least one path");

    // 2) 路由层：**每个** OpenAPI operation 都必须真挂上（不止是手抄清单里的那几条）
    //    ⚠️ 方法名必须**大写**：http::Method::from_str("put") 会得到一个**小写扩展方法**（≠ PUT），
    //    axum 路由匹配不上 ⇒ 405 而非 404 —— 差点被误读成「端点没实现」。
    //    （本项目老坑同族：门禁自身出错时，报错会伪装成产品 bug。D-011「红门先查门禁自身」。）
    let mut checked = 0usize;
    for (path_tpl, item) in paths {
        for method in item.as_object().expect("path item object").keys() {
            let req = Request::builder()
                .method(method.to_uppercase().as_str())
                .uri(concrete(path_tpl))
                .body(Body::empty())
                .unwrap();
            let res = app.clone().oneshot(req).await.unwrap();
            let status = res.status();
            if status == StatusCode::NOT_FOUND {
                // 404 可能是业务层（合法）也可能是路由未挂（漏账）——用**响应体**区分。
                let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
                let body = String::from_utf8_lossy(&bytes).to_string();
                assert!(
                    body.contains("\"error\""),
                    "route missing for {method} {path_tpl} (axum 404 with empty body = not mounted)"
                );
            }
            // 405 = 方法不被接受（路由在，但这个 method 没挂）——同样算漏账
            assert_ne!(
                status,
                StatusCode::METHOD_NOT_ALLOWED,
                "method {method} not accepted on {path_tpl} (405 = operation declared but not routed)"
            );
            checked += 1;
        }
    }
    assert!(checked >= 18, "expected the full REST surface, checked only {checked}");

    // 3) CONTRACT 表（唯一事实源 = ehr_api::CONTRACT）：登记为 stub 的必须真返回 501
    //    （反之：实装后忘了从表里移除 ⇒ OpenAPI 仍宣称 501 而实际 200 = 文档粉饰状态码，D5）
    for (method, path_tpl, _tag, _summary) in ehr_api::CONTRACT {
        let req = Request::builder()
            .method(method.to_uppercase().as_str())
            .uri(concrete(path_tpl))
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(
            res.status(),
            StatusCode::NOT_IMPLEMENTED,
            "CONTRACT entry {method} {path_tpl} must really return 501 (remove it from CONTRACT when implemented)"
        );
        // 且必须在 OpenAPI 里被标成 stub（summary 含 STUB_MARK）
        let op = &paths[*path_tpl][method.to_lowercase().as_str()];
        let summary = op.get("summary").and_then(|s| s.as_str()).unwrap_or("");
        assert!(
            summary.contains(ehr_api::STUB_MARK),
            "openapi summary for {method} {path_tpl} must carry the stub mark, got {summary:?}"
        );
    }

    // 4) 已挂路由但未进 OpenAPI 契约的人员域端点（理由见 lib.rs stub_person_* 注释）：只守卫挂载
    //    T16：这张表**只有一份**——直接读 ehr_api::MOUNTED_UNDOCUMENTED，不再各写一份
    //    （同一事实两份实现 = 漂移风险；D2：一个语义只允许一处实现）。
    for (method, path) in ehr_api::MOUNTED_UNDOCUMENTED {
        let concrete = path.replace("{pid}", "0e9a2f6c-5305-4187-b283-864f6ac6f0d7");
        let req = Request::builder().method(*method).uri(concrete.clone()).body(Body::empty()).unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_ne!(res.status(), StatusCode::NOT_FOUND, "route missing for {method} {concrete}");
    }
    // timeline on unknown pid：必须走真 handler 的存在性校验(也返回404但来自业务层)——路由存在性守卫同前。
    //    ⚠️ 判据形状：这里**不能**用 `assert_ne!(404)`——业务层「查无此人」本来就返回 404（合法答案）。
    //    判据是「**这个 404 是不是 axum 的路由未命中**」：未命中的 404 是**空体**，
    //    业务层的 404 是 JSON {"error": ...}。⇒ 断言响应体含 "error"（D-009 判据形状原则）。
    //    （本轮新坑：本测试改用 multi_thread flavor 后 block_in_place 才真正执行 ⇒
    //      单线程时它 panic 成 500，把业务 404 掩盖了 ⇒ 换成真 404 后老断言才暴露。）
    {
        let req = Request::builder()
            .uri("/api/v1/persons/no-such-person/timeline")
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND, "unknown pid must be a business 404");
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
        let body = String::from_utf8_lossy(&bytes).to_string();
        assert!(
            body.contains("\"error\""),
            "route missing for GET timeline (axum 404 with empty body = not mounted)"
        );
    }

    // 5) Web 管理端最小版（D-016）：GET / 必须回 HTML（内容非空 + content-type text/html）。
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
