// T40：写侧「**多轮**」——同一会话连续多次建档/上传（PLAN/T35 起挂着、到 T40 才关掉的第三维）。
//
// 为什么要有这道门：
//   T38 关掉了「**并发**」（person registry 并发写会把真相层写坏），
//   T39 关掉了「**大载荷**」（REST/MCP 两面上限不一致、MCP 宣称的 32MB 不可达）。
//   ⇒ 剩下「**多轮**」这一维，此前**从未被门覆盖**：
//   写侧在真进程里只被调用过「1 次建档 + 1 次上传」⇒
//   「连续 N 次写入后派生读面是否还准」「重复上传是否幂等」「重建后是否还原」
//   「MCP 面的多轮写与 REST 面是否同源」全都**没有门**。
//   ⇒ 本门把这四件事钉住。
//
// 判据（各对应一种「看起来多轮也支持、其实没支持」的形态）：
//   ① **多轮累积**：同一人连传 N 份**内容不同**的原件 ⇒ timeline/documents 条数 == N、
//      labs 点数 == N（派生读面没把前面的吞掉）；
//   ② **重复上传幂等**：同一份字节再传一次 ⇒ `deduped=true` 且**条数不变**
//      （不是「又建了一条」也不是「静默丢一份」）；
//   ③ **重建后还原**：多轮写入后重建派生索引 ⇒ 读面计数**逐字相等**（真相层是真��相）；
//   ④ **MCP 面同源**：MCP 连写 N 次 ⇒ REST 读面**立刻**看得见同样 N 条
//      （两个协议面写的是同一份真相层，不是两份）。
//   ⑤ **MCP 面并发写**（T38 只钉了 REST 面）：MCP 并发建档 ⇒ registry 仍逐行完整
//      （MCP 走的是**同一个** `store.create()` ⇒ 修复应当自动覆盖；没有门就等于没覆盖）。
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use std::path::PathBuf;
use tower::ServiceExt;

/// 每个测试独立 EHR_HOME：cargo test 默认多线程并行，共用目录会互删（D4：先怀疑测量方法）。
fn tmp_home(tag: &str) -> PathBuf {
    let h = std::env::temp_dir().join(format!("ehr-multiround-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&h);
    std::fs::create_dir_all(h.join("registry")).unwrap();
    std::fs::create_dir_all(h.join("vaults")).unwrap();
    h
}

fn registry_file(home: &std::path::Path) -> PathBuf {
    home.join("registry").join("persons.jsonl")
}

/// 读真相层**原始字节**（不做任何清理）⇒ 判据直接看落盘的东西，而不是看 API 的说法。
fn raw_registry(home: &std::path::Path) -> String {
    std::fs::read_to_string(registry_file(home)).unwrap_or_default()
}

/// 真相层每行都必须是完整 JSON（与 T38 同一判据，同一份实现思路）。
fn assert_registry_intact(home: &std::path::Path) -> usize {
    let raw = raw_registry(home);
    let lines: Vec<&str> = raw.lines().filter(|l| !l.trim().is_empty()).collect();
    let broken: Vec<String> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| serde_json::from_str::<Value>(l).is_err())
        .map(|(i, l)| format!("line {}: {}", i + 1, l))
        .collect();
    assert!(
        broken.is_empty(),
        "registry must contain only intact JSON lines; {} of {} lines are interleaved/corrupt:\n  {}",
        broken.len(),
        lines.len(),
        broken.join("\n  ")
    );
    lines.len()
}

async fn json_body(res: axum::response::Response) -> Value {
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&bytes).expect("json body")
}

/// 建一个人（走 REST 写侧 ⇒ 与真实客户端同一条路径）。
async fn create_person(app: &axum::Router, name: &str) -> String {
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/persons")
                .header("content-type", "application/json")
                .body(Body::from(json!({"name": name}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED, "create_person must be 201");
    let v = json_body(res).await;
    v["id"].as_str().expect("person id").to_string()
}

/// 上传一份原件（REST multipart 写侧）。
async fn upload(app: &axum::Router, pid: &str, filename: &str, content: &str) -> Value {
    let boundary = "MR-BOUND";
    let body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\nContent-Type: text/plain\r\n\r\n{content}\r\n--{boundary}--\r\n"
    );
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/persons/{pid}/documents"))
                .header("content-type", format!("multipart/form-data; boundary={boundary}"))
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK, "upload must be 200");
    json_body(res).await
}

