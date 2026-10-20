// 写侧·手动条目：self-measurement / note / profile-event（D-028）
//
// 为什么是这三件：上游 `MANUAL-ENTRY-DESIGN.md` 的三类「没有原件的记录」在
// core-model 里**没有新事件变体**——一律走「合成文本当文件过一遍 `Vault::import`」
// 的手法（上游 `apps/mobile_flutter/rust/src/api/vault.rs::add_synthetic_document`
// 与 `vault_profile.rs::vault_profile_record_event` 是同一手法）。本模块照抄
// **同一条**写入路径与**同一批**上游判据（`parser::render_self_measurement_text` /
// `render_profile_event_text` / `validate_self_measured_values`），不另造格式：
// 合成文本里带版本化哨兵行（`###MEDME-SELF-V1###` / `###MEDME-PROFILE-V1###`），
// 上游 `aggregate()` 读得回 ⇒ 写入方与读取方是同一对代码（D2：一个语义只允许一处实现）。
//
// 判据分工（D5：不推断 = 不粉饰）：
//  ① **生理学可能性校验**复用上游 `parser::validate_self_measured_values`
//     （拒绝 138388 mmHg 这类打错的值；收缩压≤舒张压也拒）。这是**兜底**，
//     不是临床判断（上游把「正常/偏高」判给 `home_ref_range`）。
//  ② **不换算单位**：上游写入方恒发规范单位，本层原样落库（原样透传，不替用户改写）。
//  ③ **不做 analyte 白名单**：上游刻意不做（封闭五选一是 UI 层的职责），未知 key
//     落进 `home_ref_range` 的 `_ => None` 分支 ⇒ 裸值显示、不出 flag。
//  ④ **每个字段都能被读回**：写入后立刻可从 `GET /persons/{pid}/documents` 与
//     `labs`（自测值走 `GroupKey::SelfMeasured`，与医院值**永不同组**）看到。
//
// ⚠️ **CAS 去重吞写**的坑（上游 `vault_profile_record_event` 注释里点名过）：`Vault::import`
//   按**内容字节**去重 ⇒ 若两次记录正文逐字相同，第二次会被判成重复并**一条事件都不追加**
//   （上游还回传旧 document id 假装成功）。上游给 profile_event 的正文里带
//   「记录时间:」到毫秒正是为此。本模块对三类条目**一律**在正文里写入带毫秒的
//   记录时间行 ⇒ 每次写入字节必不同 ⇒ 不会静默吞写（D5：以为有、其实没有）。
use anyhow::Result;
use chrono::{DateTime, NaiveDate, SecondsFormat, Utc};
use core_model::{DocType, NewDocument, NewOcr, OcrBackendKind, Vault};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// 写侧错误分两类：**参数不合法**（⇒ 400，带原因）与**内部故障**（⇒ 500）。
/// 分开是 D-022 的纪律：agent 拿到 400 知道「改参数」，拿到 500 知道「重试/告警」。
#[derive(Debug)]
pub enum EntryError {
    BadRequest(String),
    Internal(anyhow::Error),
}

impl From<anyhow::Error> for EntryError {
    fn from(e: anyhow::Error) -> Self {
        EntryError::Internal(e)
    }
}

impl std::fmt::Display for EntryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EntryError::BadRequest(m) => write!(f, "{m}"),
            EntryError::Internal(e) => write!(f, "{e:#}"),
        }
    }
}

// ---------- 请求体 ----------

/// 一个自测值（上游 `parser::SelfMeasuredValue` 的 HTTP 形态）。
/// `analyte_key` 是**规范键**（如 `bp_systolic`），不是显示名；`unit` 原样落库。
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct SelfMeasuredValueIn {
    /// 规范分析物键（如 `bp_systolic`/`bp_diastolic`/`heart_rate`/`body_weight`/
    /// `body_temperature`/`glucose`）。未知键不报错（上游刻意不做白名单），
    /// 但也不出参考区间/异常标志（上游 `home_ref_range` 的 `_ => None` 分支）。
    pub analyte_key: String,
    pub value: f64,
    pub unit: String,
}

