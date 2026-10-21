// D-034 · T-OCR：ingest 能力清单（`--print-capabilities`）的**库级 + HTTP 级**门禁。
//
// 三条红线，各对应一种「看起来能用其实不能」的形态：
//  ① **清单与判据同源**：清单里的 accepted/rejected 必须与 `ingest_upload` 的
//     **实际行为**逐条一致——不是「清单说什么」，而是「真上传会怎样」。
//     （同 D-009「判据形状」：判据要钉住语义，不是钉住当前快照。）
//  ② **默认构建如实拒图片**：无 ocr feature 时上传 jpg 必须**拒**（不是假装支持，
//     也不是 500「服务坏了」）；错误信息要指明**怎么开**。
//  ③ **能力清单对调用方可见**：HTTP 端点能拿到清单 ⇒ agent/客户端**自己**能判断
//     「这个构建能不能吃我的文件」，不必猜。
use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

fn tmp_home(tag: &str) -> std::path::PathBuf {
    let h = std::env::temp_dir().join(format!("ehr-cap-test-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&h);
    std::fs::create_dir_all(&h).unwrap();
    h
}

fn fixture(tag: &str) -> (std::path::PathBuf, String) {
    let home = tmp_home(tag);
    let store = ehr_api::PersonStore::new(home.clone());
    let p = store
        .create(&ehr_api::PersonNew {
            name: "林宝妹".into(),
            birthday: None,
            gender: Some("F".into()),
            id_note: None,
        })
        .unwrap();
    (home, p.id)
}

/// ① **清单与实际行为逐条一致**（不是「清单自洽」而已）。
#[test]
fn capability_manifest_agrees_with_actual_ingest_behavior() {
    use ehr_api::capabilities::*;
    let home = tmp_home("agree");
    let vault = ehr_api::vaults::open_person_vault(&home, "p1").unwrap();
    let man = capability_manifest();

    // 取一个**假的** png 字节（不是真图）：默认构建应在**扩展名判据**处就拒，
    // 不该走到解码（那会给出与能力无关的错误）。⇒ 判据是「能力」，不是「内容」。
    let fake_png = b"\x89PNG\r\n\x1a\n-not-a-real-png";
    for ext in ["png", "jpg", "jpeg", "tiff", "tif", "dcm", "exe"] {
        let name = format!("probe.{ext}");
        let r = ehr_api::vaults::ingest_upload(&vault, &name, fake_png);
        let accepted = man["accepted_exts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x.as_str() == Some(ext));
        match r {
            Ok(_) => assert!(accepted, "ext {ext} 被接受，但清单说它不在 accepted 里"),
            Err(e) => {
                let es = format!("{e:#}");
                assert!(!accepted, "ext {ext} 被拒，但清单说它在 accepted 里: {es}");
                // 拒的原因必须**可执行**（指明怎么开），不是一句「不支持」了事。
                assert!(
                    es.contains("unsupported extension"),
                    "ext {ext} 的拒绝信息没说明是扩展名问题: {es}"
                );
            }
        }
    }
    // txt 必须被接受（默认构建的唯一真能力之一）。
    let r = ehr_api::vaults::ingest_upload(&vault, "note.txt", b"hello\nworld\n");
    assert!(r.is_ok(), "txt 必须被接受: {:?}", r.err());
}

/// ② **默认构建如实拒图片**（本门禁宿主是 glibc 2.35 ⇒ ocr feature 永远 off）。
/// 若哪天宿主升级到 glibc≥2.38 且门禁用 --features ocr 跑，这门必须**跳过**而不是假绿。
#[test]
fn default_build_rejects_image_with_actionable_reason() {
    if cfg!(feature = "ocr") {
        eprintln!("[skip] 本构建开了 ocr feature ⇒ 拒图行为不适用（改由容器内 OCR 门禁覆盖）");
        return;
    }
    let home = tmp_home("reject");
    let vault = ehr_api::vaults::open_person_vault(&home, "p1").unwrap();
    let e = match ehr_api::vaults::ingest_upload(&vault, "x.jpg", b"\xff\xd8\xff\xe0") {
        Ok(_) => panic!("默认构建必须拒图片"),
        Err(e) => e,
    };
    let es = format!("{e:#}");
    assert!(es.contains("no OCR engine"), "拒绝原因必须说清是 OCR 引擎没开: {es}");
    assert!(es.contains("--features ocr"), "拒绝原因必须指明怎么开: {es}");
}

/// ③ **能力清单对调用方可见**（HTTP 级）——且与库级清单**逐字相等**。
#[tokio::test(flavor = "multi_thread")]
async fn capabilities_endpoint_matches_library_manifest() {
    let (home, _pid) = fixture("http");
    let app = ehr_api::router(ehr_api::AppState::from_home(home));

    let resp = app
        .clone()
        .oneshot(Request::get("/api/v1/capabilities").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20).await.unwrap();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v, ehr_api::capabilities::capability_manifest());
    // 关键事实对调用方可见：ocr 引擎有没有、哪些类型被拒。
    assert!(v["ocr_available"].is_boolean());
    assert!(v["accepted_exts"].is_array());
    assert!(v["rejected_exts"].is_array());
}
