// D-028：写侧手动条目（self-measurement / note / profile-event）+ 读侧 measurements 投影
// 的**库级**门禁。
//
// 四条红线，各对应一种「看起来能用其实不能」的形态：
//  ① **写入真落地**：写完必须在 timeline/documents 里看得见，且合成文本可**回链到字节**
//     （docref = CAS sha256，可走 documents/{sha}/original 取回）——「以为写进去了其实没有」。
//  ② **不得静默吞写**：CAS 按**内容**去重 ⇒ 若两次写入正文逐字相同，第二条会消失。
//     门禁钉死「正文带毫秒记录时间 ⇒ 每次写入字节不同 ⇒ 不会吞写」（上游同款坑）。
//  ③ **数值不被粉饰**：生理学不可能的值（138388 mmHg / 收缩压≤舒张压）必须被拒且**带原因**；
//     未知 analyte 不出参考区间与标志（上游 `home_ref_range` 的 `_ => None` 分支）。
//  ④ **端点可验**：真 handler 返回 200/201，且这些端点**不在** CONTRACT 表里
//     （否则 OpenAPI 仍宣称 501 —— D-025 的反向保险）。
use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

fn tmp_home(tag: &str) -> std::path::PathBuf {
    let h = std::env::temp_dir().join(format!("ehr-entries-test-{tag}-{}", std::process::id()));
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

/// ① 写入真落地 + 证据回链：自测记录写完，能在 documents 里看到，且 docref 可回链。
#[test]
fn self_measurement_lands_in_the_vault_and_is_traceable_to_bytes() {
    let (home, pid) = fixture("land");
    let vault = ehr_api::vaults::open_person_vault(&home, &pid).unwrap();
    let req = ehr_api::entries::SelfMeasurementIn {
        values: vec![
            ehr_api::entries::SelfMeasuredValueIn {
                analyte_key: "bp_systolic".into(),
                value: 128.0,
                unit: "mmHg".into(),
            },
            ehr_api::entries::SelfMeasuredValueIn {
                analyte_key: "bp_diastolic".into(),
                value: 82.0,
                unit: "mmHg".into(),
            },
        ],
        taken_at: Some("2026-10-23T08:30:00+08:00".into()),
    };
    let reply = ehr_api::entries::add_self_measurement(&vault, &req).unwrap();
    assert!(!reply.deduped, "首次写入不得被判成重复");
    assert_eq!(reply.doc_type, "self_measurement");

    // 文档确实在箱子里（timeline 看得见）
    let tl = vault.timeline().unwrap();
    assert_eq!(tl.len(), 1, "时间线应有一条记录: {tl:?}");
    assert_eq!(tl[0].doc_type.as_str(), "self_measurement");

    // 证据回链：docref = CAS sha256，能取回字节，且字节里含结构化载荷哨兵
    let items = ehr_api::vaults::list_document_items(&vault).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].content_hash, reply.content_hash, "docref 必须是合成文本的 sha");
    assert!(items[0].text_indexed, "写入的记录必须有文本层（否则读侧隐形）");
    let bytes = match ehr_api::vaults::cas_read(&vault, &reply.content_hash) {
        ehr_api::vaults::CasOutcome::Bytes(b) => b,
        _ => panic!("CAS 回链失败（回链不上=证据链断了）"),
    };
    let text = String::from_utf8(bytes).unwrap();
    assert!(
        text.contains(parser::SELF_MEASUREMENT_MARKER),
        "正文必须带上游的版本化载荷哨兵（写入方与读取方同一对代码）: {text}"
    );

    // 读侧投影能读回**逐字一致**的值（上游 parse_self_measurement_payload）
    let q = ehr_api::entries::MeasurementsQuery::default();
    let proj = ehr_api::entries::measurements_projection(&vault, &pid, &q).unwrap();
    assert_eq!(proj.count, 1, "{proj:?}");
    let m = &proj.measurements[0];
    assert!(m.payload_readable, "载荷必须读得回: {m:?}");
    assert_eq!(m.values.len(), 2, "血压是两个值: {m:?}");
    assert_eq!(m.values[0].analyte_key, "bp_systolic");
    assert_eq!(m.values[0].value, 128.0);
    assert_eq!(m.values[0].unit, "mmHg");
    // 家庭参考区间：上游 home_ref_range 对血压给的是**上限**（家庭自测血压正常上限
    // 135/85，无下限——上游既定事实）⇒ 至少有一端，且必须带出处。
    assert!(
        m.values[0].ref_low.is_some() || m.values[0].ref_high.is_some(),
        "血压应有家庭区间: {m:?}"
    );
    assert!(m.values[0].ref_source.is_some(), "区间必须带出处（查不到出处就不该给区间）");
    // 标志：128 在 135 上限之内 ⇒ N（上游：有任一端区间即给 N；无区间才不给）
    assert_eq!(m.values[0].flag.as_deref(), Some("N"), "{m:?}");

    let _ = std::fs::remove_dir_all(&home);
}

