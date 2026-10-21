// D-023：写侧 `PUT /persons/{pid}/documents/{sha}/extraction`（LLM 结构化抽取回写）的门禁。
//
// 五条红线，各对应一种「看起来能用其实不能」的形态：
//  ① **落库的是「校验后」的 extraction，不是原样交来的 JSON** —— 这是本轮修掉的**真缺陷**：
//     上一版把原样 result_json 存进 CAS，verify 的产物只用来算一个布尔就丢了
//     ⇒ **模型编造的字段照样进了真相层**（labs 聚合会把它当结构化 point 画进趋势图）。
//     判据：提交一条**编造**的化验项 ⇒ 它**不得**出现在落库的 extraction_json 里。
//  ② **evidence 闸真在工作**：原文里没有的字符串字段 ⇒ 判假 ⇒ 文本档被上游清空。
//  ③ **没有文本层的文档必须被拒**（不能「空对空恒过」——那等于不设防）。
//  ④ **mode 不猜**：只认 text|image，拼错 ⇒ 400（不静默换口径）。
//  ⑤ **端点可验**：真 handler 返回 200/400/404；且它**不在** CONTRACT 表里
//     （否则 OpenAPI 仍宣称 501 —— D-025 的反向保险）。
use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

fn tmp_home(tag: &str) -> std::path::PathBuf {
    let h = std::env::temp_dir().join(format!("ehr-extraction-test-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&h);
    std::fs::create_dir_all(&h).unwrap();
    h
}

fn fixture(tag: &str) -> (std::path::PathBuf, String) {
    let home = tmp_home(tag);
    let store = ehr_api::PersonStore::new(home.clone());
    let p = store
        .create(&ehr_api::PersonNew {
            name: "抽取门禁·甲".into(),
            birthday: None,
            gender: Some("F".into()),
            id_note: None,
        })
        .unwrap();
    (home, p.id)
}

/// 造一份**有文本层**的文档（txt 走 parser::extract 路线 ⇒ 有 OCR 文本层）。
fn upload_txt(home: &std::path::Path, pid: &str) -> String {
    let vault = ehr_api::vaults::open_person_vault(home, pid).unwrap();
    let up = ehr_api::vaults::ingest_upload(
        &vault,
        "血常规.txt",
        "XX医院检验科检验报告单\n姓名:抽取门禁·甲\n白细胞计数 6.2 10^9/L 3.5-9.5\n血红蛋白 132 g/L 115-150\n"
            .as_bytes(),
    )
    .unwrap();
    up.content_hash
}

fn new_body(result_json: &str, mode: &str) -> String {
    serde_json::json!({
        "result_json": result_json,
        "backend": "test-gateway",
        "model_version": "test-model-1",
        "mode": mode,
    })
    .to_string()
}

/// ① **编造的字段不得进真相层**（本轮修掉的真缺陷的直接判据）。
///
/// 做法：提交一条**原文里根本不存在**的化验项（血糖 99.9 mmol/L，文中无此串），
/// 文本档 ⇒ 上游 verify 判假 ⇒ 该条被**清空**。⇒ 落库的 extraction_json 里
/// **不得**出现 "血糖" 这两个字。
/// 上一版实装会在这里红（它存的是原样 JSON ⇒ "血糖" 原样落库 ⇒ 真相层被污染）。
#[test]
fn fabricated_field_never_reaches_the_truth_layer() {
    let (home, pid) = fixture("forged");
    let sha = upload_txt(&home, &pid);
    let vault = ehr_api::vaults::open_person_vault(&home, &pid).unwrap();

    let payload = serde_json::json!({
        "labs": [
            // 真项：原文逐字有
            { "name": "白细胞计数", "value": "6.2", "unit": "10^9/L", "ref_low": "3.5", "ref_high": "9.5" },
            // **伪造项**：原文里没有「血糖」也没有 99.9
            { "name": "血糖", "value": "99.9", "unit": "mmol/L", "ref_low": "3.9", "ref_high": "6.1" }
        ]
    })
    .to_string();

    let reply = ehr_api::vaults::add_extraction(
        &vault,
        &sha,
        ehr_api::vaults::ExtractionNew {
            result_json: payload,
            backend: "test-gateway".into(),
            model_version: "test-model-1".into(),
            mode: "text".into(),
            schema: 1,
        },
    )
    .unwrap();

    // 伪造项被清掉 ⇒ 落库 JSON 里没有「血糖」
    let stored = vault
        .extraction_json(reply.document_id)
        .unwrap()
        .expect("extraction must be stored");
    assert!(
        !stored.contains("血糖"),
        "伪造字段进了真相层（落库 JSON 原文含「血糖」）: {stored}"
    );
    // 真项还在（闸不是「一棍子打死」，是逐字段判）
    assert!(
        stored.contains("白细胞计数"),
        "真实字段被误清（闸过严）: {stored}"
    );
    // 回执如实报告「没全过闸」
    assert!(!reply.verified, "含伪造项 ⇒ verified 必须为 false");
    assert_eq!(reply.labs_count, 1, "只应剩 1 条真实化验项");
}

/// ①b **回执里的 result_hash 必须等于「落库内容」的哈希**（不是提交内容的哈希）。
/// 判据形状：回执的 result_hash 与 CAS 里那份 JSON 的 sha256 相等 ⇒ 消费方拿 result_hash
/// 去核，核到的是**真相层里那一份**。
#[test]
fn result_hash_points_at_the_stored_json_not_the_submitted_one() {
    let (home, pid) = fixture("hash");
    let sha = upload_txt(&home, &pid);
    let vault = ehr_api::vaults::open_person_vault(&home, &pid).unwrap();

    let payload = serde_json::json!({
        "labs": [ { "name": "白细胞计数", "value": "6.2", "unit": "10^9/L" } ]
    })
    .to_string();
    let reply = ehr_api::vaults::add_extraction(
        &vault,
        &sha,
        ehr_api::vaults::ExtractionNew {
            result_json: payload.clone(),
            backend: "test-gateway".into(),
            model_version: "test-model-1".into(),
            mode: "text".into(),
            schema: 1,
        },
    )
    .unwrap();

    let stored = vault.extraction_json(reply.document_id).unwrap().unwrap();
    let stored_hash = core_model::cas::sha256_hex(stored.as_bytes());
    assert_eq!(
        reply.result_hash, stored_hash,
        "result_hash 必须指向落库内容（真相层里那一份）"
    );
    // 提交内容与落库内容**不同**（本例因为上游规范化了 JSON 形状）⇒ 两个哈希本就该不同；
    // 若恰好相同（内容逐字相同），也不该断言不等 —— 上面那条等式才是判据。
}

/// ③ **没有文本层的文档必须被拒**（空对空恒过 = 不设防）。
///
/// 做法：直接用内核 API 造一份「**有 Document 行、但没有 OCR 文本层**」的文档
/// （import → add_document，不 add_ocr）—— 这正是「原件已入 CAS、但还没建文本层」的中间态
/// （worker 补 OCR 前）。此时 verify 无原文可比 ⇒ 必须拒，而不是「空对空恒过」。
#[test]
fn document_without_text_layer_is_rejected() {
    let (home, pid) = fixture("nolayer");
    let vault = ehr_api::vaults::open_person_vault(&home, &pid).unwrap();
    // 只 import（原件入 CAS）+ add_document（建 Document 行）——**故意不 add_ocr**。
    let imp = vault.import("待补OCR.pdf", "application/pdf", b"%PDF-1.4 not really a pdf").unwrap();
    let _doc = vault
        .add_document(core_model::NewDocument {
            source_file_id: imp.source_file.id,
            doc_type: core_model::DocType::Other,
            doc_date: None,
            doc_date_end: None,
            title: Some("待补OCR.pdf".into()),
            language: None,
            page_count: 1,
        })
        .unwrap();
    let sha = imp.source_file.content_hash.clone();
    assert!(
        vault.ocr_text(_doc.id).unwrap().trim().is_empty(),
        "这份文档应当**没有**文本层（构造前提）"
    );

    let err = ehr_api::vaults::add_extraction(
        &vault,
        &sha,
        ehr_api::vaults::ExtractionNew {
            result_json: serde_json::json!({ "labs": [] }).to_string(),
            backend: "test-gateway".into(),
            model_version: "test-model-1".into(),
            mode: "text".into(),
            schema: 1,
        },
    )
    .expect_err("无文本层必须被拒（不能空对空恒过）");
    assert!(
        err.to_string().contains("no text layer"),
        "错误信息必须说清是「没有文本层」: {err}"
    );
}

/// ④ **mode 不猜**：拼错的 mode ⇒ 400 语义的领域错误（不静默换口径）。
#[test]
fn unknown_mode_is_rejected_not_silently_coerced() {
    let (home, pid) = fixture("mode");
    let sha = upload_txt(&home, &pid);
    let vault = ehr_api::vaults::open_person_vault(&home, &pid).unwrap();
    let err = ehr_api::vaults::add_extraction(
        &vault,
        &sha,
        ehr_api::vaults::ExtractionNew {
            result_json: serde_json::json!({ "labs": [] }).to_string(),
            backend: "test-gateway".into(),
            model_version: "test-model-1".into(),
            mode: "imgae".into(), // 拼错
            schema: 1,
        },
    )
    .expect_err("未知 mode 必须被拒");
    assert!(
        err.to_string().contains("unsupported mode"),
        "错误信息必须点名 mode 不支持: {err}"
    );
}

/// ⑤ **HTTP 级**：200/400/404 三种语义各真打一次；且端点**不在** CONTRACT 表里。
#[tokio::test(flavor = "multi_thread")]
async fn http_extraction_endpoint_semantics_and_contract_table() {
    let (home, pid) = fixture("http");
    let sha = upload_txt(&home, &pid);
    let app = ehr_api::router(ehr_api::AppState::from_home(home.clone()));

    // 200：合法提交
    let body = new_body(
        &serde_json::json!({
            "labs": [ { "name": "白细胞计数", "value": "6.2", "unit": "10^9/L", "ref_low": "3.5", "ref_high": "9.5" } ]
        })
        .to_string(),
        "text",
    );
    let req = Request::put(format!("/api/v1/persons/{pid}/documents/{sha}/extraction"))
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK, "合法提交应 200");
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 20).await.unwrap();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    for f in ["content_hash", "document_id", "result_hash", "schema", "verified", "labs_count"] {
        assert!(v.get(f).is_some(), "回执缺字段 {f}: {v}");
    }
    assert_eq!(v["content_hash"], serde_json::json!(sha), "content_hash 必须回指原件 sha256");
    assert_eq!(v["verified"], serde_json::json!(true), "全字段逐字命中 ⇒ verified=true");

    // 400：坏 mode
    let req = Request::put(format!("/api/v1/persons/{pid}/documents/{sha}/extraction"))
        .header("content-type", "application/json")
        .body(Body::from(new_body("{\"labs\":[]}", "imgae")))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST, "坏 mode 应 400");

    // 404：查无此 sha（格式合法但不存在）
    let req = Request::put(format!("/api/v1/persons/{pid}/documents/{}/extraction", "ab".repeat(32)))
        .header("content-type", "application/json")
        .body(Body::from(new_body("{\"labs\":[]}", "text")))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND, "查无此档应 404");

    // 404：查无此人（业务 404，不是路由未命中）
    let req = Request::put(format!(
        "/api/v1/persons/00000000-0000-0000-0000-000000000000/documents/{sha}/extraction"
    ))
    .header("content-type", "application/json")
    .body(Body::from(new_body("{\"labs\":[]}", "text")))
    .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND, "查无此人应 404");

    // 端点已实装 ⇒ **不得**在 CONTRACT 表里（入表即宣称 501）
    assert!(
        !ehr_api::CONTRACT
            .iter()
            .any(|(_, p, _, _)| p.ends_with("/extraction")),
        "extraction 已实装，必须从 CONTRACT 表移除，否则 OpenAPI/文档会把它说成 501"
    );
}
