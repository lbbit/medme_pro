// T38：person registry（**本项目唯一新增真相层**）并发写完整性门禁。
//
// 为什么要有这道门（T38 实测抓到的真缺陷，非推演）：
//   `PersonStore::append()` 原先用 `serde_json::to_writer(&mut f, ev)` 直接往真相层文件写
//   ⇒ 那**不是一次写**，而是几十次小 write syscall ⇒ 两个并发建档请求的这些小写会
//   **交错落盘** ⇒ `registry/persons.jsonl` 里出现「半行 + 半行」的交错垃圾。
//   实测（60 个并发 `POST /persons`）：**59 行里 29 行是交错垃圾** ⇒ 此后
//   `GET /persons`、`/timeline`、审计视图等**所有**读面**永久 500**
//   （真相层被写坏，且**不可自愈** —— 只有人工去修 JSONL）。
//   ⇒ 这不是「测试抖动」，是**并发写能把真相层打坏** ⇒ 必须有机制 + 必须有门。
//
// 三条判据（各对应一种「看起来修好了其实没修」）：
//   ① **并发建档后真相层每行都是完整 JSON**（行内不破碎）—— 直接钉住「整行一次写」；
//   ② **并发建档后读面仍然可用**（`list()` 不报错，且条数 == 并发数）——
//      钉住「真相层没被写坏 ⇒ 读面不被永久 500」；
//   ③ **末行不完整（有并发写在途）≠ 真相层坏了** —— 判据必须能区分这两种情况：
//      · 末行不完整（无结尾 `\n`）⇒ 读侧**跳过**它并 warn（还没写完，不是坏）；
//      · **完整行**解析失败 ⇒ 那是真坏了 ⇒ **必须报错**，绝不「跳过坏行」掩盖。
//      ⇒ 门里都各测一次（正例 + 反例），否则门自己就没有判别力。
use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

