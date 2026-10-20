// 目标验收④「派生索引可全量重建」的库级测试（补 D-023 的门禁单测面）。
// 门禁（rebuild-e2e.sh）走真进程 HTTP 面；本文件走库级面，防「门禁只测了服务、没测内核接线」。
use anyhow::Result;

const SAMPLE: &str = "XX医院检验科检验报告单\n姓名:林宝妹  性别:女\n检验项目          结果      参考区间     标志\n白细胞计数        6.2       3.5--9.5     /\n糖化血红蛋白      7.1       4.0--6.0     H\n报告日期:2025年10月15日\n";

fn tmp_home(tag: &str) -> std::path::PathBuf {
    let h = std::env::temp_dir().join(format!("ehr-rb-test-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&h);
    std::fs::create_dir_all(&h).unwrap();
    h
}

#[test]
fn unknown_person_id_is_rejected_and_creates_nothing() {
    let home = tmp_home("unknown");
    let err = ehr_api::rebuild::rebuild_person(&home, "no-such-person").unwrap_err();
    assert!(format!("{err:#}").contains("no such vault"), "{err:#}");
    // 关键：不得凭空建出空箱（否则「参数写错」会变成「凭空多一个人」）
    assert!(!home.join("vaults").join("no-such-person").exists());
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn plan_on_empty_home_is_empty_not_error() {
    let home = tmp_home("empty");
    let plan = ehr_api::rebuild::plan_rebuild(&home).unwrap();
    assert!(plan.is_empty(), "{plan:?}");
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn rebuild_preserves_truth_and_recounts_derived_state() -> Result<()> {
    let home = tmp_home("rebuild");
    let pid = "11111111-2222-4333-8444-555555555555";
    // 造一箱：ingest 一份含中文与数值的化验单（与 ehtest 同族样本）
    let vault = ehr_api::vaults::open_person_vault(&home, pid)?;
    let rep = ehr_api::vaults::ingest_upload(&vault, "血常规报告.txt", SAMPLE.as_bytes())?;
    assert!(!rep.content_hash.is_empty());
    drop(vault);

    // 重建前基线
    let before = ehr_api::rebuild::rebuild_person(&home, pid)?;
    assert!(before.derived_db_removed, "first rebuild should have dropped the derived db");
    assert!(before.timeline_entries >= 1, "{before:?}");
    assert!(before.search_probe_hits >= 1, "FTS must be hot after rebuild: {before:?}");

    // 真相层在：log/ 与 objects/ 不可被重建动
    assert!(home.join("vaults").join(pid).join("log").is_dir());
    assert!(home.join("vaults").join(pid).join("objects").is_dir());

    // 再重建一次：计数必须与首次重建后**逐字相等**（不丢、不多）
    let again = ehr_api::rebuild::rebuild_person(&home, pid)?;
    assert_eq!(again.timeline_entries, before.timeline_entries);
    assert_eq!(again.lab_points, before.lab_points);
    assert_eq!(again.search_probe_hits, before.search_probe_hits);

    let _ = std::fs::remove_dir_all(&home);
    Ok(())
}
