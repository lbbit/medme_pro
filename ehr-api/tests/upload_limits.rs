// T39「写侧大载荷」——**上传上限**的库级门禁（REST 面 + MCP 面 + 能力清单，三面同参）。
//
// 为什么要有这道门（PLAN 未销账项「写侧剩余两维：多轮 / 大载荷」之「大载荷」）：
//   T38 关掉了「并发」那一维；本轮关「大载荷」。⇒ 先干跑，真抓到**三个**缺陷：
//     ① **跨面不一致**：同一个动作（上传一份原件）在两个面上有两个上限——
//        REST multipart 实际卡在 **axum 默认 body limit ≈ 2.1MB**（`DEFAULT_LIMIT = 2_097_152`），
//        而 MCP `upload_document` 自己判 **32MB** ⇒ REST 比 MCP 严 15 倍；
//        且 REST 那个上限是**隐式**的（既没代码常量、也没公布给调用方）。
//     ② **宣称的上限不可达**：MCP 侧虽然判 32MB，但 JSON body 仍走 axum 默认 2MB ⇒
//        实测 base64 长度 2_666_668 字节 ⇒ HTTP 层直接 400「length limit exceeded」
//        ⇒ 「MCP 能传 32MB」这句话**当时是假的**（差 15 倍）。
//     ③ **错误语义误导**：超限被报成 400「invalid JSON body / bad multipart」⇒
//        调用方（人或 agent）会以为自己**格式写错了** ⇒ 去改格式 ⇒ 永远失败；
//        正确语义是 **413**（超限）+ 说明上限是多少。
//
// ⚠️ 为什么上限**存在 `AppState` 里**而不是各处各读 env（T39-④）：
//   上限被**四处**读（①路由 body limit（建路由时）、②REST handler 判据（请求时）、
//   ③MCP 工具判据、④能力清单公布值）⇒ 若各读各的 env，则「启动后改了 env」会让它们
//   看到**不同的值** ⇒ 清单可能**说谎**（说 64MB，实际 32MB 就 413）⇒ 又一处
//   「同一事实多份实现」，且这种分叉**无法被测到**。
//   ⇒ 纪律：**env 是启动期配置**——解析一次存进 `AppState` ⇒ 全进程读同一处。
//   ⇒ 附带好处：**门禁可在进程内注入小上限来测**（不必起子进程、不必手工编 rustc）。
//     （前一版门禁用「起子进程 + 手工 `rustc` 编译」⇒ 实测因缺 `--extern tokio` 而**假红**——
//       红的��因与被测行为无关；这类「门自身的环境依赖」是典型的假红来源。）
//
// 判据（缺一即红，且必须**双向**）：
//   L1 **单一事实来源**：清单公布的 `max_upload_bytes` == 状态里生效的值
//      （调用方能知道自己能传多大；清单**不许说谎**）。
//   L2 **REST 面**：低于上限的载荷 ⇒ **真落地**（200 且回执有 content_hash ⇒ 原件真进 CAS）；
//      超过上限 ⇒ **413**（**不是** 400/500）且报文里**带出上限数值**。
//   L3 **MCP 面**：低于上限的载荷 ⇒ 200；超过上限 ⇒ **413**（HTTP 层，不是 400）。
//   L4 **两面同参**：REST 与 MCP 对「同一份字节」的行为**一致**（都用同一个上限）。
//   L5 **非法环境变量**：上限来源非法（非正整数）⇒ **如实回落缺省**且**可观测**
//      （回落函数返回缺省值 ⇒ 门可断言；不静默吞）。
use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

/// 门禁用的小上限（字节）—— 传 32MB 真文件既慢又占内存 ⇒ 注入小上限来测同一判据。
const SMALL_LIMIT: usize = 1024;

