// 派生索引全量重建（目标验收④的证明面：原件(CAS)+事件日志是唯一真相，派生库可全量重建）。
//
// 真相层 = {home}/vaults/{pid}/{objects,log}（CAS + 事件日志，append-only）。
// 派生层 = {home}/vaults/{pid}/medme.db(+wal/shm)（SQLite/FTS 缓存，可随时丢弃重建）。
// 本模块的动作：**删派生层 → 从事件日志全量回放**（内核 resilient open 走同一条路径），
// 重建后当场量出 timeline 条数 / labs 序列与点数，作为「重建后不丢、不多」的证据。
//
// 为什么做成显式命令而不是每次启动自动重建：自动重建会把「缓存坏了」静默吃掉，
// 让人以为数据在——本项目最贵的漏账形态是「以为还在」。显式命令 = 显式动作 + 显式报告。
use anyhow::{Context, Result};
use core_model::Vault;
use serde::Serialize;

/// 单人重建结果（字段即证据：rebuild 前后的计数由调用方比对）
#[derive(Serialize, Clone, Debug)]
pub struct PersonRebuild {
    pub person_id: String,
    pub vault_dir: String,
    /// 本次是否真的删过派生库（false=本来就没有派生库，如空箱）
    pub derived_db_removed: bool,
    pub rebuilt_from_log: bool, // true=删库后从日志回放重建
    pub timeline_entries: usize,
    pub lab_series: usize,
    pub lab_points: usize,
    pub search_probe_hits: usize, // FTS 热重建的探针命中数（中文词）
}

/// 派生库及其 sidecar（内核 resilient open 同样删这三个，D-009 实证过）
fn derived_paths(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    vec![
        dir.join("medme.db"),
        dir.join("medme.db-wal"),
        dir.join("medme.db-shm"),
    ]
}

/// 列出全部在档人的 vault 目录名（目录名 = registry 里的 person UUID）
pub fn list_vault_dirs(home: &std::path::Path) -> Result<Vec<(String, std::path::PathBuf)>> {
    let root = home.join("vaults");
    let mut out = Vec::new();
    let rd = match std::fs::read_dir(&root) {
        Ok(r) => r,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(e).with_context(|| format!("read {}", root.display())),
    };
    for ent in rd {
        let ent = ent.with_context(|| format!("read_dir {}", root.display()))?;
        if !ent.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let name = ent.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        out.push((name, ent.path()));
    }
    out.sort();
    Ok(out)
}

/// 删除该箱的派生库（返回是否真删过）
fn drop_derived(dir: &std::path::Path) -> bool {
    let mut removed = false;
    for p in derived_paths(dir) {
        if std::fs::remove_file(&p).is_ok() {
            removed = true;
        }
    }
    removed
}

/// 重建单人：删派生库 → resilient open（回放日志建库+FTS热重建）→ 当场量计数
pub fn rebuild_person(home: &std::path::Path, person_id: &str) -> Result<PersonRebuild> {
    let dir = home.join("vaults").join(person_id);
    // 未知 person_id 必须报错，**不得**顺手建出一个空箱（否则「参数写错」会变成「凭空多一个人」，
    // 且新建的空箱还会被当成人档列出来——D4「参数传进去≠生效」的反向形态：参数错了却"生效"了）。
    if !dir.is_dir() {
        anyhow::bail!("no such vault for person_id={person_id} ({} not found)", dir.display());
    }
    let removed = drop_derived(&dir);
    // resilient open：若回放后库仍打不开，内核会再删一次并重试；打不开即为真相层不可读 ⇒ 上抛
    let vault: Vault = Vault::open_with_device_id_resilient(&dir, crate::vaults::SERVER_DEVICE_ID)
        .map_err(|e| anyhow::anyhow!("vault open {}: {e}", dir.display()))?;
    let timeline_entries = vault.timeline().map_err(|e| anyhow::anyhow!("timeline: {e}"))?.len();
    // FTS 探针：中文分词路径(jieba+unicode61)下能否命中"白细胞"；命中数即索引热否
    let search_probe_hits = vault
        .search("白细胞", 50)
        .map(|v| v.len())
        .unwrap_or(0);
    let (lab_series, lab_points) = match crate::vaults::labs_aggregate(&vault, person_id) {
        Ok(l) => (l.series_count, l.total_points),
        Err(_) => (0, 0), // labs 聚合失败不应让重建本身失败（真相层已就绪；聚合是读侧派生）
    };
    Ok(PersonRebuild {
        person_id: person_id.to_string(),
        vault_dir: dir.display().to_string(),
        derived_db_removed: removed,
        rebuilt_from_log: removed,
        timeline_entries,
        lab_series,
        lab_points,
        search_probe_hits,
    })
}

/// dry-run 计划（不删任何东西）：列出将被重建的箱与将要删除的派生库文件
#[derive(Serialize, Clone, Debug)]
pub struct RebuildPlanItem {
    pub person_id: String,
    pub vault_dir: String,
    pub derived_files_present: Vec<String>,
    pub truth_present: bool, // log/ 或 objects/ 存在 = 真相层在，可回放
}

pub fn plan_rebuild(home: &std::path::Path) -> Result<Vec<RebuildPlanItem>> {
    let dirs = list_vault_dirs(home)?;
    let mut out = Vec::new();
    for (pid, dir) in dirs {
        let present: Vec<String> = derived_paths(&dir)
            .iter()
            .filter(|p| p.exists())
            .map(|p| p.display().to_string())
            .collect();
        let truth_present = dir.join("log").is_dir() || dir.join("objects").is_dir();
        out.push(RebuildPlanItem {
            person_id: pid,
            vault_dir: dir.display().to_string(),
            derived_files_present: present,
            truth_present,
        });
    }
    Ok(out)
}

/// 重建全部在档人的派生库（逐人独立；单人失败不影响其他人，但错误会被收集上抛）
pub fn rebuild_all(home: &std::path::Path) -> Result<Vec<PersonRebuild>> {
    let dirs = list_vault_dirs(home)?;
    let mut out = Vec::new();
    let mut errs: Vec<String> = Vec::new();
    for (pid, _) in dirs {
        match rebuild_person(home, &pid) {
            Ok(r) => out.push(r),
            Err(e) => errs.push(format!("{pid}: {e:#}")),
        }
    }
    if !errs.is_empty() {
        anyhow::bail!("rebuild failed for {} vault(s): {}", errs.len(), errs.join(" | "));
    }
    Ok(out)
}
