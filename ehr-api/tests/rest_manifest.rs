// T16 · D-032 未销账⑩：REST 面的**派生清单**（rest_manifest()）必须是实现的真镜像。
//
// 为什么要有这个门：此前「16 路径 / 18 operation」这个数字**硬编码在 5 处**
// （ehtest.sh / mcp-e2e.sh / container-smoke.sh / tests/mcp.rs / 两份 skill 文档），
// 真源（运行时 OpenAPI）就在同一份代码里 ⇒ 同一事实五份实现。加一条路由要同步改五处，
// 漏一处就红，且红得看不出是「漏改」还是「真坏了」。
//
// 本门把「派生清单」钉死：
//  ① 清单的 paths/operations 数**必须等于**运行时 OpenAPI 实际数（不是另一个手抄快照）；
//  ② paths_list 逐条**等于** OpenAPI 里的 `METHOD path`（名字与集合都对，不只数量）；
//  ③ stubs 数 == CONTRACT 表长度（stub 端点数只有一个来源）；
//  ④ mounted_undocumented 逐条 == MOUNTED_UNDOCUMENTED 表。
use serde_json::Value;

fn openapi() -> Value {
    ehr_api::openapi_json()
}

#[test]
fn rest_manifest_counts_match_runtime_openapi() {
    let man = ehr_api::rest_manifest();
    let doc = openapi();
    let paths = doc["paths"].as_object().expect("paths object");
    let ops: usize = paths
        .values()
        .map(|item| item.as_object().map(|o| o.len()).unwrap_or(0))
        .sum();
    assert_eq!(man["paths"].as_u64().unwrap(), paths.len() as u64, "manifest paths must match openapi");
    assert_eq!(man["operations"].as_u64().unwrap(), ops as u64, "manifest operations must match openapi");
}

#[test]
fn rest_manifest_paths_list_matches_openapi_exactly() {
    let man = ehr_api::rest_manifest();
    let doc = openapi();
    let mut expect: Vec<String> = Vec::new();
    for (p, item) in doc["paths"].as_object().expect("paths") {
        for m in item.as_object().expect("path item").keys() {
            expect.push(format!("{} {}", m.to_uppercase(), p));
        }
    }
    expect.sort();
    let got: Vec<String> = man["paths_list"]
        .as_array()
        .expect("paths_list array")
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(got, expect, "manifest paths_list must equal the openapi operation set");
    // 抽样语义断言（不是快照）：审计与写侧入口必须在清单里
    assert!(got.iter().any(|x| x == "GET /api/v1/audit"), "audit missing from manifest");
    assert!(got.iter().any(|x| x == "POST /api/v1/persons/{pid}/entries/note"), "note entry missing from manifest");
}

#[test]
fn rest_manifest_stub_count_equals_contract_table() {
    let man = ehr_api::rest_manifest();
    assert_eq!(
        man["stubs"].as_u64().unwrap(),
        ehr_api::CONTRACT.len() as u64,
        "stub count must have a single source: the CONTRACT table"
    );
    // D-021：派生清单**同时公布** CONTRACT 表的**逐条清单**（`contract_paths`）
    // ⇒ shell 门禁不必再手写正则去解析 lib.rs 源码数这张表（D-037 当时保留的那半截正则）。
    //
    // ⚠️ 为什么是**集合**而不是计数：本轮先写的是 `contract_len`（= CONTRACT.len()）
    //   与 `stubs`（= OpenAPI 里数 STUB_MARK）两个**计数**并断言相等。
    //   负向实验证明那是**同义反复**：往 CONTRACT 里加一条**合法**条目 ⇒ 两个计数**同步 +1**
    //   ⇒ 断言照样绿 ⇒ 它测不出任何东西。
    //   ⇒ 真正的判据是**两份来源不同的清单逐条相等**：
    //     `contract_paths` 来自 Rust 常量（源码真源）；
    //     `stub_paths`     来自**渲染后的 OpenAPI**（渲染真源，路径是 OpenAPI 合成 → merge_path）。
    //   若 CONTRACT 登记了某端点但没被合进 OpenAPI（或合进去了却没带 STUB_MARK）⇒ 两份清单分叉 ⇒ 红。
    let contract_paths: Vec<String> = man["contract_paths"]
        .as_array()
        .expect("contract_paths array")
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    let stub_paths: Vec<String> = man["stub_paths"]
        .as_array()
        .expect("stub_paths array")
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        contract_paths, stub_paths,
        "CONTRACT 常量的清单必须逐条等于 OpenAPI 渲染出的 stub 清单（两份来源不同的真源）"
    );
    assert_eq!(
        man["stubs"].as_u64().unwrap(),
        stub_paths.len() as u64,
        "stubs count must equal len(stub_paths)"
    );
    // stub 端点在 OpenAPI 里必须带 STUB_MARK（否则「文档不粉饰状态码」这条纪律失守）
    let doc = openapi();
    for (method, path, _tag, _sum) in ehr_api::CONTRACT {
        let op = &doc["paths"][*path][method.to_lowercase().as_str()];
        let summary = op.get("summary").and_then(|s| s.as_str()).unwrap_or("");
        assert!(summary.contains(ehr_api::STUB_MARK), "stub summary missing mark: {summary:?}");
    }
}