/// 自测记录请求体。血压是**两个值共享一条记录**（上游 §5.3：一次测量是最小编辑单元）。
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct SelfMeasurementIn {
    /// 至少一个值；血压请同时给收缩压与舒张压。
    pub values: Vec<SelfMeasuredValueIn>,
    /// 测量时刻（RFC3339）。缺省 = 写入时刻。
    #[serde(default)]
    pub taken_at: Option<String>,
}

/// 纯文本笔记请求体。原文即内容（上游 `add_note`）。
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct NoteIn {
    pub text: String,
    /// 记录时刻（RFC3339）。缺省 = 写入时刻。
    #[serde(default)]
    pub taken_at: Option<String>,
}

/// 病程动作日志请求体（上游 `parser::ProfileEvent`）。
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct ProfileEventIn {
    /// 动作种类（如 `enable`/`disable`/`confirm_dx`/`flare`…）。**原样透传**，
    /// 不认得的 kind 照样落库（上游：将来能读它 ⇒ 「永远可重算」的前提）。
    pub kind: String,
    /// 病种包 id（如 `sle`）。
    pub package: String,
    /// 事件发生日期 `YYYY-MM-DD`（**用户说的那天**，不是记录那天）——形状不对会悄悄排错，
    /// 故在这道边界上就挡住（上游同款校验）。
    pub at: String,
    /// 载荷 JSON **对象**（规则只按键取值；`null`/数组/裸数字对任何规则都只是噪音）。
    #[serde(default)]
    pub payload: serde_json::Value,
}

/// 写侧统一回执（三类条目同形，便于 agent 一套代码处理）。
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct EntryReply {
    /// 新建（或复用）的 document id。
    pub document_id: i64,
    /// 该条目的合成文本在 CAS 里的 sha256 —— **可回链到原件字节**
    /// （`GET /persons/{pid}/documents/{sha}/original`），与上传件同一套寻址。
    pub content_hash: String,
    /// doc_type（`self_measurement`/`note`/`profile_event`）。
    pub doc_type: String,
    /// 标题（时间线展示用）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// 事件/记录时刻（RFC3339 UTC）。
    pub taken_at: String,
    /// true = 内容与既有条目逐字相同（CAS 去重）⇒ **没有新增记录**。
    /// 正常路径恒 false（正文带毫秒记录时间）；true 时必须让调用方知道「没写进去」。
    pub deduped: bool,
}

// ---------- 写入路径（与上游同一条） ----------

/// 「没有原件的文档」的唯一写入路径：合成文本当"文件"过一遍 `vault.import`，
/// 再 `add_document` + `add_ocr`。三类手动条目**全走这一条**（不写三份），
/// 抄自上游 `apps/mobile_flutter/rust/src/api/vault.rs::add_synthetic_document`
/// （那里的注释明确说：抽出来是为了不写第三份同样的五步）。
fn add_synthetic_document(
    vault: &Vault,
    doc_type: DocType,
    when: DateTime<Utc>,
    name: &str,
    title: String,
    language: Option<String>,
    text: String,
) -> Result<(i64, String, bool), EntryError> {
    let imp = vault
        .import(name, "text/plain", text.as_bytes())
        .map_err(|e| EntryError::Internal(anyhow::anyhow!("vault import: {e}")))?;
    let sid = imp.source_file.id;
    let hash = imp.source_file.content_hash.clone();
    // CAS 去重命中：若这份内容已建过档，返回既有 document（**如实标 deduped=true**，
    // 绝不让调用方以为新写了一条——上游在这里也是回传旧 id，但上游的正文带毫秒时间，
    // 正常路径不会走到这里；我们同样保证正文逐次不同）。
    if imp.deduped {
        if let Ok(true) = vault.has_document(sid) {
            if let Ok(Some(d)) = vault.document_by_source_file_id(sid) {
                return Ok((d.id, hash, true));
            }
        }
    }
    let doc = vault
        .add_document(NewDocument {
            source_file_id: sid,
            doc_type,
            doc_date: Some(when),
            doc_date_end: None,
            title: Some(title),
            language,
            page_count: 1,
        })
        .map_err(|e| EntryError::Internal(anyhow::anyhow!("add_document: {e}")))?;
    vault
        .add_ocr(NewOcr {
            document_id: doc.id,
            page_no: 1,
            backend: OcrBackendKind::Native,
            model_version: "ehr-manual-entry".into(),
            text,
            confidence: None,
        })
        .map_err(|e| EntryError::Internal(anyhow::anyhow!("add_ocr: {e}")))?;
    Ok((doc.id, hash, false))
}