async fn get(app: &axum::Router, uri: &str) -> Value {
    let res = app
        .clone()
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK, "GET {uri} must be 200");
    json_body(res).await
}

/// MCP 工具调用（JSON-RPC over POST /mcp）。
async fn call_tool(app: &axum::Router, name: &str, args: Value) -> Value {
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
                           "params":{"name": name, "arguments": args}}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let v = json_body(res).await;
    assert!(v.get("error").is_none(), "unexpected jsonrpc error: {v}");
    let r = &v["result"];
    assert_eq!(r["isError"], false, "tool {name} failed: {v}");
    r["structuredContent"].clone()
}

/// base64 编码（写侧 MCP 传二进制原件用；零新依赖）。
fn b64(bytes: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

/// 判据①：**多轮累积**——同一人连传 N 份内容不同的原件 ⇒ 派生读面条数 == N。
///
/// 为什么这条不能省：「只调 1 次」的写侧在多轮下可能悄悄丢前面的（CAS 按内容去重 +
/// 事件日志追加 + 派生索引三处都可能吞）⇒ 单轮测**永远发现不了**。
#[tokio::test(flavor = "multi_thread")]
async fn multi_round_uploads_accumulate_in_read_surfaces() {
    const N: usize = 20;
    let home = tmp_home("accumulate");
    let app = ehr_api::router(ehr_api::AppState::from_home(&home));
    let pid = create_person(&app, "多轮累积").await;

    let mut hashes = std::collections::HashSet::new();
    for i in 0..N {
        let content = format!("姓名：多轮\n检查项目：血常规\n白细胞计数 {}.0 10^9/L\n", i + 1);
        let up = upload(&app, &pid, &format!("lab{}.txt", i + 1), &content).await;
        assert_eq!(up["text_indexed"], true, "每份都应有文本层（否则读侧隐形）: {up}");
        let h = up["content_hash"].as_str().unwrap().to_string();
        assert_eq!(h.len(), 64, "回执必须带真 CAS 哈希");
        assert!(hashes.insert(h.clone()), "内容各不相同 ⇒ 哈希不应重复: {h}");
    }

    let tl = get(&app, &format!("/api/v1/persons/{pid}/timeline")).await;
    assert_eq!(tl["count"], N, "多轮后 timeline 必须有 {N} 条: {tl}");
    let docs = get(&app, &format!("/api/v1/persons/{pid}/documents")).await;
    assert_eq!(docs["count"], N, "多轮后 documents 必须有 {N} 条: {docs}");
    let labs = get(&app, &format!("/api/v1/persons/{pid}/labs")).await;
    assert_eq!(labs["documents_scanned"], N, "labs 必须扫到 {N} 份: {labs}");
    assert_eq!(labs["total_points"], N, "labs 必须有 {N} 个点: {labs}");
    // 哈希集合 == 回执集合（读面没换文档）
    let listed: std::collections::HashSet<&str> = docs["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["content_hash"].as_str().unwrap())
        .collect();
    assert_eq!(listed.len(), N, "documents 里哈希必须各不相同（没有重复行）");
    for h in &hashes {
        assert!(listed.contains(h.as_str()), "回执里的哈希 {h} 必须在读面里");
    }
}

/// 判据②：**重复上传幂等**——同一份字节传 N 次 ⇒ 恰好 **1** 条文档，
///   且**每次回执的 `document_id` / `text_indexed` 都与真相层一致**。
///
/// ⚠️ 预期口径说明：CAS **按内容**去重是既有契约（上游 `import()` 同款）⇒
///   同一份字节传 N 次，箱里应当**只有 1 条**文档，第 2 次起 `deduped=true`
///   ⇒ 「N 次传 ⇒ N 条」是**错的**预期（那是把「多轮」与「多份不同内容」混为一谈）。
///
/// 这条判据的**判别力**在回执上（T40 真缺陷）：
///   修前：第 2 次起回执 `document_id=null` / `text_indexed=false`
///         ⇒ 而真相层里那份**已建档、已索引** ⇒ 回执与真相层不一致 ⇒ 本条会红；
///   修后：回执回填内核既有 Document/OCR 事实 ⇒ 每一次都与读面一致 ⇒ 绿。
#[tokio::test(flavor = "multi_thread")]
async fn reuploading_same_bytes_is_idempotent_and_reply_matches_truth() {
    const N: usize = 5;
    let home = tmp_home("idempotent");
    let app = ehr_api::router(ehr_api::AppState::from_home(&home));
    let pid = create_person(&app, "多轮幂等").await;

    let content = "姓名：多轮\n检查项目：血常规\n白细胞计数 6.2 10^9/L\n";
    let mut first_doc_id: Option<i64> = None;
    for i in 0..N {
        let up = upload(&app, &pid, &format!("same{}.txt", i), content).await;
        if i == 0 {
            assert_eq!(up["deduped"], false, "首次上传不应判重: {up}");
            assert_eq!(up["text_indexed"], true, "首次上传应有文本层: {up}");
            first_doc_id = Some(up["document_id"].as_i64().expect("document_id"));
        } else {
            assert_eq!(up["deduped"], true, "第 {} 次重传同字节必须判重: {up}", i + 1);
            // ★ T40 真缺陷所在：回执必须与真相层一致（document_id 不得为 null、text_indexed 不得为 false）
            assert_eq!(
                up["document_id"].as_i64(),
                first_doc_id,
                "重传回执的 document_id 必须与首次一致（回执是「真相层现在是什么」的快照）: {up}"
            );
            assert_eq!(up["text_indexed"], true, "重传回执必须如实说「已索引」: {up}");
        }
    }
    // 真相层：恰好 1 条（同内容去重），且它的 document_id 就是回执里的那个
    let docs = get(&app, &format!("/api/v1/persons/{pid}/documents")).await;
    assert_eq!(docs["count"], 1, "同内容 N 次 ⇒ 箱里应恰好 1 条: {docs}");
    assert_eq!(
        docs["items"][0]["document_id"].as_i64(),
        first_doc_id,
        "读面的 document_id 必须与回执一致（同一事实两处必须相同）: {docs}"
    );
    assert_eq!(docs["items"][0]["text_indexed"], true, "读面必须说已索引: {docs}");
    let tl = get(&app, &format!("/api/v1/persons/{pid}/timeline")).await;
    assert_eq!(tl["count"], 1, "timeline 同样只有 1 条: {tl}");
}

/// 判据③：**重建后还原**——多轮写入后重建派生索引 ⇒ 读面计数逐字相等。
///
/// 为什么这条是「多轮」维度专属：单轮时派生层是「刚建的」，重建必然还原 ⇒ 测不出问题；
/// 多轮时派生层里可能有**中途**产生的状态（增量索引、FTS 热更），重建才暴露「重建不还原」。
#[tokio::test(flavor = "multi_thread")]
async fn multi_round_survives_derived_index_rebuild() {
    const N: usize = 12;
    let home = tmp_home("rebuild");
    let app = ehr_api::router(ehr_api::AppState::from_home(&home));
    let pid = create_person(&app, "多轮重建").await;
    for i in 0..N {
        let content = format!("姓名：多轮\n检查项目：血常规\n白细胞计数 {}.0 10^9/L\n", i + 1);
        upload(&app, &pid, &format!("r{}.txt", i + 1), &content).await;
    }
    let before_tl = get(&app, &format!("/api/v1/persons/{pid}/timeline")).await["count"].clone();
    let before_labs = get(&app, &format!("/api/v1/persons/{pid}/labs")).await["total_points"].clone();
    assert_eq!(before_tl, json!(N));

    let rep = ehr_api::rebuild::rebuild_person(&home, &pid).expect("rebuild must succeed");
    assert!(rep.rebuilt_from_log, "重建必须真的删掉派生库并从日志回放: {rep:?}");

    let after_tl = get(&app, &format!("/api/v1/persons/{pid}/timeline")).await["count"].clone();
    let after_labs = get(&app, &format!("/api/v1/persons/{pid}/labs")).await["total_points"].clone();
    assert_eq!(after_tl, before_tl, "重建后 timeline 必须逐字相等");
    assert_eq!(after_labs, before_labs, "重建后 labs 点数必须逐字相等");
}

/// 判据④：**MCP 面多轮写 ⇒ REST 面立刻可见**（两个协议面写的是同一份真相层）。
///
/// 这条是**跨面**判据：MCP 写 N 次后，REST 读面（timeline / documents）必须也是 N 条。
/// 若 MCP 写侧另存了一份（或写进别的目录），这条会红。
#[tokio::test(flavor = "multi_thread")]
async fn mcp_multi_round_writes_are_visible_over_rest() {
    const N: usize = 8;
    let home = tmp_home("mcp-round");
    let app = ehr_api::router(ehr_api::AppState::from_home(&home));

    let p = call_tool(&app, "create_person", json!({"name": "MCP多轮"})).await;
    let pid = p["id"].as_str().unwrap().to_string();

    for i in 0..N {
        let content = format!("姓名：mcp\n检查项目：血常规\n白细胞计数 {}.0 10^9/L\n", i + 1);
        let up = call_tool(
            &app,
            "upload_document",
            json!({
                "person_id": pid,
                "filename": format!("m{}.txt", i + 1),
                "content_base64": b64(content.as_bytes()),
            }),
        )
        .await;
        assert_eq!(up["text_indexed"], true, "MCP 写入也要有文本层: {up}");
        assert_eq!(up["deduped"], false, "第 {} 份不应判重", i + 1);
    }

    // REST 读面看到同样的 N 条 ⇒ 两面同源
    let tl = get(&app, &format!("/api/v1/persons/{pid}/timeline")).await;
    assert_eq!(tl["count"], N, "MCP 写 {N} 份 ⇒ REST timeline 必须也是 {N} 条: {tl}");
    let docs = get(&app, &format!("/api/v1/persons/{pid}/documents")).await;
    assert_eq!(docs["count"], N, "MCP 写 {N} 份 ⇒ REST documents 必须也是 {N} 条: {docs}");
    // MCP 自己的读面也一致
    let mdocs = call_tool(&app, "list_documents", json!({"person_id": pid})).await;
    assert_eq!(mdocs["count"], N, "MCP 读面必须也是 {N} 条: {mdocs}");
}

/// 判据⑤：**MCP 面并发建档** ⇒ registry 仍逐行完整、读面仍可用。
///
/// T38 修的是 `store.create()`，MCP 面走的是**同一个**函数 ⇒ 修复应当自动覆盖 MCP 面；
/// 但「应当覆盖」不是「已覆盖」——没有门就等于没有证据（PLAN/T38 未销账②原话）。
#[tokio::test(flavor = "multi_thread")]
async fn concurrent_mcp_creates_keep_registry_intact() {
    const N: usize = 32;
    let home = tmp_home("mcp-conc");
    let app = ehr_api::router(ehr_api::AppState::from_home(&home));

    let mut tasks = Vec::new();
    for i in 0..N {
        let app = app.clone();
        tasks.push(tokio::spawn(async move {
            call_tool(&app, "create_person", json!({"name": format!("MCP并发-{i}")})).await["id"]
                .as_str()
                .unwrap()
                .to_string()
        }));
    }
    let mut ids = std::collections::HashSet::new();
    for t in tasks {
        ids.insert(t.await.unwrap());
    }
    assert_eq!(ids.len(), N, "并发建档返回的 id 必须互不相同");

    let lines = assert_registry_intact(&home);
    assert_eq!(lines, N, "registry 必须恰好 {N} 行（无半条、无丢条）");
    let listed = get(&app, "/api/v1/persons").await;
    assert_eq!(listed.as_array().unwrap().len(), N, "读面必须仍可用且条数正确");
}