/// ② 不得静默吞写：两条**内容完全相同**的笔记必须落成两条记录。
/// （CAS 按内容去重 ⇒ 正文若逐字相同，第二条会被吞掉且接口照样回 201。）
#[test]
fn identical_notes_are_not_silently_deduped_away() {
    let (home, pid) = fixture("dedup");
    let vault = ehr_api::vaults::open_person_vault(&home, &pid).unwrap();
    let req = ehr_api::entries::NoteIn {
        text: "今天有点头晕".into(),
        taken_at: None,
    };
    let a = ehr_api::entries::add_note(&vault, &req).unwrap();
    let b = ehr_api::entries::add_note(&vault, &req).unwrap();
    assert_ne!(a.content_hash, b.content_hash, "两次写入的字节必须不同");
    assert!(!b.deduped, "第二次写入不得被判成重复");
    assert_eq!(vault.timeline().unwrap().len(), 2, "两条笔记应落成两条记录");

    // 读侧：note 的 text 是**用户原文**（存储时补的「记录时间」行应被剥掉）
    let q = ehr_api::entries::MeasurementsQuery::default();
    let proj = ehr_api::entries::measurements_projection(&vault, &pid, &q).unwrap();
    assert_eq!(proj.count, 2);
    for m in &proj.measurements {
        assert_eq!(m.text.as_deref(), Some("今天有点头晕"), "note 原文应原样透传: {m:?}");
    }
    let _ = std::fs::remove_dir_all(&home);
}

/// ③ 数值不被粉饰：不可能的值被拒（带原因）；未知 analyte 不出区间/标志。
#[test]
fn implausible_values_are_rejected_and_unknown_analyte_gets_no_range() {
    let (home, pid) = fixture("plausible");
    let vault = ehr_api::vaults::open_person_vault(&home, &pid).unwrap();

    // 138388 mmHg ⇒ 400 类错误 + 具体原因
    let bad = ehr_api::entries::SelfMeasurementIn {
        values: vec![ehr_api::entries::SelfMeasuredValueIn {
            analyte_key: "bp_systolic".into(),
            value: 138_388.0,
            unit: "mmHg".into(),
        }],
        taken_at: None,
    };
    let err = ehr_api::entries::add_self_measurement(&vault, &bad).unwrap_err();
    match err {
        ehr_api::entries::EntryError::BadRequest(m) => {
            assert!(m.contains("implausible"), "错误必须带原因: {m}")
        }
        other => panic!("应是 400 类错误，实得 {other:?}"),
    }
    assert_eq!(vault.timeline().unwrap().len(), 0, "被拒的写入不得留下半成品");

    // 未知 analyte：不报错（上游刻意不做白名单），但**不得**给区间/标志
    let unknown = ehr_api::entries::SelfMeasurementIn {
        values: vec![ehr_api::entries::SelfMeasuredValueIn {
            analyte_key: "mystery_marker".into(),
            value: 42.0,
            unit: "x".into(),
        }],
        taken_at: None,
    };
    let r = ehr_api::entries::add_self_measurement(&vault, &unknown).unwrap();
    let q = ehr_api::entries::MeasurementsQuery::default();
    let proj = ehr_api::entries::measurements_projection(&vault, &pid, &q).unwrap();
    let m = &proj.measurements[0];
    assert_eq!(m.values.len(), 1);
    assert_eq!(m.values[0].analyte_key, "mystery_marker");
    assert!(m.values[0].ref_low.is_none() && m.values[0].ref_high.is_none(), "未知项不得有区间");
    assert!(m.values[0].flag.is_none(), "无区间时不得猜标志（不猜「正常」）");
    assert_eq!(m.docref, r.content_hash);
    let _ = std::fs::remove_dir_all(&home);
}