/// D-037：派生清单里的 `stub_paths` 必须**逐条等于** CONTRACT 表的 operation 集合。
///
/// 为什么要有这门：此前「某端点是否仍被宣称 501」这个判据**只能**由门禁脚本
/// 手写正则去解析 `lib.rs` 源码（`pub const CONTRACT[^=]*=\s*&\[(.*?)\n\];`）——
/// ① 同一事实两种实现；② **那份正则在 CONTRACT 变空后会跨表匹配到
/// `MOUNTED_UNDOCUMENTED`** ⇒ meds/conds/entries 的「端点不在 CONTRACT 表」门
/// 实际在查**错误的表**（假门：D-009「判据形状」的老问题）。
///
/// 现在判据由实现派生并公布（`stub_paths`），门禁读清单 ⇒ 本门钉死
/// 「派生清单 == 单一事实源」，任何一边漂移立刻红。
#[test]
fn rest_manifest_stub_paths_match_contract_table() {
    let man = ehr_api::rest_manifest();
    let got: Vec<String> = man["stub_paths"]
        .as_array()
        .expect("stub_paths array")
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    let mut expect: Vec<String> = ehr_api::CONTRACT
        .iter()
        .map(|(m, p, _t, _s)| format!("{} {}", m.to_uppercase(), p))
        .collect();
    expect.sort();
    assert_eq!(got, expect, "stub_paths must be the CONTRACT table, item by item");
    // 个数也必须自洽（stubs 是 stub_paths 的长度）——两份字段不许分叉
    assert_eq!(
        man["stubs"].as_u64().unwrap(),
        got.len() as u64,
        "stubs count must equal len(stub_paths)"
    );
    // ⚠️ **反向保险**：stub_paths 里**绝不能**出现「已实装」的端点
    // （否则 OpenAPI 会宣称 501，而实现真返回 200/400/404 —— D-025 纪律失守）。
    for banned in [
        "GET /api/v1/audit",
        "PUT /api/v1/persons/{pid}/documents/{sha}/extraction",
        "GET /api/v1/persons/{pid}/medications",
        "GET /api/v1/persons/{pid}/conditions",
        "GET /api/v1/persons/{pid}/measurements",
    ] {
        assert!(!got.iter().any(|x| x == banned), "implemented endpoint still claimed as stub: {banned}");
    }
}

#[test]
fn rest_manifest_mounted_undocumented_matches_shared_table() {
    let man = ehr_api::rest_manifest();
    let got: Vec<String> = man["mounted_undocumented"]
        .as_array()
        .expect("mounted_undocumented array")
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    let expect: Vec<String> = ehr_api::MOUNTED_UNDOCUMENTED
        .iter()
        .map(|(m, p)| format!("{} {}", m.to_uppercase(), p))
        .collect();
    assert_eq!(got, expect, "mounted-undocumented list must come from the shared table");
}
