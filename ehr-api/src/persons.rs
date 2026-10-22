// person registry —— 本项目唯一新增真相层。
// append-only JSONL（与内核事件日志同哲学）：{EHR_HOME}/registry/persons.jsonl
// 读侧折叠：后写覆盖先写；tombstone(op=delete)从列表剔除。派生 SQLite/FTS 索引可重建，不在本层。
use anyhow::{Context, Result};
use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

/// **进程内**的 registry 追加锁（全局一把，不是每文件一把）。
///
/// 为什么需要它（T38 实测抓到的真缺陷）：
///   `append()` 原先用 `serde_json::to_writer(&mut f, ev)` 往文件里写 —— 那**不是一次写**，
///   而是**几十次小 write syscall**（键、值、逗号逐段吐）⇒ 两个并发建档请求的这些小写
///   会**交错落盘** ⇒ 真相层 `registry/persons.jsonl` 里出现「半行 + 半行」的交错垃圾。
///   实测：60 个并发 `POST /persons` ⇒ **59 行里 29 行是交错垃圾** ⇒ 此后
///   `GET /persons`、`/timeline`、审计视图等**所有**读面永久 500（真相层被写坏且不可自愈）。
///   ⇒ 这不是「抖动」，是**真相层可被并发写打坏** ⇒ 必须机制化。
///
/// 为什么是「单次 write_all + 锁」两件事（缺一不可）：
///   ① 锁 ⇒ 同一进程内的并发 append 互斥（临界区覆盖「序列化 + 写入」整段）；
///   ② **先序列化成整行 String，再一次性 `write_all`** ⇒ 即使有**跨进程**并发
///      （锁只管本进程），O_APPEND 模式下**一次 write syscall** 的内容也不会与别家交错
///      （内核在同一次写入里把整段字节追加到文件尾）⇒ 行不再破碎。
///   ⇒ 只加锁不改写粒度：锁能防住本进程，但「一把锁只护住一个 write_all」这件事
///      仍需由「整行一次写」保证 ⇒ 两者是**不同**的失效模式，必须都治。
static APPEND_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 取 registry 追加锁。poison 时**取回内部值**继续用（ poisons 只说明「曾有线程在锁里 panic」，
/// 追加临界区里没有不变量会被破坏 ⇒ 不该让一次 panic 把 registry 永久写死）。
fn append_lock() -> std::sync::MutexGuard<'static, ()> {
    APPEND_LOCK.lock().unwrap_or_else(|p| p.into_inner())
}

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

/// registry 里的一条**原始事件**（真相层原样；审计视图用）。
///
/// 与 `Person`（折叠后的当前态）不同：这里**不折叠**，一条 op 就是一行真相。
/// 审计视图要的就是「发生过什么」而不是「现在是什么」⇒ 单独暴露原始事件。
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct RegistryEventOut {
    /// `create`/`update`/`delete`
    pub op: String,
    /// 事件时刻（RFC3339 UTC，秒精度）
    pub ts: String,
    pub person_id: String,
    /// 事件携带的主数据（`delete` 墓碑无 data ⇒ 缺省）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<PersonNew>,
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
        // ① 先把整行序列化好（**不**直接 to_writer 到文件：那会变成几十次小 write ⇒ 并发交错）
        let mut line = serde_json::to_vec(ev).context("serialize registry event")?;
        line.push(b'\n');
        // ② 进程内互斥（覆盖「写」整段）
        let _guard = append_lock();
        // ③ O_APPEND + **一次 write_all** ⇒ 跨进程并发也不会把行写碎
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.file)
            .with_context(|| format!("open {}", self.file.display()))?;
        f.write_all(&line).context("write registry line")?;
        f.flush().context("flush")?; // 低频写，每次同步落盘：真相层宁慢勿丢
        Ok(())
    }

    /// 读出 registry 文件的**完整行**。
    ///
    /// 为什么要有这个函数（T38）：
    ///   写侧现在是「整行一次 write_all」⇒ **行内**不会破碎；但**跨进程**并发时，
    ///   读侧仍可能正好读到「写了一半的最后一行」（末尾没有换行符）。
    ///   ⇒ **「末行不完整」≠「真相层被写坏」** ⇒ 两者必须**分开判**：
    ///     · 末行不完整（无结尾 `\n`）⇒ 说明有并发写在途 ⇒ **跳过该行**（它还没写完）
    ///       并打 warn（可观测，不静默）；
    ///     · **完整行**解析失败 ⇒ 那是真的坏了（历史遗留的交错垃圾 / 磁盘损坏）⇒ **必须红**，
    ///       绝不「跳过坏行」——否则真相层坏了却装作没事（D-050「派生期望值不得为空」同族：
    ///       掩盖比报错更坏）。
    ///   ⇒ 判据落点：**只有「最后一行且不完整」可豁免**，其它任何一行坏 ⇒ 一律报错。
    fn complete_lines<'a>(&self, contents: &'a str) -> Vec<&'a str> {
        let mut out = Vec::new();
        let mut rest = contents;
        while let Some(idx) = rest.find('\n') {
            let (line, tail) = rest.split_at(idx);
            out.push(line);
            rest = &tail[1..];
        }
        // 剩下的尾巴：非空 ⇒ 说明最后一次写还没写完（末尾无 '\n'）⇒ 跳过（并留 warn）
        if !rest.is_empty() {
            tracing::warn!(
                file = %self.file.display(),
                tail_bytes = rest.len(),
                "registry 末行不完整（有并发写在途）⇒ 本次读跳过该行（真相层未受损）"
            );
        }
        out
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
        for (ln, line) in self.complete_lines(&contents).into_iter().enumerate() {
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

    /// 读出 registry 的**全部原始事件**（不折叠），按写入顺序。
    ///
    /// 审计视图用：`list()` 给的是「当前态」，审计要的是「发生过什么」——
    /// 折叠会把 update 的历史抹平、把 delete 的墓碑藏掉，两种都是「以为有、其实没有」。
    /// 真相层是 append-only JSONL ⇒ 逐行读出来就是全部历史，无需额外索引。
    pub fn events(&self) -> Result<Vec<RegistryEventOut>> {
        let contents = match std::fs::read_to_string(&self.file) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => return Err(e).with_context(|| format!("read {}", self.file.display())),
        };
        let mut out = Vec::new();
        for (ln, line) in self.complete_lines(&contents).into_iter().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let ev: RegistryEvent = serde_json::from_str(line).with_context(|| {
                format!("{}:{} corrupt registry line", self.file.display(), ln + 1)
            })?;
            out.push(RegistryEventOut {
                op: match ev.op {
                    Op::Create => "create",
                    Op::Update => "update",
                    Op::Delete => "delete",
                }
                .to_string(),
                ts: ev.ts,
                person_id: ev.person_id,
                data: ev.data,
            });
        }
        Ok(out)
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