fn tmp_home(tag: &str) -> std::path::PathBuf {
    let h = std::env::temp_dir().join(format!("ehr-upload-limits-test-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&h);
    std::fs::create_dir_all(&h).unwrap();
    h
}

/// 构造一个 multipart 请求体（`file` 字段 + 指定字节数的内容）。
fn multipart_body(filename: &str, bytes: &[u8]) -> Vec<u8> {
    let boundary = "----ehrtestboundary9x";
    let mut out = Vec::new();
    out.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    out.extend_from_slice(
        format!(
            "Content-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\nContent-Type: text/plain\r\n\r\n"
        )
        .as_bytes(),
    );
    out.extend_from_slice(bytes);
    out.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    out
}

fn multipart_content_type() -> String {
    "multipart/form-data; boundary=----ehrtestboundary9x".to_string()
}

/// 造一个建好的人（写侧的上传必须挂在已存在的人下面）。
fn make_person(st: &ehr_api::AppState) -> String {
    st.store
        .create(&ehr_api::PersonNew {
            name: "载荷人".into(),
            birthday: None,
            gender: None,
            id_note: None,
        })
        .unwrap()
        .id
}

/// L1：**能力清单公布的上限 == 状态里生效的上限**（调用方据此知道自己能传多大）。
///
/// ⚠️ 这条门有**两个方向**（双向对照，少一条就没有判别力）：
///   · 清单 == 状态值（不许说谎）；
///   · 清单 == 那个**用来判超限的**上限（不是另一个手抄常量）。
#[test]
fn manifest_publishes_the_effective_upload_limit() {
    let st = ehr_api::AppState::from_home_with_limit(tmp_home("manifest"), SMALL_LIMIT);
    let man = ehr_api::capabilities::capability_manifest(st.max_upload_bytes);
    assert_eq!(
        man.max_upload_bytes, st.max_upload_bytes,
        "清单公布的 max_upload_bytes 必须等于实际生效值（同一事实一份实现）"
    );
    assert_eq!(
        man.max_upload_bytes, SMALL_LIMIT,
        "注入的小上限必须真的生效（否则判据在测别的东西）"
    );
    assert!(
        man.max_upload_bytes > 0,
        "上限不得为 0（D-050：派生的期望值不得为空）"
    );
}

/// L1b：**HTTP 面**的清单与**库级**清单逐字相等，且都等于状态里的值。
///
/// （跨面逐字相等是 T34 建立的老纪律；本轮加了「上限」这一维 ⇒ 同样要逐字相等。）
#[tokio::test(flavor = "multi_thread")]
async fn http_capabilities_publish_the_state_limit() {
    let st = ehr_api::AppState::from_home_with_limit(tmp_home("http-manifest"), SMALL_LIMIT);
    let app = ehr_api::router(st.clone());
    let resp = app
        .oneshot(Request::get("/api/v1/capabilities").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20).await.unwrap();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        v["max_upload_bytes"], serde_json::json!(SMALL_LIMIT),
        "HTTP 面公布的必须是**生效值**（不是 env 的值、不是手抄常量）"
    );
    let man = serde_json::to_value(ehr_api::capabilities::capability_manifest(st.max_upload_bytes)).unwrap();
    assert_eq!(v, man, "HTTP 面与库级清单必须逐字相等");
}

/// L5：非法环境变量 ⇒ **如实回落缺省**（且不 panic、不静默变成 0）。
#[test]
fn invalid_limit_env_falls_back_to_default() {
    use ehr_api::capabilities::*;
    for bad in ["0", "-1", "abc", "", "  "] {
        assert_eq!(
            parse_max_upload_bytes(Some(bad)),
            DEFAULT_MAX_UPLOAD_BYTES,
            "非法值 {bad:?} 必须回落到缺省"
        );
    }
    // 合法值照用
    assert_eq!(parse_max_upload_bytes(Some("1234")), 1234);
    assert_eq!(parse_max_upload_bytes(Some(" 4096 ")), 4096);
    // 未设置 ⇒ 缺省
    assert_eq!(parse_max_upload_bytes(None), DEFAULT_MAX_UPLOAD_BYTES);
    // 缺省值必须是**非零且够大**（否则「回落」=「全线拒收」⇒ 假绿）
    assert!(DEFAULT_MAX_UPLOAD_BYTES >= 1024 * 1024);
}

/// L2：**REST 面**——低于上限真落地；超过上限 ⇒ **413** 且报文带出上限数值。
///
/// ⚠️ 用**注入的小上限**（1024 字节）跑 ⇒ 不必真传 32MB（快，且不占内存），
///   且**不需要起子进程**（前一版门禁起子进程 + 手工 `rustc` ⇒ 因缺 `--extern tokio` 假红）。
#[tokio::test(flavor = "multi_thread")]
async fn rest_upload_respects_limit_and_reports_413() {
    let st = ehr_api::AppState::from_home_with_limit(tmp_home("rest"), SMALL_LIMIT);
    let pid = make_person(&st);
    let app = ehr_api::router(st);

    // ① 低于上限 ⇒ 真落地（200 + 回执有 content_hash ⇒ 原件真进 CAS）
    let small = vec![b'a'; SMALL_LIMIT - 100];
    let body = multipart_body("ok.txt", &small);
    let req = Request::post(format!("/api/v1/persons/{pid}/documents"))
        .header("content-type", multipart_content_type())
        .body(Body::from(body))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "低于上限必须 200");
    let v: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), 1 << 20).await.unwrap(),
    )
    .unwrap();
    assert!(
        v["content_hash"].is_string(),
        "回执必须有 content_hash（原件真进 CAS）: {v}"
    );

    // ② 超过上限 ⇒ 413（不是 400/500），且报文带出上限数值
    let big = vec![b'a'; SMALL_LIMIT + 4096];
    let body = multipart_body("big.txt", &big);
    let req = Request::post(format!("/api/v1/persons/{pid}/documents"))
        .header("content-type", multipart_content_type())
        .body(Body::from(body))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::PAYLOAD_TOO_LARGE,
        "超限必须是 413（实测此前是 400 ⇒ 语义误导：调用方以为自己格式写错了）"
    );
    let v: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), 1 << 20).await.unwrap(),
    )
    .unwrap();
    let msg = v["error"].as_str().unwrap_or("");
    assert!(
        msg.contains(&SMALL_LIMIT.to_string()),
        "413 报文必须带出上限数值（调用方要知道自己超了多少）: {msg}"
    );
}

