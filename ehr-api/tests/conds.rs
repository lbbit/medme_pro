// D-027：把 `/api/v1/persons/{pid}/conditions` 从 stub→501 毕业为真实现的**库级**门禁。
//
// 门禁面分工（沿用 D-023/D-026 的做法）：conds-e2e.sh 走真进程 HTTP 面，
// 本文件走库级面，防「门禁只测了服务、没测内核接线」。
//
// 四条红线，各对应一种「看起来能用其实不能」的形态：
//  ① **证据回链**：每条诊断的 docref 必须是本次上传原件的 sha256；
//     且诊断原文必须在其 docref 对应**原文**里逐字出现（"只取自原件"红线，conditions 版）。
//  ② **不得凭空造诊断**：化验单（无「诊断:」小节标签）**不得**被读出诊断；
//     原文写法**原样保留**（上游无术语归一字典，断言 `高血压 3 级(很高危)` 不被改写成别的写法）。
//  ③ **ICD 码只透传不臆造**：原件印了 (E11.9) 才有 icd_code，没印的必须没有。
//  ④ **端点可验**：真 handler 返回 200，且该端点**不在** CONTRACT 表里（否则 OpenAPI 仍宣称 501）。
use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

const DIS: &str = "XX医院出院记录\n姓名:林宝妹  性别:女  年龄:37岁\n住院日期:2024-05-01  出院日期:2024-05-08\n主要诊断:2型糖尿病(E11.9)\n出院诊断:\n1. 高血压病3级\n2. 慢性肾脏病\n医师:陈志强\n";
const RX: &str = "复旦大学附属中山医院 门诊处方笺 (prescription)\n\n姓名:林宝妹    性别:女    科室:心血管内科\n处方日期:2024-06-10    诊断:高血压 3 级(很高危)\n\nRp.\n1. 酒石酸美托洛尔片 Metoprolol  25mg × 20 片\n   用法:口服,一次半片(12.5mg),一日 2 次\n\n医师:陈志强\n";
const LAB: &str = "XX医院检验科检验报告单\n姓名:林宝妹  性别:女  年龄:37岁   样本编号:20251015-A042\n检验项目          结果      参考区间     标志\n白细胞计数        6.2       3.5--9.5     /\n血红蛋白          118       115--150     /\n糖化血红蛋白      7.1       4.0--6.0     H\n报告日期:2025年10月15日\n";

