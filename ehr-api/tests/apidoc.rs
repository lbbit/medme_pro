// D-025 / 目标验收⑤：人类可读接口文档（docs/API.md）的**漂移保险**。
//
// 三条红线，各对应一种「文档在骗人」的形态：
//  ① CONTRACT 表宣称「未实装(501)」的端点，**必须真的返回 501** —— 否则 OpenAPI 与现实脱节
//     （本轮就抓到 labs：实装 200，却仍挂在 CONTRACT 里标成 stub→501）。
//  ② OpenAPI 里的每个 (method,path) 都**必须出现在生成文档里** —— 否则文档漏端点。
//  ③ 生成文档里的每个端点**必须真的存在于 OpenAPI** —— 否则文档出现幽灵端点。
//  ④ 文档必须**显式标注** stub 端点（D5：状态码不许粉饰）。
use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

#[tokio::test(flavor = "multi_thread")]
async fn contract_entries_really_return_501() {
    let tmp = std::env::temp_dir().join(format!("ehr-api-contract501-{}", std::process::id()));
    std::fs::create_dir_all(tmp.join("vaults")).unwrap();
    let st = ehr_api::AppState::from_home(&tmp);
    let app = ehr_api::router(st);

    for (method, path, _tag, _summary) in ehr_api::CONTRACT {
        let concrete = path.replace("{pid}", "p1").replace("{sha}", "aa");
        let req = Request::builder()
            .method(method.to_uppercase().as_str()) // ⚠️ HTTP 方法大小写敏感：小写 "get" ⇒ 405 而非命中路由
            .uri(concrete.clone())
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(
            res.status(),
            StatusCode::NOT_IMPLEMENTED,
            "CONTRACT 表里的 {method} {concrete} 宣称 stub(501)，实际返回 {:?} —— \
             端点已实装就必须从 CONTRACT 表移除，否则 OpenAPI/文档会把它说成 501",
            res.status()
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn generated_doc_covers_every_openapi_operation_and_vice_versa() {
    let doc = ehr_api::openapi_json();
    let md = ehr_api::apidoc::render(&doc);
    let paths = doc["paths"].as_object().expect("openapi paths");

    let mut n_ops = 0;
    for (p, item) in paths {
        let map = item.as_object().unwrap();
        for (m, op) in map {
            if !op.is_object() {
                continue;
            }
            n_ops += 1;
            let head = format!("### `{} {}`", m.to_uppercase(), p);
            assert!(md.contains(&head), "生成文档缺少端点小节：{head}");
            // 路径参数占位名必须出现在文档里（读者要知道该填什么）
            for pr in op["parameters"].as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
                if pr["in"].as_str() == Some("path") {
                    let n = pr["name"].as_str().unwrap();
                    assert!(md.contains(n), "生成文档缺少路径参数 {n}（端点 {head}）");
                }
            }
        }
    }
    // D-021：端点数**不许写死快照**（此前是 `>= 16`）⇒ 判据改为「两份独立产物**逐条相等**」。
    // 为什么：`>= 16` 是**快照不是判据**——加一条路由它不会红（少一条端点也不会红），
    // 真正能抓漂移的是「文档里的小节数 == OpenAPI 里的 operation 数」。
    //
    // ⚠️ 关键：**不能**写成 `n_ops == rest_manifest()["operations"]`——那是**同义反复**
    //   （两者都从 openapi_json() 派生 ⇒ 永远相等 ⇒ 永远绿；这正是 D-037「绿但测错对象」
    //   的最隐蔽形态：一个看起来很严谨、实则测不出任何东西的断言）。
    //   真判据必须**跨产物**：**渲染出来的 Markdown 小节**（apidoc::render 的产物）
    //   vs **OpenAPI 原文**（两份独立产物）⇒ 一边漏小节/多小节/重复小节都会红。
    // ⚠️ 探针自身的坑（本轮实撞）：生成文档里 `### ` 小节有**两类**——端点小节
    //   （`### \`GET /api/v1/audit\``）与**schema 小节**（`### \`AuditReply\``）。
    //   只按 `### ` 前缀数 ⇒ 50（19 端点 + 31 schema）⇒ 断言立刻红。
    //   ⇒ 判别式 = 「小节内容是 `METHOD path` 形状且 METHOD 是 HTTP 动词」。
    //   （D-011「红门先查门禁自身」的同族：门禁探针出错时，报错面貌会伪装成产品 bug。）
    const HTTP_METHODS: [&str; 7] = ["GET", "POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS"];
    let md_sections = md
        .lines()
        .filter_map(|l| l.strip_prefix("### `"))
        .filter_map(|rest| {
            let (m, p) = rest.split_once(' ')?;
            let m = m.to_uppercase();
            if !HTTP_METHODS.contains(&m.as_str()) {
                return None; // schema 小节（无空格或非 HTTP 动词）
            }
            Some(format!("{} {}", m, p.trim_end_matches('`')))
        })
        .collect::<Vec<_>>();
    assert_eq!(
        md_sections.len(),
        n_ops,
        "生成文档的端点小节数必须等于 OpenAPI 的 operation 数（跨产物判据，不是同源比较）"
    );
    // 逐条相等（不只是个数）：文档小节集合 == OpenAPI operation 集合
    let mut openapi_ops: Vec<String> = Vec::new();
    for (p, item) in paths {
        for (m, op) in item.as_object().unwrap() {
            if op.is_object() {
                openapi_ops.push(format!("{} {}", m.to_uppercase(), p));
            }
        }
    }
    openapi_ops.sort();
    let mut md_sorted = md_sections.clone();
    md_sorted.sort();
    assert_eq!(
        md_sorted, openapi_ops,
        "生成文档的端点小节集合必须逐条等于 OpenAPI 的 operation 集合"
    );

    // 反向：文档小节里的每个端点都必须真实存在于 OpenAPI（无幽灵端点）
    for line in md.lines() {
        if let Some(rest) = line.strip_prefix("### `") {
            let (method, path) = match rest.split_once(' ') {
                Some((m, p)) => (m, p.trim_end_matches('`')),
                None => continue,
            };
            let has = paths
                .get(path)
                .and_then(|it| it.get(method.to_lowercase().as_str()))
                .is_some();
            assert!(has, "生成文档出现幽灵端点：{method} {path}（OpenAPI 里没有）");
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn doc_marks_stub_endpoints_and_states_auth_rules() {
    let doc = ehr_api::openapi_json();
    let md = ehr_api::apidoc::render(&doc);
    // stub 端点必须逐个被标注
    for (method, path, _t, _s) in ehr_api::CONTRACT {
        let head = format!("### `{} {}`", method.to_uppercase(), path);
        let idx = md.find(&head).expect("contract 端点必须在文档里有小节");
        // 按字符边界截断（中文文档，不能按字节切——否则 panic: not a char boundary）
        let seg: String = md[idx..].chars().take(800).collect();
        assert!(seg.contains("501"), "端点 {head} 未在文档里标注 501");
        assert!(seg.contains("stub"), "端点 {head} 未在文档里标注 stub");
    }
    // 认证与错误码必须写明（读者要靠它调对接口）
    assert!(md.contains("Authorization: Bearer"), "文档必须写明鉴权头");
    assert!(md.contains("401") && md.contains("403"), "文档必须写明 401/403 语义");
    assert!(md.contains("503"), "文档必须写明重建期 503");
    assert!(md.contains("自动生成"), "文档必须声明自己是生成的（防手改）");
}
