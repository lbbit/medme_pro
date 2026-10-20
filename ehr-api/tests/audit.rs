// D-030：`GET /api/v1/audit` 从 stub→501 毕业为真实现的**库级**门禁。
//
// 门禁面分工（沿用 D-023/D-026 做法）：audit-e2e.sh 走真进程 HTTP 面，本文件走库级面。
//
// 三条红线，各对应一种「看起来能用其实不能」的形态：
//  ① **口径分层、不假装只有一层**：审计有两层真相层——registry（人员主数据）与
//     vault（上游内核事件日志）。只实现一层却不说 ⇒ 审计「看起来完整」实则漏账。
//  ② **vault 侧复用上游判据**：能复用的判据一律复用上游 `Vault::audit_log()`，
//     **不另写一份**「哪些事件算审计条目」的判据（D2：一个语义只允许一处实现）。
//     硬证据不是「两边都有数据」，而是 **vault 侧的条目与上游 `audit_log()` 逐条相等**。
//  ③ **registry 侧不折叠**：折叠会把 update 历史抹平、把 delete 墓碑藏掉
//     （D5：以为有、其实没有）。
//  ④ **参数真进判据**：`person`/`op` 是真过滤（不是摆设）。
use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

fn tmp_home(tag: &str) -> std::path::PathBuf {
    let h = std::env::temp_dir().join(format!("ehr-audit-test-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&h);
    std::fs::create_dir_all(&h).unwrap();
    h
}

/// 造一个 home：一个人 + 一份上传原件（⇒ vault 侧有「导入」审计条目）。
/// 返回 (home, pid, sha)。
fn fixture(tag: &str) -> (std::path::PathBuf, String, String) {
    let home = tmp_home(tag);
    let store = ehr_api::PersonStore::new(home.clone());
    let person = store
        .create(&ehr_api::PersonNew {
            name: "审计门禁·甲".into(),
            birthday: Some("1980-01-02".into()),
            gender: Some("F".into()),
            id_note: None,
        })
        .unwrap();
    let pid = person.id;
    let vault = ehr_api::vaults::open_person_vault(&home, &pid).unwrap();
    let up = ehr_api::vaults::ingest_upload(&vault, "血常规报告.txt", "白细胞计数 6.2 3.5--9.5 /\n".as_bytes()).unwrap();
    drop(vault);
    (home, pid, up.content_hash)
}

#[test]
fn audit_view_covers_both_truth_layers_and_reuses_upstream_judgment() {
    let (home, pid, sha) = fixture("layers");

    // registry 侧：原始事件（create 一条）
    let store = ehr_api::PersonStore::new(home.clone());
    let reg_events = store.events().unwrap();
    assert_eq!(reg_events.len(), 1, "建档应产生一条 registry 事件: {reg_events:?}");
    assert_eq!(reg_events[0].op, "create");
    assert_eq!(reg_events[0].person_id, pid);

    // vault 侧：与上游 `audit_log()` **逐条相等**（这是「复用上游判据」的硬证据）
    let vault = ehr_api::vaults::open_person_vault(&home, &pid).unwrap();
    let upstream = vault.audit_log().unwrap();
    drop(vault);
    assert!(!upstream.is_empty(), "上传原件应产生导入审计条目");

    let q = ehr_api::audit::AuditQuery::default();
    let reply = ehr_api::audit::audit_view(&home, &q).unwrap();
    assert_eq!(reply.count, reg_events.len() + upstream.len(),
        "审计条目数必须 = registry 事件数 + 上游审计条数（不另写判据、不多不少）: {reply:?}");

    let vault_side: Vec<_> = reply.entries.iter().filter(|e| e.source == "vault").collect();
    assert_eq!(vault_side.len(), upstream.len());
    // 逐条相等（内容哈希 + 动作名 + 时刻）
    for u in &upstream {
        let hit = reply
            .entries
            .iter()
            .find(|e| e.source == "vault" && e.sha256.as_deref() == u.sha256.as_deref() && e.op == u.action);
        assert!(hit.is_some(), "上游审计条目在审计视图里找不到: {u:?}");
        let hit = hit.unwrap();
        assert_eq!(hit.ts, u.timestamp, "时刻必须原样透传");
        assert_eq!(hit.person_id, pid, "vault 侧条目必须归到该人");
    }
    // 导入条目的 sha256 = 那份原件的 sha256（内容哈希留痕）
    assert!(reply.entries.iter().any(|e| e.sha256.as_deref() == Some(sha.as_str())),
        "导入审计条目应带原件内容哈希: {reply:?}");

    // registry 侧条目：source=registry、op=create、detail 带人读摘要
    let reg = reply.entries.iter().find(|e| e.source == "registry").expect("registry 条目");
    assert_eq!(reg.op, "create");
    assert_eq!(reg.person_id, pid);
    assert!(reg.detail.as_deref().unwrap_or("").contains("审计门禁·甲"), "{reg:?}");
    assert!(reg.sha256.is_none(), "registry 事件没有内容哈希（不臆造）: {reg:?}");

    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn audit_filters_person_and_op_for_real() {
    let (home, pid, _sha) = fixture("filter");
    // 再建第二个人 ⇒ 审计里应有两人
    let store = ehr_api::PersonStore::new(home.clone());
    let other = store
        .create(&ehr_api::PersonNew { name: "审计门禁·乙".into(), birthday: None, gender: None, id_note: None })
        .unwrap();

    // person 过滤：只回该人的条目
    let q = ehr_api::audit::AuditQuery { person: Some(pid.clone()), op: None };
    let r = ehr_api::audit::audit_view(&home, &q).unwrap();
    assert!(r.count > 0, "该人应有审计条目");
    assert!(r.entries.iter().all(|e| e.person_id == pid), "person 过滤未生效: {r:?}");

    // op 过滤（registry 侧 create）
    let q = ehr_api::audit::AuditQuery { person: None, op: Some("create".into()) };
    let r = ehr_api::audit::audit_view(&home, &q).unwrap();
    assert!(r.count >= 2, "两人各一条 create: {r:?}");
    assert!(r.entries.iter().all(|e| e.op == "create"), "op 过滤未生效: {r:?}");

    // op 过滤（vault 侧上游中文动作名）
    let q = ehr_api::audit::AuditQuery { person: None, op: Some("导入".into()) };
    let r = ehr_api::audit::audit_view(&home, &q).unwrap();
    assert!(r.count >= 1, "导入条目应命中: {r:?}");
    assert!(r.entries.iter().all(|e| e.op == "导入"), "op 过滤未生效: {r:?}");

    // 大小写不敏感
    let q = ehr_api::audit::AuditQuery { person: None, op: Some("CREATE".into()) };
    let r = ehr_api::audit::audit_view(&home, &q).unwrap();
    assert!(r.count >= 2, "op 过滤应大小写不敏感: {r:?}");

    // 别人的 id 过滤 ⇒ 0 条（过滤器真在判据里，不是摆设）
    let q = ehr_api::audit::AuditQuery { person: Some(other.id.clone()), op: Some("导入".into()) };
    let r = ehr_api::audit::audit_view(&home, &q).unwrap();
    assert_eq!(r.count, 0, "乙没有导入件 ⇒ 0 条: {r:?}");

    let _ = std::fs::remove_dir_all(&home);
}

#[tokio::test(flavor = "multi_thread")]
async fn audit_endpoint_returns_200_and_is_not_in_contract_table() {
    let (home, pid, _sha) = fixture("http");
    let st = ehr_api::AppState::from_home(&home);
    let app = ehr_api::router(st);

    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/audit")
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(
        res.status(),
        StatusCode::OK,
        "audit 端点返回 {:?}：已实装的端点必须 200（501 说明仍是 stub）",
        res.status()
    );
    let body = axum::body::to_bytes(res.into_body(), 1 << 20).await.unwrap();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(v["count"].as_u64().unwrap() >= 2, "{}", v);
    assert!(v["entries"].is_array(), "{}", v);
    assert!(v["entries"].as_array().unwrap().iter().any(|e| e["source"] == "registry"), "{}", v);

    // person 过滤在 HTTP 面同样真过滤
    let req = Request::builder()
        .method("GET")
        .uri(format!("/api/v1/audit?person={pid}"))
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), 1 << 20).await.unwrap();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    for e in v["entries"].as_array().unwrap() {
        assert_eq!(e["person_id"].as_str().unwrap(), pid, "person 过滤未生效: {v}");
    }

    // 已实装 ⇒ 必须从 CONTRACT 表移除（入表即宣称 stub→501，D-025 纪律）
    assert!(
        !ehr_api::CONTRACT.iter().any(|(_, p, _, _)| *p == "/api/v1/audit"),
        "audit 已实装，必须从 CONTRACT 表移除，否则 OpenAPI/文档会说它 501"
    );
    let _ = std::fs::remove_dir_all(&home);
}