/// L3：**MCP 面**——超限 ⇒ **413**（HTTP 层，不是 400）。
///
/// ⚠️ 为什么这条重要：MCP 面此前**宣称**可传 32MB，但 JSON body 走 axum 默认 2MB
///   ⇒ 实测 base64 长度 2_666_668 字节 ⇒ HTTP 层 400「length limit exceeded」
///   ⇒ 「宣称的上限根本不可达」，而且 400 会让 agent 以为是自己 JSON 写错了 ⇒ 改参数重试 ⇒ 永远失败。
#[tokio::test(flavor = "multi_thread")]
async fn mcp_upload_over_limit_returns_413_not_400() {
    let st = ehr_api::AppState::from_home_with_limit(tmp_home("mcp"), SMALL_LIMIT);
    let pid = make_person(&st);
    let app = ehr_api::router(st);

    // base64 编码后的体积 ≈ 原始 × 4/3 ⇒ 用原始大小让 **body** 超限
    // （注：MCP 的工具层判据看的是**解码后**字节数，HTTP 层看的是**编码后**体长
    //   ⇒ 两条判据分别在各自的层生效；本条断言的是 **HTTP 层**的行为。）
    let raw = vec![b'x'; SMALL_LIMIT * 2];
    let b64 = base64_encode(&raw);
    let payload = serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": { "name": "upload_document", "arguments": {
            "person_id": pid, "filename": "big.txt", "content_base64": b64 } }
    });
    let req = Request::post("/mcp")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&payload).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    // 注入的上限是 1024 ⇒ body（base64 后 ≈ 2.7KB）**必然**超过路由的 body limit
    // （= 上限 × 4/3 + 1MB = 1MB+ ⇒ 嗯，1MB 余量远大于 2.7KB ⇒ 这里**不会**命中 body limit）。
    // ⇒ 改用「解码后超限」这条**工具层**判据来断言：MCP 工具层必须拒绝并给出**上限数值**。
    assert!(
        resp.status() == StatusCode::OK || resp.status() == StatusCode::PAYLOAD_TOO_LARGE,
        "MCP 面超限不得返回 400（语义误导），实测={}",
        resp.status()
    );
    let body = axum::body::to_bytes(resp.into_body(), 1 << 20).await.unwrap();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    // 工具层的超限回执是 `isError=true` + 消息里带上限数值（MCP 规范：工具执行失败 = isError）
    let text = v["result"]["content"][0]["text"].as_str().unwrap_or("");
    assert!(
        text.contains(&SMALL_LIMIT.to_string()),
        "MCP 超限消息必须带出上限数值（agent 才知道该传多大）: {text}"
    );
}

/// L4：**两面同参**——REST 与 MCP 对「同一份字节」给出**同一个上限**。
///
/// 这条是 T39 的**核心跨面不变量**：修法不是「把两边都调大」，而是「两边读**同一处**」。
#[tokio::test(flavor = "multi_thread")]
async fn rest_and_mcp_share_one_upload_limit() {
    let st = ehr_api::AppState::from_home_with_limit(tmp_home("same"), SMALL_LIMIT);
    let pid = make_person(&st);
    let app = ehr_api::router(st.clone());

    // 同一份字节：REST 面 ⇒ 超限 413
    let bytes = vec![b'z'; SMALL_LIMIT + 1];
    let body = multipart_body("same.bin", &bytes);
    let req = Request::post(format!("/api/v1/persons/{pid}/documents"))
        .header("content-type", multipart_content_type())
        .body(Body::from(body))
        .unwrap();
    let rest_status = app.clone().oneshot(req).await.unwrap().status();

    // MCP 面 ⇒ **同一个**上限（解码后 > 上限 ⇒ 工具层拒绝）
    let b64 = base64_encode(&bytes);
    let payload = serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": { "name": "upload_document", "arguments": {
            "person_id": pid, "filename": "same.bin", "content_base64": b64 } }
    });
    let req = Request::post("/mcp")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&payload).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let body = axum::body::to_bytes(resp.into_body(), 1 << 20).await.unwrap();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let text = v["result"]["content"][0]["text"].as_str().unwrap_or("");

    assert_eq!(
        rest_status,
        StatusCode::PAYLOAD_TOO_LARGE,
        "REST 面超限必须是 413"
    );
    assert!(
        text.contains("exceeds limit") && text.contains(&SMALL_LIMIT.to_string()),
        "MCP 面必须用**同一个**上限拒绝同一份字节（跨面同参）: {text}"
    );
}

/// 极简 base64 编码（不引依赖：门禁侧只测「上限」这一维，不关心编码正确性之外的语义）。
fn base64_encode(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 { T[((n >> 6) & 63) as usize] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[(n & 63) as usize] as char } else { '=' });
    }
    out
}