/// profile-event：载荷逐字读回，事件发生日（at）与记录日**分开**。
#[test]
fn profile_event_payload_round_trips_and_event_date_is_separate() {
    let (home, pid) = fixture("pe");
    let vault = ehr_api::vaults::open_person_vault(&home, &pid).unwrap();
    let req = ehr_api::entries::ProfileEventIn {
        kind: "enable".into(),
        package: "sle".into(),
        at: "2026-10-20".into(),
        payload: serde_json::json!({"severity": "mild"}),
    };
    let reply = ehr_api::entries::add_profile_event(&vault, &req).unwrap();
    assert_eq!(reply.doc_type, "profile_event");

    let q = ehr_api::entries::MeasurementsQuery::default();
    let proj = ehr_api::entries::measurements_projection(&vault, &pid, &q).unwrap();
    assert_eq!(proj.count, 1);
    let m = &proj.measurements[0];
    assert!(m.payload_readable);
    assert_eq!(m.event_kind.as_deref(), Some("enable"));
    assert_eq!(m.event_package.as_deref(), Some("sle"));
    assert_eq!(m.event_at.as_deref(), Some("2026-10-20"), "事件发生日应原样透传");
    assert_eq!(m.event_payload.as_ref().unwrap()["severity"], "mild");

    // at 形状不对 ⇒ 400 带原因（不改日期，边界就挡住）
    let bad = ehr_api::entries::ProfileEventIn {
        kind: "enable".into(),
        package: "sle".into(),
        at: "20/10/2026".into(),
        payload: serde_json::json!({}),
    };
    assert!(matches!(
        ehr_api::entries::add_profile_event(&vault, &bad).unwrap_err(),
        ehr_api::entries::EntryError::BadRequest(_)
    ));
    let _ = std::fs::remove_dir_all(&home);
}