/// 解析「记录时刻」：给了就用（RFC3339），没给就取现在。
/// ⚠️ 上游把「没给」解释成**本地墙上时间**再贴 UTC 标签（凌晨记的一条不至于落到昨天）。
/// 服务端跑在容器里通常是 UTC，故这里用 UTC；**如实透传**不做本地化推断。
fn parse_taken_at(taken_at: Option<&str>) -> Result<DateTime<Utc>, EntryError> {
    match taken_at {
        None => Ok(Utc::now()),
        Some(s) => DateTime::parse_from_rfc3339(s.trim())
            .map(|d| d.with_timezone(&Utc))
            .map_err(|e| {
                EntryError::BadRequest(format!("taken_at must be RFC3339 (e.g. 2026-10-23T08:30:00+08:00): {e}"))
            }),
    }
}

/// 毫秒级「记录时间」行：既是给人看的，也保证**每次写入的字节都不同**
/// （CAS 按内容去重 ⇒ 正文逐字相同会静默吞掉一条记录，D5）。
fn record_time_line(when: &DateTime<Utc>) -> String {
    format!("记录时间:{}", when.to_rfc3339_opts(SecondsFormat::Millis, true))
}

/// 规范分析物键的中文标签（上游 `self_measured_label` 的同形实现，上游那份在 app 层、
/// 非公开 API ⇒ 这里按同一份映射重写；**只影响人读文案**，不影响数值/判据）。
fn analyte_label(key: &str) -> &'static str {
    match key {
        "bp_systolic" => "收缩压",
        "bp_diastolic" => "舒张压",
        "heart_rate" => "心率",
        "body_weight" => "体重",
        "body_temperature" => "体温",
        "glucose" => "血糖",
        _ => "记录值",
    }
}