fn tmp_home(tag: &str) -> std::path::PathBuf {
    let h = std::env::temp_dir().join(format!("ehr-registry-conc-test-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&h);
    std::fs::create_dir_all(h.join("registry")).unwrap();
    std::fs::create_dir_all(h.join("vaults")).unwrap();
    h
}

fn registry_file(home: &std::path::Path) -> std::path::PathBuf {
    home.join("registry").join("persons.jsonl")
}

/// 读真相层原始字节（**不**做任何清理）⇒ 判据直接看落盘的东西，而不是看 API 的说法。
fn raw_registry(home: &std::path::Path) -> String {
    std::fs::read_to_string(registry_file(home)).unwrap_or_default()
}

/// 判据①+②：**并发**建档后，真相层每行都是完整 JSON，且读面可用、条数正确。
///
/// 用**库级**并发（多个 OS 线程同时 `store.create()`）而不是 HTTP 面：
///   库级能精确控制并发度与条数，判据更聚焦（不掺 HTTP 层的变量）；
///   HTTP 面那条路径由 `registry-concurrency-e2e.sh` 覆盖（真进程 + 真 HTTP）。
#[test]
fn concurrent_creates_leave_registry_intact() {
    const N: usize = 64;
    let home = tmp_home("concurrent");
    let store = std::sync::Arc::new(ehr_api::PersonStore::new(home.clone()));

    let mut handles = Vec::new();
    for i in 0..N {
        let s = store.clone();
        handles.push(std::thread::spawn(move || {
            let req = ehr_api::PersonNew {
                name: format!("并发-{i}"),
                birthday: Some("1980-05-05".into()),
                gender: Some("male".into()),
                id_note: None,
            };
            s.create(&req).expect("create must succeed")
        }));
    }
    let created: Vec<_> = handles.into_iter().map(|h| h.join().expect("thread joined")).collect();
    assert_eq!(created.len(), N, "all creates must return");

    // ① 真相层每行都是**完整 JSON**（行内不破碎）—— 直接验落盘字节
    //    ⚠️ 顺序有意：先判「行是否完整」，再判「行数」—— 缺陷现场是**交错半行**，
    //    那会同时打破两者；先报「哪一行坏了」才是可定位的证据（行数只是症状）。
    let raw = raw_registry(&home);
    let lines: Vec<&str> = raw.lines().filter(|l| !l.trim().is_empty()).collect();
    let broken: Vec<String> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| serde_json::from_str::<serde_json::Value>(l).is_err())
        .map(|(i, l)| format!("line {}: {}", i + 1, l))
        .collect();
    assert!(
        broken.is_empty(),
        "registry must contain only intact JSON lines; {} of {} lines are interleaved/corrupt:\n  {}",
        broken.len(),
        lines.len(),
        broken.join("\n  ")
    );
    assert_eq!(
        lines.len(),
        N,
        "registry must contain exactly {N} lines (one per create); got {}",
        lines.len()
    );

    // ② 读面仍然可用，且条数 == 并发数（**不**因并发写而永久 500）
    let listed = store.list().expect("list must still work after concurrent writes");
    assert_eq!(listed.len(), N, "list() must return all {N} persons, got {}", listed.len());
    // 每个建档返回的 id 都必须能在读面找到（没有丢失、没有半条）
    let ids: std::collections::HashSet<&str> = listed.iter().map(|p| p.id.as_str()).collect();
    for p in &created {
        assert!(ids.contains(p.id.as_str()), "created person {} missing from list()", p.id);
    }
}

/// 判据③-a（正例）：**末行不完整**（末尾无 `\n`）⇒ 读侧跳过它并照常返回其余部分
///   ⇒ 「有并发写在途」**不等于**「真相层坏了」。
///
/// 这条是**反向**断言：若实现改成「读侧一遇坏行就整体报错」⇒ 这条会红（门有判别力）。
#[test]
fn torn_tail_line_is_skipped_not_fatal() {
    let home = tmp_home("torn-tail");
    let store = ehr_api::PersonStore::new(home.clone());
    store
        .create(&ehr_api::PersonNew {
            name: "完整行".into(),
            birthday: None,
            gender: None,
            id_note: None,
        })
        .unwrap();
    // 手工追加一个**没有结尾换行**的半行（模拟「写了一半」的并发写在途）
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(registry_file(&home))
        .unwrap();
    use std::io::Write;
    f.write_all(br#"{"op":"create","ts":"2026-01-01T00:00:00Z","person_id":"aa"#).unwrap();
    drop(f);

    let listed = store.list().expect("a torn TAIL must not break the read surface");
    assert_eq!(listed.len(), 1, "the complete line must still be read");
    assert_eq!(listed[0].name, "完整行");
}

/// 判据③-b（反例）：**完整行**解析失败 ⇒ **必须报错**（真坏了，绝不掩盖）。
///
/// 这条是**正向**断言：若实现「跳过任何坏行」⇒ 真相层被写坏却装作没事 ⇒ 这条会红。
#[test]
fn corrupt_complete_line_is_fatal() {
    let home = tmp_home("corrupt-line");
    let store = ehr_api::PersonStore::new(home.clone());
    store
        .create(&ehr_api::PersonNew {
            name: "好行".into(),
            birthday: None,
            gender: None,
            id_note: None,
        })
        .unwrap();
    // 在**中间**插一条完整但内容坏掉的行（**有结尾换行** ⇒ 不是「在途半行」）
    let raw = raw_registry(&home);
    std::fs::write(
        registry_file(&home),
        format!("{}{}\n", raw, r#"{"op":"create","ts":BROKEN"person_id":"x""#),
    )
    .unwrap();

    let err = store.list().expect_err("a corrupt COMPLETE line must be fatal (not silently skipped)");
    let msg = format!("{err:#}");
    assert!(
        msg.contains("corrupt registry line"),
        "error must point at the corrupt line, got: {msg}"
    );
}

/// 判据④：**HTTP 面**并发建档 ⇒ 真相层不坏、读面不 500。
///   （库级判据①②已覆盖内核；这里补一条「真进程 + 真 HTTP」的端到端视角。）
#[tokio::test(flavor = "multi_thread")]
async fn concurrent_http_creates_keep_read_surface_alive() {
    const N: usize = 32;
    let home = tmp_home("http-conc");
    let st = ehr_api::AppState::from_home(&home);
    let app = ehr_api::router(st);

    let mut tasks = Vec::new();
    for i in 0..N {
        let app = app.clone();
        tasks.push(tokio::spawn(async move {
            let body = format!(r#"{{"name":"http并发-{i}"}}"#);
            let req = Request::builder()
                .method("POST")
                .uri("/api/v1/persons")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap();
            let res = app.oneshot(req).await.unwrap();
            res.status()
        }));
    }
    for t in tasks {
        let st = t.await.unwrap();
        assert_eq!(st, StatusCode::CREATED, "every concurrent create must return 201");
    }

    // 真相层完整
    let raw = raw_registry(&home);
    let lines: Vec<&str> = raw.lines().filter(|l| !l.trim().is_empty()).collect();
    let broken: Vec<String> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| serde_json::from_str::<serde_json::Value>(l).is_err())
        .map(|(i, l)| format!("line {}: {}", i + 1, l))
        .collect();
    assert!(
        broken.is_empty(),
        "registry must contain only intact JSON lines; {} of {} lines are interleaved/corrupt:\n  {}",
        broken.len(),
        lines.len(),
        broken.join("\n  ")
    );
    assert_eq!(lines.len(), N, "registry must have exactly {N} intact lines");

    // 读面仍可用（**不** 500）
    let req = Request::builder().uri("/api/v1/persons").body(Body::empty()).unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK, "GET /persons must stay 200 after concurrent writes");
}