/// 过滤器**真过滤**（不是「传了没反应」）：kind / from / to 各自生效。
#[test]
fn measurements_filters_actually_filter() {
    let (home, pid) = fixture("filter");
    let vault = ehr_api::vaults::open_person_vault(&home, &pid).unwrap();
    ehr_api::entries::add_self_measurement(
        &vault,
        &ehr_api::entries::SelfMeasurementIn {
            values: vec![ehr_api::entries::SelfMeasuredValueIn {
                analyte_key: "glucose".into(),
                value: 5.6,
                unit: "mmol/L".into(),
            }],
            taken_at: Some("2026-10-01T08:00:00Z".into()),
        },
    )
    .unwrap();
    ehr_api::entries::add_note(
        &vault,
        &ehr_api::entries::NoteIn {
            text: "十月二号".into(),
            taken_at: Some("2026-10-02T08:00:00Z".into()),
        },
    )
    .unwrap();

    let all = ehr_api::entries::measurements_projection(
        &vault,
        &pid,
        &ehr_api::entries::MeasurementsQuery::default(),
    )
    .unwrap();
    assert_eq!(all.count, 2);

    let only_note = ehr_api::entries::measurements_projection(
        &vault,
        &pid,
        &ehr_api::entries::MeasurementsQuery {
            kind: Some("note".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(only_note.count, 1, "kind 过滤器必须真过滤");
    assert_eq!(only_note.measurements[0].kind, "note");

    let oct = ehr_api::entries::measurements_projection(
        &vault,
        &pid,
        &ehr_api::entries::MeasurementsQuery {
            from: Some("2026-10-02".into()),
            to: Some("2026-10-31".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(oct.count, 1, "from/to 过滤器必须真过滤");
    assert_eq!(oct.measurements[0].kind, "note");

    // 非法 kind / 非法日期 ⇒ 400 类错误（带原因），不静默忽略
    assert!(matches!(
        ehr_api::entries::measurements_projection(
            &vault,
            &pid,
            &ehr_api::entries::MeasurementsQuery {
                kind: Some("nope".into()),
                ..Default::default()
            }
        )
        .unwrap_err(),
        ehr_api::entries::EntryError::BadRequest(_)
    ));
    assert!(matches!(
        ehr_api::entries::measurements_projection(
            &vault,
            &pid,
            &ehr_api::entries::MeasurementsQuery {
                from: Some("2026-13-40".into()),
                ..Default::default()
            }
        )
        .unwrap_err(),
        ehr_api::entries::EntryError::BadRequest(_)
    ));
    let _ = std::fs::remove_dir_all(&home);
}

/// ④ 端点可验：HTTP 面 201/200，且三个写端点 + 读端点**不在** CONTRACT 表里。
#[tokio::test(flavor = "multi_thread")]
async fn entry_endpoints_are_real_and_not_in_contract_table() {
    let (home, pid) = fixture("http");
    let st = ehr_api::AppState::from_home(&home);
    let app = ehr_api::router(st);

    // 写自测 ⇒ 201
    let body = serde_json::json!({
        "values": [{"analyte_key": "heart_rate", "value": 72, "unit": "bpm"}]
    });
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/persons/{pid}/entries/self-measurement"))
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::CREATED, "写自测应 201（501 说明仍是 stub）");
    let b = axum::body::to_bytes(res.into_body(), 1 << 20).await.unwrap();
    let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(v["doc_type"], "self_measurement");
    assert!(v["content_hash"].as_str().unwrap().len() == 64, "回执必须带 CAS sha256");

    // 不可能的值 ⇒ 400（不是 500）
    let bad = serde_json::json!({
        "values": [{"analyte_key": "bp_systolic", "value": 138388, "unit": "mmHg"}]
    });
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/persons/{pid}/entries/self-measurement"))
        .header("content-type", "application/json")
        .body(Body::from(bad.to_string()))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST, "不可能的值应 400");

    // 写笔记 ⇒ 201
    let body = serde_json::json!({"text": "今天有点头晕"});
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/persons/{pid}/entries/note"))
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);

    // 写 profile-event ⇒ 201
    let body = serde_json::json!({"kind":"enable","package":"sle","at":"2026-10-20","payload":{}});
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/persons/{pid}/entries/profile-event"))
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);

    // 读侧 measurements ⇒ 200 且三条都在
    let req = Request::builder()
        .method("GET")
        .uri(format!("/api/v1/persons/{pid}/measurements"))
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK, "measurements 应 200（501 说明仍是 stub）");
    let b = axum::body::to_bytes(res.into_body(), 1 << 20).await.unwrap();
    let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(v["count"], 3, "{v}");

    // 未知人 ⇒ 404（不是 501 也不是 500）
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/persons/00000000-0000-4000-8000-000000000000/measurements")
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    // 且这些端点必须**不在** CONTRACT 表里（入表即宣称 stub→501）
    for p in [
        "/api/v1/persons/{pid}/measurements",
        "/api/v1/persons/{pid}/entries/self-measurement",
        "/api/v1/persons/{pid}/entries/note",
        "/api/v1/persons/{pid}/entries/profile-event",
    ] {
        assert!(
            !ehr_api::CONTRACT.iter().any(|(_, cp, _, _)| *cp == p),
            "{p} 已实装，必须从 CONTRACT 表移除，否则 OpenAPI/文档会说它 501"
        );
    }
    let _ = std::fs::remove_dir_all(&home);
}
