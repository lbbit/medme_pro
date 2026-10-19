// person registry —— 本项目唯一新增真相层。
// append-only JSONL（与内核事件日志同哲学）：{EHR_HOME}/registry/persons.jsonl
// 读侧折叠：后写覆盖先写；tombstone(op=delete)从列表剔除。派生 SQLite/FTS 索引可重建，不在本层。
use anyhow::{Context, Result};
use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    Create,
    Update,
    Delete,
}

/// 建档请求体（PATCH/DELETE 端点在阶段2后半接入，复用此信封的 update/tombstone op）
// utoipa 约定：ToSchema derive 须紧随 serde derives 之后
#[derive(Clone, Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct PersonNew {
    pub name: String,
    #[serde(default)]
    pub birthday: Option<String>, // "YYYY-MM-DD"或更粗(如仅年份)——宽松存、严谨在展示与聚合层做
    #[serde(default)]
    pub gender: Option<String>,
    #[serde(default)]
    pub id_note: Option<String>, // 就诊卡号/病历号等外部锚点
}

/// 对外人员视图（折叠后的当前态）。id=UUIDv4 字符串形式（schema 保持纯 string，不依赖可选 feature —— D1）
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
pub struct Person {
    pub id: String,
    pub name: String,
    pub birthday: Option<String>,
    pub gender: Option<String>,
    pub id_note: Option<String>,
    pub created_at: String, // rfc3339 UTC(秒精度)
    pub updated_at: String,
}

/// JSONL 中的一行事件（真相）。person_id 同样为 UUIDv4 字符串。
#[derive(Serialize, Deserialize)]
struct RegistryEvent {
    op: Op,
    ts: String,
    person_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<PersonNew>,
}

fn now_rfc3339() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

pub struct PersonStore {
    file: PathBuf, // {home}/registry/persons.jsonl
}

impl PersonStore {
    pub fn new(home: PathBuf) -> Self {
        Self {
            file: home.join("registry").join("persons.jsonl"),
        }
    }

    fn append(&self, ev: &RegistryEvent) -> Result<()> {
        if let Some(dir) = self.file.parent() {
            std::fs::create_dir_all(dir).context("mkdir registry dir")?;
        }
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.file)
            .with_context(|| format!("open {}", self.file.display()))?;
        serde_json::to_writer(&mut f, ev).context("serialize registry event")?;
        f.write_all(b"\n").context("write newline")?;
        f.flush().context("flush")?; // 低频写，每次同步落盘：真相层宁慢勿丢
        Ok(())
    }

    /// 折叠全量事件 → 当前态列表（按创建序）。MVP 规模假设<千级人·万级事件，直读足够；
    /// 规模化时此处换“派生索引可重建”路径而真相格式不变。
    pub fn list(&self) -> Result<Vec<Person>> {
        struct Cur { p: Person, deleted: bool }
        let contents = match std::fs::read_to_string(&self.file) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => return Err(e).with_context(|| format!("read {}", self.file.display())),
        };
        let mut order: Vec<String> = vec![];
        let mut cur: std::collections::HashMap<String, Cur> = Default::default();
        for (ln, line) in contents.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let ev: RegistryEvent = serde_json::from_str(line).with_context(|| {
                format!("{}:{} corrupt registry line", self.file.display(), ln + 1)
            })?;
            match ev.op {
                Op::Create | Op::Update => {
                    let d = ev.data.context("create/update event missing data")?;
                    // 创建序只在首次见到该 id 时记录（id 本身 UUID，重放/乱序容忍）
                    if !cur.contains_key(&ev.person_id) {
                        order.push(ev.person_id.clone());
                        cur.insert(
                            ev.person_id.clone(),
                            Cur { p: Person {
                                id: ev.person_id.clone(), name: String::new(),
                                birthday: None, gender: None, id_note: None,
                                created_at: ev.ts.clone(), updated_at: ev.ts.clone(),
                            }, deleted: false },
                        );
                    }
                    let c = cur.get_mut(&ev.person_id).expect("just ensured");
                    c.p.name = d.name;
                    if d.birthday.is_some() { c.p.birthday = d.birthday; }
                    if d.gender.is_some() { c.p.gender = d.gender; }
                    if d.id_note.is_some() { c.p.id_note = d.id_note; }
                    c.p.updated_at = ev.ts.clone();
                    // Create/Update落在墓碑id上=“归档撤销”复活语义(臂已收窄为 !=Delete，故恒 false→清墓碑)
                    c.deleted = false;
                }
                Op::Delete => match cur.get_mut(&ev.person_id) {
                    Some(c) => { c.deleted = true; c.p.updated_at = ev.ts.clone(); }
                    None => tracing::warn!(id = %ev.person_id, line = ln + 1, "delete for unknown person (replay-order tolerant skip)"),
                },
            }
        }
        Ok(order.iter().filter_map(|id| cur.get(id)).filter(|c| !c.deleted).map(|c| c.p.clone()).collect())
    }

    pub fn create(&self, req: &PersonNew) -> Result<Person> {
        let id = uuid::Uuid::new_v4().to_string();
        let ts = now_rfc3339();
        let mut data = req.clone();
        data.name = data.name.trim().to_string();
        self.append(&RegistryEvent { op: Op::Create, ts: ts.clone(), person_id: id.clone(), data: Some(data.clone()) })?;
        Ok(Person {
            id, name: data.name, birthday: data.birthday, gender: data.gender, id_note: data.id_note,
            created_at: ts.clone(), updated_at: ts,
        })
    }
}