/// 数值显示格式：整数不带小数点（上游 `fmt_value` 同款，避免写成「128.0 mmHg」）。
fn fmt_value(v: f64) -> String {
    if v.fract() == 0.0 && v.is_finite() {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

/// 这批值该给文档起的标题：血压（收缩压/舒张压任一在场）统一叫「血压」，
/// 其余取第一个值的中文标签（上游 `self_measured_title` 同款）。
fn self_measured_title(values: &[parser::SelfMeasuredValue]) -> String {
    let has = |k: &str| values.iter().any(|v| v.analyte_key == k);
    if has("bp_systolic") || has("bp_diastolic") {
        "血压".to_string()
    } else {
        values
            .first()
            .map(|v| analyte_label(&v.analyte_key).to_string())
            .unwrap_or_else(|| "记录".to_string())
    }
}

/// 写一条自测记录（结构化、append-only）。
///
/// 判据全在上游：`parser::validate_self_measured_values`（生理学可能性）、
/// `parser::render_self_measurement_text`（正文 + 版本化载荷行）。
pub fn add_self_measurement(
    vault: &Vault,
    req: &SelfMeasurementIn,
) -> Result<EntryReply, EntryError> {
    if req.values.is_empty() {
        return Err(EntryError::BadRequest("values must not be empty".into()));
    }
    let mut sv: Vec<parser::SelfMeasuredValue> = Vec::with_capacity(req.values.len());
    for v in &req.values {
        let key = v.analyte_key.trim();
        if key.is_empty() {
            return Err(EntryError::BadRequest("analyte_key must not be empty".into()));
        }
        if v.unit.trim().is_empty() {
            return Err(EntryError::BadRequest(
                "unit must not be empty (上游写入方恒发规范单位，本层不代为换算)".into(),
            ));
        }
        if !v.value.is_finite() {
            return Err(EntryError::BadRequest("value must be a finite number".into()));
        }
        sv.push(parser::SelfMeasuredValue {
            analyte_key: key.to_string(),
            value: v.value,
            unit: v.unit.trim().to_string(),
        });
    }
    // 生理学可能性校验（上游唯一的写入前闸）：拒绝打错的值（138388 mmHg）/ 收缩压≤舒张压。
    if let Err(violation) = parser::validate_self_measured_values(&sv) {
        return Err(EntryError::BadRequest(format!(
            "implausible self-measured value: {violation:?}"
        )));
    }
    let when = parse_taken_at(req.taken_at.as_deref())?;
    let mut human_lines: Vec<String> = sv
        .iter()
        .map(|v| format!("{} {} {}", analyte_label(&v.analyte_key), fmt_value(v.value), v.unit))
        .collect();
    human_lines.push(record_time_line(&when));
    let text = parser::render_self_measurement_text(&human_lines, &sv);
    let title = self_measured_title(&sv);
    let name = format!("self-measurement-{}.txt", when.format("%Y%m%dT%H%M%S%.3f"));
    let (document_id, content_hash, deduped) = add_synthetic_document(
        vault,
        DocType::SelfMeasurement,
        when,
        &name,
        title,
        Some("zh".into()),
        text,
    )?;
    Ok(EntryReply {
        document_id,
        content_hash,
        doc_type: "self_measurement".into(),
        title: Some(self_measured_title(&sv)),
        taken_at: when.to_rfc3339(),
        deduped,
    })
}

/// 写一条纯文本笔记。原文即内容：不需要结构化载荷编码（上游 `add_note`）。
pub fn add_note(vault: &Vault, req: &NoteIn) -> Result<EntryReply, EntryError> {
    let text = req.text.trim().to_string();
    if text.is_empty() {
        return Err(EntryError::BadRequest("note text must not be empty".into()));
    }
    let when = parse_taken_at(req.taken_at.as_deref())?;
    // 标题取首行（超长截断到 30 字符——列表页标题不需要整段笔记；上游同款）。
    let title: String = text.lines().next().unwrap_or(&text).chars().take(30).collect();
    // ⚠️ 正文里必须带**随记录时刻变的字节**，否则同内容两条笔记会被 CAS 去重吞掉
    //   （上游 add_note 的正文就是用户原文，天然有这个问题；我们补一行毫秒记录时间）。
    //   注意：补了记录时间行后，「原文」不再是 `ocr_text` 的全部——文档详情里
    //   仍然**逐字包含**用户原文（记录时间行只是多了一行），不是改写。
    let mut body = text.clone();
    body.push('\n');
    body.push_str(&record_time_line(&when));
    let language = parser::detect_language(&text);
    let name = format!("note-{}.txt", when.format("%Y%m%dT%H%M%S%.3f"));
    let (document_id, content_hash, deduped) = add_synthetic_document(
        vault,
        DocType::Note,
        when,
        &name,
        title.clone(),
        language,
        body,
    )?;
    Ok(EntryReply {
        document_id,
        content_hash,
        doc_type: "note".into(),
        title: Some(title),
        taken_at: when.to_rfc3339(),
        deduped,
    })
}

/// kind 的中文说法：**只映射今天真有入口会记的那两种**（上游 `kind_label` 同款纪律：
/// 认不出的原样印 kind 本身，不编）。
fn kind_label(kind: &str) -> &str {
    match kind {
        "enable" => "开启病程档案",
        "disable" => "关闭病程档案",
        other => other,
    }
}

/// 写一条病程动作日志（上游 `vault_profile_record_event`）。
///
/// 判据：① `at` 必须是 `YYYY-MM-DD`（形状不对会悄悄排错 ⇒ 在边界挡住）；
/// ② `payload` 必须是 JSON **对象**（规则只按键取值）。
pub fn add_profile_event(vault: &Vault, req: &ProfileEventIn) -> Result<EntryReply, EntryError> {
    let kind = req.kind.trim();
    let package = req.package.trim();
    if kind.is_empty() {
        return Err(EntryError::BadRequest("kind must not be empty".into()));
    }
    if package.is_empty() {
        return Err(EntryError::BadRequest("package must not be empty".into()));
    }
    if NaiveDate::parse_from_str(req.at.trim(), "%Y-%m-%d").is_err() {
        return Err(EntryError::BadRequest(format!(
            "at must be YYYY-MM-DD (event date, not record date): {}",
            req.at
        )));
    }
    if !req.payload.is_object() {
        return Err(EntryError::BadRequest(
            "payload must be a JSON object (use {} when empty)".into(),
        ));
    }
    let at = req.at.trim().to_string();
    let ev = parser::ProfileEvent {
        kind: kind.to_string(),
        package: package.to_string(),
        at: at.clone(),
        payload: req.payload.clone(),
    };
    let when = parse_taken_at(None)?;
    let human_lines = vec![
        kind_label(&ev.kind).to_string(),
        format!("病种:{}", ev.package),
        format!("日期:{}", ev.at),
        record_time_line(&when),
    ];
    let text = parser::render_profile_event_text(&human_lines, &ev);
    let title = format!("病程档案 · {}", kind_label(&ev.kind));
    let name = format!("profile-event-{}.txt", when.format("%Y%m%dT%H%M%S%.3f"));
    let (document_id, content_hash, deduped) = add_synthetic_document(
        vault,
        DocType::ProfileEvent,
        when,
        &name,
        title.clone(),
        Some("zh".into()),
        text,
    )?;
    Ok(EntryReply {
        document_id,
        content_hash,
        doc_type: "profile_event".into(),
        title: Some(title),
        taken_at: when.to_rfc3339(),
        deduped,
    })
}

/// 打开某人箱子并写一条自测记录（供 handler 与门禁共用同一条路径）。
pub fn write_self_measurement(
    home: &std::path::Path,
    person_id: &str,
    req: &SelfMeasurementIn,
) -> Result<EntryReply, EntryError> {
    let vault = crate::vaults::open_person_vault(home, person_id)
        .map_err(|e| EntryError::Internal(e))?;
    add_self_measurement(&vault, req)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plausible_range_violation_is_rejected_with_reason() {
        let vault = crate::vaults::open_person_vault(
            &std::env::temp_dir().join(format!("ehr-entries-unit-{}", std::process::id())),
            "p",
        )
        .unwrap();
        let req = SelfMeasurementIn {
            values: vec![SelfMeasuredValueIn {
                analyte_key: "bp_systolic".into(),
                value: 138_388.0,
                unit: "mmHg".into(),
            }],
            taken_at: None,
        };
        let err = add_self_measurement(&vault, &req).unwrap_err();
        match err {
            EntryError::BadRequest(m) => assert!(m.contains("implausible"), "应给出具体原因: {m}"),
            other => panic!("应是 400 类错误，实得 {other:?}"),
        }
    }

    #[test]
    fn systolic_not_above_diastolic_is_rejected() {
        let vault = crate::vaults::open_person_vault(
            &std::env::temp_dir().join(format!("ehr-entries-unit2-{}", std::process::id())),
            "p",
        )
        .unwrap();
        let req = SelfMeasurementIn {
            values: vec![
                SelfMeasuredValueIn {
                    analyte_key: "bp_systolic".into(),
                    value: 88.0,
                    unit: "mmHg".into(),
                },
                SelfMeasuredValueIn {
                    analyte_key: "bp_diastolic".into(),
                    value: 138.0,
                    unit: "mmHg".into(),
                },
            ],
            taken_at: None,
        };
        let err = add_self_measurement(&vault, &req).unwrap_err();
        assert!(matches!(err, EntryError::BadRequest(_)), "实得 {err:?}");
    }

    #[test]
    fn profile_event_rejects_non_object_payload_and_bad_date() {
        let vault = crate::vaults::open_person_vault(
            &std::env::temp_dir().join(format!("ehr-entries-unit3-{}", std::process::id())),
            "p",
        )
        .unwrap();
        let bad_payload = ProfileEventIn {
            kind: "enable".into(),
            package: "sle".into(),
            at: "2026-10-23".into(),
            payload: serde_json::json!([1, 2]),
        };
        assert!(matches!(
            add_profile_event(&vault, &bad_payload).unwrap_err(),
            EntryError::BadRequest(_)
        ));
        let bad_date = ProfileEventIn {
            kind: "enable".into(),
            package: "sle".into(),
            at: "2026/10/23".into(),
            payload: serde_json::json!({}),
        };
        assert!(matches!(
            add_profile_event(&vault, &bad_date).unwrap_err(),
            EntryError::BadRequest(_)
        ));
    }

    /// CAS 按内容去重 ⇒ 若两次写入正文逐字相同，第二条会**静默消失**。
    /// 本门禁钉死「正文带毫秒记录时间 ⇒ 每次写入字节不同 ⇒ 不会吞写」。
    #[test]
    fn two_identical_notes_are_not_deduped_into_one() {
        let home = std::env::temp_dir().join(format!("ehr-entries-unit4-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        let vault = crate::vaults::open_person_vault(&home, "p").unwrap();
        let req = NoteIn {
            text: "今天头晕".into(),
            taken_at: None,
        };
        let a = add_note(&vault, &req).unwrap();
        let b = add_note(&vault, &req).unwrap();
        assert_ne!(a.content_hash, b.content_hash, "两次写入的正文必须不同（否则会被 CAS 去重吞掉）");
        assert!(!b.deduped, "第二次写入不得被判成重复");
        let _ = std::fs::remove_dir_all(&home);
    }
}

// ---------- 读侧：measurements 投影（D-028）----------
//
// 为什么读侧也归本模块：`measurements` 是**写侧三类条目的回放投影**
// （D-027 的未销账项②点名的「其数据源尚未有写侧端点」——本轮把写侧补上，
// 读侧才有东西可投影）。投影判据**不另造**：
//   ① `self_measurement` 文档 ⇒ 用**上游** `parser::parse_self_measurement_payload`
//      逐字读回结构化载荷（写入方用它写、读取方用它读 ⇒ 同一对代码，D2）。
//   ② `profile_event` 文档 ⇒ 用**上游** `parser::parse_profile_event_payload`。
//   ③ `note` 文档 ⇒ 原文即内容（`text` 原样透传，**不猜**它是什么）。
//   ④ 读不出载荷 ⇒ **如实标 `payload_readable=false`**，不半猜、不静默吞掉
//      （上游同款纪律：读不出就是没有）。

/// 一条自测值（从合成载荷逐字读回）。
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct MeasurementValueOut {
    /// 规范分析物键（如 `bp_systolic`）
    pub analyte_key: String,
    pub value: f64,
    /// 单位（**原样**，写入方恒发规范单位，本层不换算）
    pub unit: String,
    /// 家庭参考区间（上游 `home_ref_range`：**查不到出处就不给区间**，绝不臆造阈值）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ref_low: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ref_high: Option<f64>,
    /// 区间出处（上游写明的来源文本；随区间一起给，不单独出现）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ref_source: Option<String>,
    /// 相对家庭区间的标志（H/L/N）。**无区间时恒为 None**（不猜「正常」）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flag: Option<String>,
}

/// 一条手动记录（自测值 或 动作事件）。
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct MeasurementOut {
    pub document_id: i64,
    /// 记录类型（`self_measurement`/`note`/`profile_event`）
    pub kind: String,
    /// 记录时刻（RFC3339 UTC）——文档日期
    pub taken_at: String,
    /// 标题（时间线展示用）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// 证据 docref = 合成文本的 sha256（可走 `documents/{sha}/original` 取回字节）
    pub docref: String,
    /// 自测值（`self_measurement` 才有；一条记录可含多个值——血压是两个值）
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<MeasurementValueOut>,
    /// 动作事件字段（`profile_event` 才有）：kind / 病种 / **事件发生的那天** / 载荷
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_package: Option<String>,
    /// 事件发生日期（`YYYY-MM-DD`，**用户说的那天**，不是记录那天）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_payload: Option<serde_json::Value>,
    /// 笔记正文（`note` 才有；原样透传，**不解析**它是什么）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// true = 结构化载荷**读不出来**（哨兵缺失/JSON 坏了/版本不符）。
    /// 此时 `values` 为空、原文仍在 `text` 里 —— **如实标注，不半猜**。
    pub payload_readable: bool,
}

/// measurements 投影的查询参数。
#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
pub struct MeasurementsQuery {
    /// 只看某一类（`self_measurement`/`note`/`profile_event`）。缺省 = 全看。
    #[serde(default)]
    pub kind: Option<String>,
    /// 起始日期（含），`YYYY-MM-DD`。缺省 = 不限。
    #[serde(default)]
    pub from: Option<String>,
    /// 结束日期（含），`YYYY-MM-DD`。缺省 = 不限。
    #[serde(default)]
    pub to: Option<String>,
}

/// measurements 投影的整个回执。
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct MeasurementsReply {
    pub person_id: String,
    /// 投影到的记录数
    pub count: usize,
    pub measurements: Vec<MeasurementOut>,
}

/// 剥掉存储时补在末尾的「记录时间:…」行（note 专用）。
///
/// 只认**末行**且前缀严格是「记录时间:」的那一行 ⇒ 不会误伤用户正文里恰好出现的
/// 同名字样（那只会出现在中间行或非末行）。剥不掉就原样返回（不猜、不裁）。
fn strip_record_time_line(text: &str) -> String {
    let trimmed = text.trim_end_matches('\n');
    if let Some((head, last)) = trimmed.rsplit_once('\n') {
        if last.starts_with("记录时间:") {
            return head.to_string();
        }
    } else if trimmed.starts_with("记录时间:") {
        return String::new();
    }
    trimmed.to_string()
}

/// 把一条 RFC3339/日期串解析成 NaiveDate（只接受 `YYYY-MM-DD`）。
fn parse_day(s: &str) -> Result<NaiveDate, EntryError> {
    NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").map_err(|e| {
        EntryError::BadRequest(format!("date must be YYYY-MM-DD: {s} ({e})"))
    })
}

/// `measurements` 投影：把写侧三类手动条目**回放**成一条条记录。
///
/// 判据全部取自上游（不另造）：载荷用上游 `parse_*_payload` 逐字读回；
/// 区间/标志用上游 `home_ref_range`（**查不到出处就不给区间**）。
/// `kind`/`from`/`to` 三个过滤器**真过滤**（不是「传了没反应」——D-010④同款防御：
/// 非法日期一律 400 并给原因，不静默忽略）。
pub fn measurements_projection(
    vault: &Vault,
    person_id: &str,
    q: &MeasurementsQuery,
) -> Result<MeasurementsReply, EntryError> {
    let kind_filter = q.kind.as_deref().map(|s| s.trim()).filter(|s| !s.is_empty());
    if let Some(k) = kind_filter {
        if !matches!(k, "self_measurement" | "note" | "profile_event") {
            return Err(EntryError::BadRequest(format!(
                "kind must be one of self_measurement|note|profile_event, got {k}"
            )));
        }
    }
    let from = match q.from.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => Some(parse_day(s)?),
        None => None,
    };
    let to = match q.to.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => Some(parse_day(s)?),
        None => None,
    };
    if let (Some(f), Some(t)) = (from, to) {
        if f > t {
            return Err(EntryError::BadRequest(format!(
                "from ({f}) must not be after to ({t})"
            )));
        }
    }

    let mut out: Vec<MeasurementOut> = Vec::new();
    for tl in vault.timeline().map_err(|e| EntryError::Internal(anyhow::anyhow!("{e}")))? {
        let dt = tl.doc_type.as_str();
        if !matches!(dt, "self_measurement" | "note" | "profile_event") {
            continue; // 只投影写侧三类手动条目
        }
        if let Some(k) = kind_filter {
            if dt != k {
                continue;
            }
        }
        let Some(doc) = vault
            .document_by_id(tl.document_id)
            .map_err(|e| EntryError::Internal(anyhow::anyhow!("{e}")))?
        else {
            continue;
        };
        let Some(sf) = vault
            .source_file_by_id(doc.source_file_id)
            .map_err(|e| EntryError::Internal(anyhow::anyhow!("{e}")))?
        else {
            continue;
        };
        // 日期过滤按**记录日**（文档日期）；`profile_event` 的 `at`（事件发生那天）
        // 是另一件事（上游明确区分），不混用。
        let day = tl.doc_date.map(|d| d.date_naive());
        if let Some(f) = from {
            if !day.map(|d| d >= f).unwrap_or(false) {
                continue;
            }
        }
        if let Some(t) = to {
            if !day.map(|d| d <= t).unwrap_or(false) {
                continue;
            }
        }
        let text = vault
            .ocr_text(tl.document_id)
            .map_err(|e| EntryError::Internal(anyhow::anyhow!("ocr_text: {e}")))?;
        let mut item = MeasurementOut {
            document_id: tl.document_id,
            kind: dt.to_string(),
            taken_at: tl
                .doc_date
                .map(|d| d.to_rfc3339())
                .unwrap_or_default(),
            title: tl.title.clone(),
            docref: sf.content_hash.clone(),
            values: Vec::new(),
            event_kind: None,
            event_package: None,
            event_at: None,
            event_payload: None,
            text: None,
            payload_readable: true,
        };
        match dt {
            "self_measurement" => {
                // 上游读回：读不出就是没有（不半猜）。此时原文仍在 text 里。
                match parser::parse_self_measurement_payload(&text) {
                    Some(vs) => {
                        item.values = vs.iter().map(|v| {
                            let range = parser::home_ref_range(&v.analyte_key);
                            let (lo, hi, src) = match &range {
                                Some(r) => (r.low, r.high, Some(r.source.to_string())),
                                None => (None, None, None),
                            };
                            // 标志：**逐字照抄**上游 `build_self_measured_observation` 的判据
                            //（上游源码 aggregate.rs）：超上限 ⇒ H；低于下限 ⇒ L；
                            // 只要**有任一端**区间 ⇒ N；**完全无区间** ⇒ None（不猜「正常」）。
                            // ⚠️ 血压的家庭区间**只有上限**（家庭自测血压正常上限 135/85，
                            // 无下限——上游 home_ref_range 的既定事实），故 128/82 是 N 而非无标志。
                            let flag = if hi.is_some_and(|h| v.value > h) {
                                Some("H".to_string())
                            } else if lo.is_some_and(|l| v.value < l) {
                                Some("L".to_string())
                            } else if lo.is_some() || hi.is_some() {
                                Some("N".to_string())
                            } else {
                                None
                            };
                            MeasurementValueOut {
                                analyte_key: v.analyte_key.clone(),
                                value: v.value,
                                unit: v.unit.clone(),
                                ref_low: lo,
                                ref_high: hi,
                                ref_source: src,
                                flag,
                            }
                        }).collect();
                    }
                    None => {
                        item.payload_readable = false;
                        item.text = Some(text);
                    }
                }
            }
            "profile_event" => match parser::parse_profile_event_payload(&text) {
                Some(ev) => {
                    item.event_kind = Some(ev.kind);
                    item.event_package = Some(ev.package);
                    item.event_at = Some(ev.at);
                    item.event_payload = Some(ev.payload);
                }
                None => {
                    item.payload_readable = false;
                    item.text = Some(text);
                }
            },
            _ => {
                // note：原文即内容，**不解析**它是什么（上游同款纪律）。
                // 存储时我们在用户原文后补了一行「记录时间:」以避免 CAS 去重吞写
                // （见 add_note 注释）——此处**如实剥掉那一行**，让 text 回到**用户原文**。
                item.text = Some(strip_record_time_line(&text));
            }
        }
        out.push(item);
    }
    Ok(MeasurementsReply {
        person_id: person_id.to_string(),
        count: out.len(),
        measurements: out,
    })
}
