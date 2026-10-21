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
    // stub 端点在 OpenAPI 里必须带 STUB_MARK（否则「文档不粉饰状态码」这条纪律失守）
    let doc = openapi();
    for (method, path, _tag, _sum) in ehr_api::CONTRACT {
        let op = &doc["paths"][*path][method.to_lowercase().as_str()];
        let summary = op.get("summary").and_then(|s| s.as_str()).unwrap_or("");
        assert!(summary.contains(ehr_api::STUB_MARK), "stub summary missing mark: {summary:?}");
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
