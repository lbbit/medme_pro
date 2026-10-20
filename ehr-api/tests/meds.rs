// D-026：把 `/api/v1/persons/{pid}/medications` 从 stub→501 毕业为真实现的**库级**门禁。
//
// 门禁面分工（沿用 D-023 的做法）：meds-e2e.sh 走真进程 HTTP 面，本文件走库级面，
// 防「门禁只测了服务、没测内核接线」。
//
// 三条红线，各对应一种「看起来能用其实不能」的形态：
//  ① **证据回链**：每条 mention 的 docref 必须是本次上传原件的 sha256；
//     且药名/剂量必须在其 docref 对应**原文**里逐字出现（"只取自原件"红线， meds 版）。
//  ② **类型分流不得退化**：doc_type 如实传 ⇒ 化验单**不得**被读成用药
//     （上游 wants_meds 只放行 prescription；传 None = 全放行 = 化验单行被误读成药）。
//  ③ **端点可验**：真 handler 返回 200，且该端点**不在** CONTRACT 表里（否则 OpenAPI 仍宣称 501）。
use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

const LAB: &str = "XX医院检验科检验报告单\n姓名:林宝妹  性别:女\n检验项目          结果      参考区间     标志\n白细胞计数        6.2       3.5--9.5     /\n糖化血红蛋白      7.1       4.0--6.0     H\n报告日期:2025年10月15日\n";
const RX: &str = "复旦大学附属中山医院 门诊处方笺 (prescription)\n\n姓名:张建国    性别:男    年龄:59岁    科室:心血管内科\n处方日期:2024-06-10    诊断:高血压 3 级(很高危)\n\nRp.\n1. 酒石酸美托洛尔片 Metoprolol  25mg × 20 片\n   用法:口服,一次半片(12.5mg),一日 2 次\n2. 苯磺酸氨氯地平片 Amlodipine  5mg × 14 片\n   用法:口服,一次 1 片,一日 1 次(晨服)\n3. 阿司匹林肠溶片 Aspirin  100mg × 30 片\n   用法:口服,一次 1 片,一日 1 次(餐后)\n\n医师:陈志强\n";

fn tmp_home(tag: &str) -> std::path::PathBuf {
    let h = std::env::temp_dir().join(format!("ehr-meds-test-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&h);
    std::fs::create_dir_all(&h).unwrap();
    h
}

/// 建一个已**登记在册**的人 + 含「化验单 + 处方」两件原件的箱子。
/// 返回 (home, pid, 化验单sha, 处方sha)。
fn fixture(tag: &str) -> (std::path::PathBuf, String, String, String) {
    let home = tmp_home(tag);
    // 先在 registry 建档（pid 由服务端生成 ⇒ 是真 UUID，也保证 require_person 放行）
    let store = ehr_api::PersonStore::new(home.clone());
    let person = store
        .create(&ehr_api::PersonNew {
            name: "林宝妹".into(),
            birthday: None,
            gender: Some("F".into()),
            id_note: None,
        })
        .unwrap();
    let pid = person.id;
    let vault = ehr_api::vaults::open_person_vault(&home, &pid).unwrap();
    let lab = ehr_api::vaults::ingest_upload(&vault, "血常规报告.txt", LAB.as_bytes()).unwrap();
    let rx = ehr_api::vaults::ingest_upload(&vault, "处方.txt", RX.as_bytes()).unwrap();
    assert!(lab.text_indexed && rx.text_indexed, "fixture docs must be indexed");
    drop(vault);
    (home, pid, lab.content_hash, rx.content_hash)
}

#[test]
fn meds_aggregate_reads_only_prescription_docs_and_backlinks_to_originals() {
    let (home, pid, lab_sha, rx_sha) = fixture("aggregate");
    let vault = ehr_api::vaults::open_person_vault(&home, &pid).unwrap();
    let reply = ehr_api::vaults::meds_aggregate(&vault, &pid).unwrap();

    // ② 类型分流：两件原件都参与了扫描（documents_scanned=2），
    //    但**用药只来自处方**（化验单不产生任何 mention）。
    assert_eq!(reply.documents_scanned, 2, "两份原件都应被扫描");
    assert!(reply.med_count >= 3, "处方应读出 ≥3 个药: {reply:?}");
    assert!(reply.mention_count >= 3, "{reply:?}");
    for m in &reply.medications {
        // ① 证据回链：docref ⊆ 本次上传集，且**不得**指向化验单
        assert!(!m.docrefs.is_empty(), "med span must carry docrefs: {m:?}");
        for d in &m.docrefs {
            assert!(*d == rx_sha || *d == lab_sha, "docref 不在本次上传集: {d}");
            assert_ne!(*d, lab_sha, "化验单被读成了用药（doc_type 分流退化）: {m:?}");
        }
        for mm in &m.mentions {
            assert_eq!(mm.docref, rx_sha, "mention 必须来自处方原件: {mm:?}");
            // 剂量/药名逐字在原文里（"只取自原件"红线）
            assert!(RX.contains(&mm.raw_name), "药名不在原文: {mm:?}");
            if let Some(dose) = &mm.dose {
                assert!(RX.contains(dose), "剂量不在原文: {mm:?}");
            }
        }
    }
    // 上游不推断停药：status 恒 active（透传，不改写）
    assert!(reply.medications.iter().all(|m| m.status == "active"), "{reply:?}");
    let _ = std::fs::remove_dir_all(&home);
}

#[tokio::test(flavor = "multi_thread")]
async fn medications_endpoint_returns_200_with_evidence_backlinks() {
    let (home, pid, _lab_sha, rx_sha) = fixture("http");
    let st = ehr_api::AppState::from_home(&home);
    let app = ehr_api::router(st);

    // ③ 端点可验：真 handler ⇒ 200（不是 CONTRACT 表里的 501）
    let req = Request::builder()
        .method("GET")
        .uri(format!("/api/v1/persons/{pid}/medications"))
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(
        res.status(),
        StatusCode::OK,
        "medications 端点返回 {:?}：已实装的端点必须 200（501 说明仍是 stub）",
        res.status()
    );
    let body = axum::body::to_bytes(res.into_body(), 1 << 20).await.unwrap();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(v["med_count"].as_u64().unwrap() >= 3, "{}", v);
    // 证据回链在 HTTP 面同样成立
    for m in v["medications"].as_array().unwrap() {
        for d in m["docrefs"].as_array().unwrap() {
            assert_eq!(d.as_str().unwrap(), rx_sha, "docref 必须指向处方原件: {d}");
        }
    }

    // 且该端点必须**不在** CONTRACT 表里（入表即宣称 stub→501，D-025 纪律）
    assert!(
        !ehr_api::CONTRACT.iter().any(|(_, p, _, _)| *p == "/api/v1/persons/{pid}/medications"),
        "medications 已实装，必须从 CONTRACT 表移除，否则 OpenAPI/文档会说它 501"
    );
    let _ = std::fs::remove_dir_all(&home);
}