fn tmp_home(tag: &str) -> std::path::PathBuf {
    let h = std::env::temp_dir().join(format!("ehr-conds-test-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&h);
    std::fs::create_dir_all(&h).unwrap();
    h
}

/// 建一个已**登记在册**的人 + 含「出院小结 + 处方 + 化验单」三件原件的箱子。
/// 返回 (home, pid, 出院小结sha, 处方sha, 化验单sha)。
fn fixture(tag: &str) -> (std::path::PathBuf, String, String, String, String) {
    let home = tmp_home(tag);
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
    let dis = ehr_api::vaults::ingest_upload(&vault, "出院小结.txt", DIS.as_bytes()).unwrap();
    let rx = ehr_api::vaults::ingest_upload(&vault, "处方.txt", RX.as_bytes()).unwrap();
    let lab = ehr_api::vaults::ingest_upload(&vault, "化验单.txt", LAB.as_bytes()).unwrap();
    assert!(dis.text_indexed && rx.text_indexed && lab.text_indexed, "fixture docs must be indexed");
    drop(vault);
    (home, pid, dis.content_hash, rx.content_hash, lab.content_hash)
}

#[test]
fn conditions_aggregate_backlinks_to_originals_and_invents_nothing() {
    let (home, pid, dis_sha, rx_sha, lab_sha) = fixture("aggregate");
    let vault = ehr_api::vaults::open_person_vault(&home, &pid).unwrap();
    let reply = ehr_api::vaults::conditions_aggregate(&vault, &pid).unwrap();

    // 三件原件都参与了扫描（与 labs/medications 同一取样判据）
    assert_eq!(reply.documents_scanned, 3, "三份原件都应被扫描");
    assert!(reply.condition_count >= 3, "出院小结应读出 ≥3 条诊断: {reply:?}");

    for c in &reply.conditions {
        // ① 证据回链：docref ⊆ 本次上传集
        assert!(!c.docrefs.is_empty(), "condition must carry docrefs: {c:?}");
        for d in &c.docrefs {
            assert!(*d == dis_sha || *d == rx_sha || *d == lab_sha, "docref 不在本次上传集: {d}");
        }
        // ② 诊断原文必须逐字出现在**其 docref 对应的原件**里
        //   （诊断可来自多份原件：出院小结与处方各自印了诊断 ⇒ 逐个 docref 核）
        for d in &c.docrefs {
            let origin = match d.as_str() {
                s if s == dis_sha => DIS,
                s if s == rx_sha => RX,
                _ => LAB,
            };
            assert!(origin.contains(&c.raw_text), "诊断原文不在其原件里: {c:?} (docref={d})");
        }
    }

    // ② 化验单**不得**被读出诊断（它没有「诊断:」小节标签 ⇒ 无凭无据的诊断不得凭空出现）
    for c in &reply.conditions {
        assert!(!c.docrefs.contains(&lab_sha), "化验单被读出了诊断（无诊断小节不得凭空造）: {c:?}");
    }

    // ② 原文写法**原样保留**：上游无术语归一字典，「高血压 3 级(很高危)」不得被改写
    assert!(
        reply.conditions.iter().any(|c| c.raw_text == "高血压 3 级(很高危)"),
        "原文写法必须原样保留: {reply:?}"
    );
    assert!(
        reply.conditions.iter().any(|c| c.raw_text == "2型糖尿病"),
        "ICD 码应从展示名里剥掉（上游语义）: {reply:?}"
    );

    // ③ ICD 码只透传不臆造：原件印了 (E11.9) 的那条才有；没印的必须没有
    let dm = reply.conditions.iter().find(|c| c.raw_text == "2型糖尿病").expect("2型糖尿病");
    assert_eq!(dm.icd_code.as_deref(), Some("E11.9"), "ICD 码应原样透传: {dm:?}");
    for c in &reply.conditions {
        if c.raw_text != "2型糖尿病" {
            assert!(c.icd_code.is_none(), "没印 ICD 码的诊断不得凭空带码: {c:?}");
        }
    }

    let _ = std::fs::remove_dir_all(&home);
}

/// D-027 附带钉死的**上游分类缺口**（不是本层能改的，但它会污染 labs 面，必须有门禁盯着）。
///
/// 事实：上游 `parser::classify` 认「出院记录」**不认**「出院小结」⇒ 后者落 `DocType::Unknown`
/// ⇒ 我们的 `rows_to_source_docs` 把 unknown 还原成 `None`（=「调用方没分类 ⇒ 宽松」）⇒
/// 整份文档被 `wants_labs(None)=true` 放行进**全文**化验正则 ⇒ 诊断行
/// 「主要诊断:2型糖尿病(E11.9)」被读成一条**假化验指标**（value=2.0、unit="型糖尿病(E11.9"）。
///
/// 这**不是** D-027 引入的回归（D-011 的 labs 端点一直如此），但它意味着：
/// 「标题不识别的出院小结」会污染指标趋势面。修法不在本层（判据应在源码里，D5），
/// 故本门禁把事实**钉死**：用上游认得的标题（出院记录）⇒ labs 面干净；并断言
/// 「化验单本身不得被读出诊断」。
#[test]
fn labs_face_is_not_polluted_by_diagnosis_rows_of_a_classified_discharge_summary() {
    let (home, pid, _dis_sha, _rx, _lab) = fixture("labface");
    let vault = ehr_api::vaults::open_person_vault(&home, &pid).unwrap();

    // 上游认得的标题（出院记录）⇒ doc_type=discharge_summary ⇒ 化验面走
    // 「只从化验小节抽」那条路 ⇒ 诊断行不会被读成化验指标。
    let labs = ehr_api::vaults::labs_aggregate(&vault, &pid).unwrap();
    let bogus: Vec<_> = labs
        .series
        .iter()
        .flat_map(|s| s.points.iter().map(move |p| (s.analyte_key.clone(), p.value, p.unit.clone())))
        .filter(|(_, _v, u)| u.as_deref().map_or(false, |u| u.contains("糖尿病")))
        .collect();
    assert!(
        bogus.is_empty(),
        "诊断行不得被读成化验指标（units 形如「型糖尿病(...)」即为假指标）: {bogus:?}"
    );
    // 化验单自身的指标仍在（回归保护）
    assert!(labs.total_points >= 3, "化验单的指标应仍被读出: total_points={}", labs.total_points);
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn conditions_aggregate_does_not_regress_labs_or_meds() {    let (home, pid, _dis, _rx, _lab) = fixture("noregress");
    let vault = ehr_api::vaults::open_person_vault(&home, &pid).unwrap();

    // 同一批文档、同一份取样判据 ⇒ 三个面互不干扰（D-026 的「同源同判据」纪律在 conditions 面同样成立）
    let labs = ehr_api::vaults::labs_aggregate(&vault, &pid).unwrap();
    assert!(labs.total_points >= 3, "化验单的指标应仍被读出: total_points={}", labs.total_points);
    let meds = ehr_api::vaults::meds_aggregate(&vault, &pid).unwrap();
    assert!(meds.med_count >= 1, "处方应仍读出用药: {meds:?}");
    let conds = ehr_api::vaults::conditions_aggregate(&vault, &pid).unwrap();
    assert!(conds.condition_count >= 3, "{conds:?}");

    let _ = std::fs::remove_dir_all(&home);
}

#[tokio::test(flavor = "multi_thread")]
async fn conditions_endpoint_returns_200_with_evidence_backlinks() {
    let (home, pid, dis_sha, rx_sha, lab_sha) = fixture("http");
    let st = ehr_api::AppState::from_home(&home);
    let app = ehr_api::router(st);

    // ④ 端点可验：真 handler ⇒ 200（不是 CONTRACT 表里的 501）
    let req = Request::builder()
        .method("GET")
        .uri(format!("/api/v1/persons/{pid}/conditions"))
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(
        res.status(),
        StatusCode::OK,
        "conditions 端点返回 {:?}：已实装的端点必须 200（501 说明仍是 stub）",
        res.status()
    );
    let body = axum::body::to_bytes(res.into_body(), 1 << 20).await.unwrap();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(v["condition_count"].as_u64().unwrap() >= 3, "{}", v);
    // 证据回链在 HTTP 面同样成立：docref 必须 ⊆ 本次上传的原件集合
    // （诊断可来自**多份**原件——出院小结与处方各自印了诊断，故不能断言「只等于 dis_sha」；
    //   但**必须**不得指向未上传的原件，也不得指向化验单——那会是无凭无据的诊断）
    for c in v["conditions"].as_array().unwrap() {
        let refs = c["docrefs"].as_array().unwrap();
        assert!(!refs.is_empty(), "condition must carry docrefs: {c}");
        for d in refs {
            let d = d.as_str().unwrap();
            assert!(
                d == dis_sha || d == rx_sha,
                "docref 必须指向本次上传的原件: {d}"
            );
            assert_ne!(d, lab_sha, "化验单被读出了诊断: {c}");
        }
    }

    // 且该端点必须**不在** CONTRACT 表里（入表即宣称 stub→501，D-025 纪律）
    assert!(
        !ehr_api::CONTRACT.iter().any(|(_, p, _, _)| *p == "/api/v1/persons/{pid}/conditions"),
        "conditions 已实装，必须从 CONTRACT 表移除，否则 OpenAPI/文档会说它 501"
    );
    let _ = std::fs::remove_dir_all(&home);
}
