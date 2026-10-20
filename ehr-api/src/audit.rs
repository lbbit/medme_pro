// medme-ehr · 审计视图（`GET /api/v1/audit`）——D-030
//
// 为什么是「审计视图」而不是又一份状态查询：本服务有两层真相层，各自带自己的
// 事件流——**每人 vault 的内核事件日志**（上游 MedMe 语义）与 **person registry 的
// append-only JSONL**（本项目新增的唯一主数据真相表）。审计要回答的是
// 「**发生过什么**」，而不是「现在是什么」⇒ 不能用 `Person`（折叠后的当前态），
// 也不能只读 vault（会漏掉建档/改档/归档）。
//
// ⚠️ **口径分层，不假装只有一层**（D5：不说=粉饰）：
//   - `source="vault"` 的条目来自**上游 `Vault::audit_log()`**——上游自己的投影
//     （导入/导出/分享/删除四类；DocumentAdded/OcrAdded/ExtractionAdded 是内部
//     派生写入，上游刻意跳过）。**本层不另写一份判据**：能复用的判据一律复用上游，
//     上游说「不展示」的事件我们也不展示（否则两份口径会漂移成「审计里多出/少了
//     某些动作」这种说不清的差异）。
//   - `source="registry"` 的条目来自 **registry 原始事件**（create/update/delete），
//     逐行读出、**不折叠**——折叠会把 update 历史抹平、把 delete 墓碑藏掉。
//
// 一个诚实的边界（写进文档，不粉饰）：上游 `audit_log()` **不展示**手动条目的
// 合成导入（`DocumentAdded`/`OcrAdded` 被跳过），所以「写了一条自测/笔记」在
// `source="vault"` 里**看不到行**，只能在该人的 timeline/documents 里看到。
// 本层**如实**说明这一点，而不是造一条「写入」事件来补齐（那是编造）。
//
// 过滤语义（参数真进判据，不是摆设）：
//   - `person=<person_id>`：只留该人的条目（vault 侧按 vault 目录归人，registry 侧按事件字段）。
//   - `op=<op>`：按动作名过滤，大小写不敏感。vault 侧用上游中文动作名
//     （`导入`/`导出`/`分享`/`删除`），registry 侧用 `create`/`update`/`delete`。
//     两种命名并存是**如实**的（上游给的是中文动作名，我们不擅自改写它）——
//     故文档里两种都列出。
use crate::persons::PersonStore;
use crate::vaults;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// 审计条目（两层真相层合并后的统一视图）。
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AuditEntryOut {
    /// 真相层来源：`registry`（本项目的人员主数据）/ `vault`（上游内核事件日志）。
    pub source: String,
    /// 动作名。registry 侧 = `create`/`update`/`delete`；vault 侧 = 上游中文动作名
    /// （`导入`/`导出`/`分享`/`删除`）——**原样透传，不归一**（归一会让上游口径失真）。
    pub op: String,
    /// 事件时刻（RFC3339 UTC）。
    pub ts: String,
    /// 归属的人（person_id）。vault 侧由「哪个人的 vault」推出。
    pub person_id: String,
    /// 概要（文件名 / 动作细节 / 变更后的主数据）。**可选键**（registry 的 delete 墓碑无 data）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// 内容 sha256（可核验、防篡改）。**可选键**（registry 事件没有内容哈希）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

/// 审计查询参数。
#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
pub struct AuditQuery {
    /// 只看某个人（person_id）。缺省 = 全部人。
    #[serde(default)]
    pub person: Option<String>,
    /// 只看某个动作（大小写不敏感）。registry 侧 `create`/`update`/`delete`；
    /// vault 侧上游中文动作名 `导入`/`导出`/`分享`/`删除`。
    #[serde(default)]
    pub op: Option<String>,
}

/// 审计回执。
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AuditReply {
    /// 参与审计的人数（registry 侧有 vault 的人 ∪ registry 里出现过的人）。
    pub persons_scanned: usize,
    /// 命中的条目数。
    pub count: usize,
    /// 命中的审计条目（**新est first**，与上游 `audit_log()` 的展示口径一致）。
    pub entries: Vec<AuditEntryOut>,
}

/// 组装审计视图。
///
/// 判据分工：
///   - **vault 侧**：直接调用上游 `Vault::audit_log()`（上游自己的投影），
///     **不另写一份**「哪些事件算审计条目」的判据（D2：一个语义只允许一处实现）。
///   - **registry 侧**：`PersonStore::events()` 逐行读原始事件，**不折叠**。
pub fn audit_view(home: &std::path::Path, q: &AuditQuery) -> Result<AuditReply> {
    let store = PersonStore::new(home.to_path_buf());
    let reg_events = store.events()?;

    // op 过滤：大小写不敏感；空串 = 不过滤（与 measurements 的 kind 过滤同款「先 trim」纪律）。
    let op_filter = q.op.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(|s| s.to_lowercase());
    let op_wanted = |op: &str| match &op_filter {
        None => true,
        Some(f) => op.to_lowercase() == *f,
    };

    let mut entries: Vec<AuditEntryOut> = Vec::new();

    // ---- registry 侧（人员主数据真相层）----
    for ev in &reg_events {
        if !op_wanted(&ev.op) {
            continue;
        }
        let detail = ev.data.as_ref().map(|d| {
            // 摘要人读信息：姓名 + 可选主数据；**不猜**未提供的字段。
            let mut s = d.name.clone();
            if let Some(b) = &d.birthday {
                s.push_str(&format!(" · 生日 {b}"));
            }
            if let Some(g) = &d.gender {
                s.push_str(&format!(" · 性别 {g}"));
            }
            if let Some(n) = &d.id_note {
                s.push_str(&format!(" · 外部锚点 {n}"));
            }
            s
        });
        entries.push(AuditEntryOut {
            source: "registry".into(),
            op: ev.op.clone(),
            ts: ev.ts.clone(),
            person_id: ev.person_id.clone(),
            detail,
            sha256: None,
        });
    }

    // ---- vault 侧（上游内核事件日志真相层）----
    // 逐人打开 vault 读上游审计投影。人员集合 = registry 出现过的人 ∪ 已有 vault 目录的人
    // （后者可能已归档但目录仍在——归档≠删除真相层，审计仍应看得见）。
    let mut person_ids: Vec<String> = Vec::new();
    for ev in &reg_events {
        if !person_ids.contains(&ev.person_id) {
            person_ids.push(ev.person_id.clone());
        }
    }
    let vaults_dir = home.join("vaults");
    if let Ok(rd) = std::fs::read_dir(&vaults_dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue; // 锁文件等
            }
            if !person_ids.contains(&name) {
                person_ids.push(name);
            }
        }
    }
    for pid in &person_ids {
        let dir = vaults_dir.join(pid);
        if !dir.is_dir() {
            continue;
        }
        let vault = match vaults::open_person_vault(home, pid) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(person = %pid, "audit: open vault failed: {e:#}");
                continue;
            }
        };
        let log = match vault.audit_log() {
            Ok(l) => l,
            Err(e) => {
                tracing::warn!(person = %pid, "audit: read audit_log failed: {e:#}");
                continue;
            }
        };
        for a in log {
            if !op_wanted(&a.action) {
                continue;
            }
            entries.push(AuditEntryOut {
                source: "vault".into(),
                op: a.action.clone(),
                ts: a.timestamp.clone(),
                person_id: pid.clone(),
                detail: Some(a.detail.clone()),
                sha256: a.sha256.clone(),
            });
        }
    }

    // person 过滤
    if let Some(p) = q.person.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        entries.retain(|e| e.person_id == p);
    }

    // newest first（与上游展示口径一致；同一时刻按 source/op 稳定排序 ⇒ 结果可复现）
    entries.sort_by(|a, b| b.ts.cmp(&a.ts).then(a.person_id.cmp(&b.person_id)).then(a.op.cmp(&b.op)));

    Ok(AuditReply {
        persons_scanned: person_ids.len(),
        count: entries.len(),
        entries,
    })
}
