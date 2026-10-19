// per-person vault 挂载层（D-002 一人一箱）。
// 约定死路径：{EHR_HOME}/vaults/{person_uuid}/ —— pid 一律先对 registry 折叠列表校验存在且未删，
// 因此目录名恒为已建档 UUID（防路径注入；UUID 由服务端生成，客户端无从夹带 ../）。
// device_id 固定 "ehr-api"：服务端即唯一写入设备(P2)。Vault 短开会话(MVP)：SQLite open 含幂等
// migration+FTS warm，千级 QPS 前不做连接池(D3/YAGNI)，resilient 变体=缓存库损坏自动重建(POC步骤09实证)。
use anyhow::{Context, Result};
// D2：只用 core_model 顶层 re-export 的稳定接口名；此层只见 Vault，内核条目类型由 From 推断(D5:不留无用import)
use core_model::Vault;
use serde::Serialize;

pub const SERVER_DEVICE_ID: &str = "ehr-api";

// ---- 「每人写锁」（进程内）：「每人串行」的 MVP 形态(D2封装:worker队列化后调用方无感知) ----
// vault SQLite 单写者、HTTP 面天然并发 → 改某人 vault 的入口必须先持此锁。
// std Mutex 无 force_unlock(parking_lot才有) ⇒ "长期持有+任意点释放"只能让 guard 驻留 TLS 建模。
// 本设计是三轮编译期D4实撞逼出来的(每条对应曾经的错误方案，勿退回旧形):
//   ❌函数返回MutexGuard'static —— E0515: Arc clone自map再drop(map)，guard借用悬垂slot；
//   ❌`raw.lock()...;`裸表达式 —— must-use告警揪出"guard当场Drop=从未持锁"(静默失效,最险);
//   ❌&'static锚 + mem::forget手工配平 —— E0597栈Arc生命期不足，且leak/forget配对=又一处"得记得"(D6判据违例)。
// ✔现方案:HeldSlot **拥有** slot(Arc'static拷贝作抵押)+guard，成对进出TLS——深度计数与OS锁状态永不脱节由类型担保。
// 代价(D5登记):同一pid跨线程重复acquire会死锁(owner-only重入设计)。MVP成立:block_in_place临界区短且全程同步；
// worker ingest到来时以队列为唯一写入路径消除该场景(Token接口不变)。槽位永久驻留不回收(pid<千级,YAGNI)。
// poison语义(D4查证std文档):poisoned后首个lock()报Err但逻辑上仍取得锁；into_inner取guard的那次持有，其后的
// drop解锁**不清毒**(只有成功lock的guard才清)→本实现接受"毒随前任panic残留至下一次真Ok lock才被任一后来者清除";
// vault侧本就事件日志+CAS可自愈(POC步骤09)，此处不加force-clear复杂度(D7)。
type Slot = std::sync::Arc<std::sync::Mutex<()>>; // 双层内层视图
struct PersonSlotTable(std::sync::Mutex<std::collections::HashMap<String, std::sync::Arc<Slot>>>);
static PERSON_SLOTS: std::sync::LazyLock<PersonSlotTable> =
    std::sync::LazyLock::new(|| PersonSlotTable(Default::default()));

/// TLS在场项：slot=表外多挂的一份外层强引用(&'static经Box::leak获得——E0597的正解是"**拥有并负责归还**"而非借用栈值)；
/// guard借它的Mutex。字段声明序⇒Drop先guard后slot ⇒ borrow不会悬垂('a推导自&*self.slot,同帧销毁安全,D4手推过)。
struct HeldSlot { slot: &'static std::sync::Arc<Slot>, guard: Option<std::sync::MutexGuard<'static, ()>> }

thread_local! {
    /// (owner重入深度, 在场栈)。depth 0⇒OS锁在手；仅0↔1穿越真锁，嵌套acquire零开销(无push/pop系统调用)。
    static LOCK_STATE: std::cell::RefCell<(usize, Vec<HeldSlot>)> = const { std::cell::RefCell::new((0, Vec::new())) };
}

pub struct LockToken; // RAII存在即持有；Drop释放(handler多条return分支忘解锁不可能化 —— D4-b当年service参数事故的同类形态机制化)

impl LockToken {
    pub fn acquire(person_id: &str) -> LockToken {
        let new_slot = || {
            let mut map = PERSON_SLOTS.0.lock().unwrap_or_else(|p| p.into_inner());
            let s: std::sync::Arc<Slot> = map.entry(person_id.to_string()).or_default().clone();
            drop(map); // 先还全局表锁再拿槽位锁（获取顺序仅此一路径→无环）
            s
        };
        LOCK_STATE.with(|st| {
            let mut b = st.borrow_mut();
            if b.0 == 0 {
                let leaked: &'static std::sync::Arc<Slot> = Box::leak(Box::new(new_slot()));
                // guard借'static解引用(derive自leaked本身)→与slot字段无借用关系，Vec重排/移动皆安全:
                let g = leaked.lock().unwrap_or_else(|p| p.into_inner());
                b.1.push(HeldSlot { slot: leaked, guard: Some(g) });
                b.0 = 1;
            } else if b.0 < usize::MAX {
                b.0 += 1;
            } else { panic!("person lock depth overflow"); }
        });
        LockToken
    }
}

impl Drop for LockToken {
    fn drop(&mut self) {
        debug_assert!(LOCK_STATE.try_with(|st| st.borrow().0 > 0).unwrap_or(false),
            "LockToken dropped off owner thread(depth==0 ⇒ cross-thread misuse, E0515同族编程错误)");
        LOCK_STATE.with(|st| {
            let mut b = st.borrow_mut();
            debug_assert!(b.0 >= 1, "lock token release underflow on this thread");
            b.0 -= 1;
            if b.0 == 0 {
                if let Some(mut hs) = b.1.pop() {
                    drop(hs.guard.take());      // guard先亡=真unlock(poison时std顺路不清毒——语义见顶部注)
                    // from_raw 只收裸指针且实型是双层 Arc(Slot自身已是Arc<Mutex>)——E0308两次实锤后由编译器纠出。
// 归还leak的抵押品:此刻guard已弃，无借用存活(D4手推；唯一unsafe点在此登记)。
                    unsafe { drop(Box::from_raw(hs.slot as *const std::sync::Arc<Slot> as *mut std::sync::Arc<Slot>)) }; // 归还leak的抵押品:此刻guard已弃，无借用存活(D4手推；唯一unsafe点在此登记)
                }
            }
        });
    }
}

/// 打开某人的箱子。pid 必须已在 registry（调用方保证）；不存在目录则新建空箱(vault 自举 schema)。
pub fn open_person_vault(home: &std::path::Path, person_id: &str) -> Result<Vault> {
    let dir = home.join("vaults").join(person_id);
    std::fs::create_dir_all(&dir).with_context(|| format!("mkdir {}", dir.display()))?;
    Vault::open_with_device_id_resilient(&dir, SERVER_DEVICE_ID)
        .map_err(|e| anyhow::anyhow!("vault open {}: {e}", dir.display()))
}

// ---- 读侧 DTO（上游内核类型不挂 serde 展示注解 —— D2：投影层在我们这，别改上游）----
// 条目类型走 core_model 顶层 re-export(公开API面)，仅本模块私有——D5:不留对外泄漏的无用import。
use core_model::{SearchHit, TimelineEntry};

#[derive(Serialize, utoipa::ToSchema)]
pub struct TimelineItem {
    pub document_id: i64,
    pub doc_date: Option<String>, // rfc3339 UTC
    pub doc_date_end: Option<String>,
    pub doc_type: String,
    pub title: Option<String>,
}

impl From<TimelineEntry> for TimelineItem {
    fn from(e: TimelineEntry) -> Self {
        Self {
            document_id: e.document_id,
            doc_date: e.doc_date.map(|d| d.to_rfc3339()),
            doc_date_end: e.doc_date_end.map(|d| d.to_rfc3339()),
            doc_type: e.doc_type.as_str().to_string(),
            title: e.title,
        }
    }
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct SearchItem {
    pub document_id: i64,
    pub title: Option<String>,
    /// FTS5 snippet：[高亮]标记为内核原生产出(jieba预分词+unicode61 中文路径 POC步骤06实证)
    pub snippet: String,
}

impl From<SearchHit> for SearchItem {
    fn from(h: SearchHit) -> Self {
        Self { document_id: h.document_id, title: h.title, snippet: h.snippet }
    }
}

// ---- 写侧：multipart 上传 ingest（POC import_demo::ingest_txt 的 HTTP 版；调用方必须持 person_write_lock）----
use core_model::{NewDocument, NewOcr, OcrBackendKind};

#[derive(Serialize, utoipa::ToSchema)]
pub struct UploadReply {
    pub content_hash: String,     // sha256 CAS —— docref(API-DESIGN:对外id一律内容哈希)
    pub deduped: bool,            // true=原件早已在箱内(幂等)。document_id可能None(stored_raw件)，其自愈路径见has_document分支注释。
    #[serde(default)]
    pub document_id: Option<i64>, // None ⇒ stored_raw_only(PDF解析失败，待worker补建索引行)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doc_type: Option<String>,
    #[serde(default)]
    pub text_indexed: bool,       // OCR文本层已建(FTS可查前提)
    #[serde(default)]
    pub stored_raw_only: bool,    // ⚠️原件已在CAS永不丢(D5诚实旗标)——原因逐条进 messages
    #[serde(default)]
    pub messages: Vec<String>,    // 非致命告警通道(与上游 pipeline ingest_wiki「errors收集」同哲学)
}

fn raw_reply(content_hash: String) -> UploadReply {
    UploadReply { content_hash, deduped: false, document_id: None, doc_type: None,
                  text_indexed: false, stored_raw_only: false, messages: vec![] }
}

pub fn ingest_upload(vault: &Vault, filename: &str, bytes: &[u8]) -> Result<UploadReply> {
    use std::io::Write as _;
    let ext = std::path::Path::new(filename)
        .extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    anyhow::ensure!(matches!(ext.as_str(), "txt" | "pdf"),
        "unsupported extension \".{ext}\": needs dicom/ocr worker (stage-3+)");
    let mime = if ext == "pdf" { "application/pdf" } else { "text/plain" };
    let imp = vault.import(filename, mime, bytes)?; // sha256 CAS 登记，永不改原件

    // dedup + has_document：自愈语义 —— raw-only件(PDF失败)或历史缺索引件重传同字节即补建Document+文本层；
    // 已完整索引的dedup=零改动直接返回(幂等护栏 POC步骤03同源)。⚠️不猜storage_path(D4),见 stage。
    let mut reply = raw_reply(imp.source_file.content_hash.clone());
    reply.deduped = imp.deduped;
    if !imp.deduped || !vault.has_document(imp.source_file.id)? {
        let tmp_dir = staging_dir(&imp.source_file.content_hash);
        std::fs::create_dir_all(&tmp_dir).context("mkdir staging")?;
        let staged = tmp_dir.join(format!("{}.{}", imp.source_file.content_hash, ext));
        let r = (|staged: &std::path::Path| -> Result<Option<parser::Extracted>> {
            match parser::extract(staged) { Ok(e) => Ok(Some(e)), Err(_) => Ok(None) } // extract内部catch_unwind pdf panic→Err(D5兜底实证于上游源码注释)
        })({ std::fs::File::create(&staged)?.write_all(bytes)?; &staged });
        let _ = std::fs::remove_dir_all(&tmp_dir); // 暂存即用即弃(CAS才是真相)
        match r? {
            None => { reply.stored_raw_only = true; reply.messages.push(
                "pdf-extract failed/panicked on this PDF: original stored in CAS forever, awaiting ocr worker to index".into()); return Ok(reply); }
            Some(e) => {
                let doc = vault.add_document(NewDocument {
                    source_file_id: imp.source_file.id,
                    doc_type: e.doc_type.clone(),
                    doc_date: e.doc_date,
                    doc_date_end: e.doc_date_end,
                    title: Some(filename.to_string()),
                    language: e.language,
                    page_count: e.page_count.max(1),
                })?;
                vault.add_ocr(NewOcr {
                    document_id: doc.id, page_no: 1, backend: OcrBackendKind::Native,
                    model_version: "ehr-http-ingest".into(), text: e.text.clone(), confidence: None,
                })?;
                reply.document_id = Some(doc.id);
                reply.doc_type = Some(doc.doc_type.as_str().to_string());
                reply.text_indexed = true;
            }
        }
    }
    Ok(reply)
}

fn staging_dir(hash: &str) -> std::path::PathBuf {
    // hash命名=天然去重+并发安全；放在系统temp(可弃置)，EHR_HOME属运维区不放垃圾(D3分层)。
    std::env::temp_dir().join(format!("medme-ehr-staging-{hash}"))
}

// ---- sha256 docref → 单文档详情 / CAS 原件流（API-DESIGN「documents/{sha256}」两行的落地）----
/// hash 形态校验(与内核 is_object_hash 同语义：恰 64 位小写 hex)。
/// 对外一律先过这道闸再进任何路径拼接——D4:不猜 storage_path，也不让畸形串摸到 fs。
pub fn is_sha(s: &str) -> bool {
    // ⚠️别加回 is_ascii_lowercase():它对 ASCII **数字返回 false**(std语义="是ASCII字母且小写")——
    //   实撞过:合法sha(含0-9)被自家400闸全灭。digit∪a..=f 已完备表达"64位小写hex"(D5教训在此留疤)。
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// timeline 扫描定位 (document, source_file)。**已知代价(D5)**：只读公开接口、无 hash→id 反查，
/// O(n)/请求 —— MVP 规模成立；QPS 上去后加派生索引(docs/PLAN「派生可重建」纪律覆盖此演进)，真相层不变。
/// 已删文档不在 timeline ⇒ 天然不可见(Raw Never Dies，但投影已撤)。
pub fn resolve_doc_ref(vault: &Vault, sha: &str) -> Result<Option<(core_model::Document, core_model::SourceFile)>> {
    for tl in vault.timeline()? {
        let Some(doc) = vault.document_by_id(tl.document_id)? else { continue };
        let Some(sf) = vault.source_file_by_id(doc.source_file_id)? else { continue };
        if sf.content_hash == sha {
            return Ok(Some((doc, sf)));
        }
    }
    Ok(None)
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct TextLayerMeta {
    pub pages_with_text: Vec<i32>, // ocr_page_numbers：缺哪些页由此对照 page_count 判断
    pub model_version: Option<String>, // "text-layer"/"ppocr-v5"…(内核口径,第一页那条)
    pub backend: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_confidence: Option<f32>, // 内核取各页最小值(最保守)；native 文本层恒 None
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct DocumentDetail {
    // source file(CAS 真相锚点)
    pub content_hash: String,
    pub original_name: String,
    pub mime_type: String,
    pub byte_size: i64,
    pub imported_at: String,
    // document(投影行)
    pub document_id: i64,
    pub doc_type: String,
    pub doc_date: Option<String>,
    pub doc_date_end: Option<String>,
    pub title: Option<String>,
    pub language: Option<String>,
    pub page_count: i32,
    pub encounter_id: Option<i64>,
    // API-DESIGN:{sha} = document + OCR文本层 + extraction JSON。
    // text/extraction 一律**透传不猜**(D5)：解析失败降级 raw+旗标，绝不静默吞内容。
    pub text_indexed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    pub text_truncated_bytes: usize, // >0 ⇒ text 被响应体护栏截断(D7；全文走 /original)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ocr_meta: Option<TextLayerMeta>,
    /// parseable=true ⇒ json 为内核 result_json 的解析体；false ⇒ raw 原样留字符串(MVP agent 自己判)。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extraction: Option<ExtractionPass>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct ExtractionPass {
    pub parseable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub json: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<String>,
}

const TEXT_CAP: usize = 64 * 1024; // 响应体护栏：超长正文截到此字节数(UTF-8 安全边界)

fn cap_text(s: String) -> (String, usize) {
    if s.len() <= TEXT_CAP {
        return (s, 0);
    }
    let mut cut = TEXT_CAP;
    while cut > 0 && !s.is_char_boundary(cut) {
        cut -= 1; // D4手推:len≠char界，中文多字节——按字节切会裂字
    }
    let tr = s.len() - cut;
    (s[..cut].to_string(), tr)
}

pub fn document_detail(vault: &Vault, sha: &str) -> Result<Option<DocumentDetail>> {
    let Some((doc, sf)) = resolve_doc_ref(vault, sha)? else { return Ok(None) };
    let pages = vault.ocr_page_numbers(doc.id)?;
    let text_indexed = !pages.is_empty();
    let (text, truncated) = if text_indexed {
        let t = vault.ocr_text(doc.id).map_err(|e| anyhow::anyhow!("ocr_text: {e}"))?;
        let (t, tr) = cap_text(t);
        (Some(t), tr)
    } else {
        (None, 0)
    };
    let ocr_meta = text_indexed.then(|| -> Result<TextLayerMeta> {
        Ok(TextLayerMeta {
            pages_with_text: pages.clone(),
            model_version: vault.ocr_model_version(doc.id).map_err(|e| anyhow::anyhow!("{e}"))?,
            backend: vault.ocr_backend(doc.id).map_err(|e| anyhow::anyhow!("{e}"))?,
            min_confidence: vault.ocr_confidence(doc.id).map_err(|e| anyhow::anyhow!("{e}"))?,
        })
    }).transpose()?;
    // result_json 引用不替代(D2红线)：可解析则透传结构体，不可解析降级 raw ——都如实交付。
    let extraction = match vault.extraction_json(doc.id)? {
        None => None,
        Some(s) => Some(match serde_json::from_str::<serde_json::Value>(&s) {
            Ok(v) => ExtractionPass { parseable: true, json: Some(v), raw: None },
            Err(_) => ExtractionPass { parseable: false, json: None, raw: Some(s) },
        }),
    };
    Ok(Some(DocumentDetail {
        content_hash: sf.content_hash.clone(),
        original_name: sf.original_name.clone(),
        mime_type: sf.mime_type.clone(),
        byte_size: sf.byte_size,
        imported_at: sf.imported_at.to_rfc3339(),
        document_id: doc.id,
        doc_type: doc.doc_type.as_str().to_string(),
        doc_date: doc.doc_date.map(|d| d.to_rfc3339()),
        doc_date_end: doc.doc_date_end.map(|d| d.to_rfc3339()),
        title: doc.title.clone(),
        language: doc.language.clone(),
        page_count: doc.page_count,
        encounter_id: doc.encounter_id,
        text_indexed,
        text,
        text_truncated_bytes: truncated,
        ocr_meta,
        extraction,
    }))
}

/// CAS 读取的三态出口 —— handler 按态映射 HTTP 码，不在此猜。
pub enum CasOutcome {
    Bytes(Vec<u8>),
    MalformedHash,               // →400(形态闸在前，理论不可达；留作 belt)
    NotFound,                    // →404：raw 从未同步进箱/已被投影删除后清理? (MVP:CAS永不删，即"没有这个对象")
    Integrity(String),           // →500+登记：sha256 复核不过=数据被污染(D5红线，绝不静默回字节)
    Other(anyhow::Error),
}

pub fn cas_read(vault: &Vault, sha: &str) -> CasOutcome {
    if !is_sha(sha) {
        return CasOutcome::MalformedHash;
    }
    match vault.read_object(sha) {
        Ok(b) => CasOutcome::Bytes(b),
        Err(core_model::MedmeError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => CasOutcome::NotFound,
        Err(core_model::MedmeError::Other(m)) if m.contains("malformed hash") => CasOutcome::MalformedHash,
        Err(core_model::MedmeError::Other(m)) if m.contains("integrity check") => CasOutcome::Integrity(m),
        Err(e) => CasOutcome::Other(anyhow::anyhow!("{e}")),
    }
}

// ---- documents list：timeline行 + source_file_by_id(hash) + ocr_page_numbers(text层判定)，全走内核公开查询面----
#[derive(Serialize, utoipa::ToSchema)]
pub struct DocumentListItem {
    pub document_id: i64,
    pub content_hash: String,   // CAS docref → /documents/{sha}/original 可下载
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_name: Option<String>,
    pub mime_type: String,
    pub byte_size: i64,
    pub doc_date: Option<String>,
    pub doc_date_end: Option<String>,
    pub doc_type: String,
    pub title: Option<String>,
    pub text_indexed: bool,     // false ⇒ raw-only，待 worker(见 upload stored_raw_only 同义)
}

pub fn list_document_items(vault: &Vault) -> Result<Vec<DocumentListItem>> {
    let mut out = Vec::new();
    for tl in vault.timeline()? {
        let doc = match vault.document_by_id(tl.document_id)? { Some(d) => d, None => continue };
        let sf = match vault.source_file_by_id(doc.source_file_id)? { Some(s) => s, None => continue };
        out.push(DocumentListItem {
            document_id: doc.id,
            content_hash: sf.content_hash.clone(),
            original_name: Some(sf.original_name.clone()),
            mime_type: sf.mime_type.clone(),
            byte_size: sf.byte_size,
            doc_date: tl.doc_date.map(|d| d.to_rfc3339()),
            doc_date_end: tl.doc_date_end.map(|d| d.to_rfc3339()),
            doc_type: tl.doc_type.as_str().to_string(),
            title: tl.title.clone(),
            text_indexed: !vault.ocr_page_numbers(doc.id)?.is_empty(),
        });
    }
    Ok(out)
}

// ---- 读侧·聚合域: GET /persons/{pid}/labs —— parser::aggregate() 跨文档全量聚合(D-010后半最后一件) ----
// POC import_demo 步骤07的 HTTP 版。两条红线从 POC 原样继承，勿退化:
// ① 证据回链(「数值只取自原件」): LabPoint.source = SourceDoc.index,本层维护 index→content_hash(CAS docref),
//    消费方拿 sha 走 /documents/{sha}(/original rehash=GATE D5a语义)可一路溯源到字节;数值由上游 text/extraction_json
//    产出,投影层绝不改写(D2)。token级数值核验留在 ehtest GATE-L(e2e现场重验,不把校验逻辑埋进被测面自己)。
// ② labs 全文**不 cap_text**(detail 截断是展示层语义);表格截半页会静默缺点——测量方法先自证(D4)。
// extraction_json 透传=云抽取优先正则的上游原生路径(agent 后续 PUT extraction 后 labs 自动采用,无需改此处)。

#[derive(Serialize, utoipa::ToSchema)]
pub struct LabPointOut {
    /// 日精度 RFC3339 日期(YYYY-MM-DD);来源文档无日期时 None(内核排序保证 None 殿后)
    pub date: Option<String>,
    /// 显示基准值 —— 与所在序列 ref_low/ref_high 同单位(上游不变量,D2 引用不改写)
    pub value: f64,
    pub unit: Option<String>,
    /// 规范单位换算值(跨院比较只许用它;无可换算时 None)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value_canonical: Option<f64>,
    /// "H"/"L" 异常标志(原样透传)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flag: Option<String>,
    /// true=云端抽取且本地校验器未能在原文确认(图表安全标记;正则路径恒 false)
    pub unverified: bool,
    /// 证据 docref=该数值出处原件的 sha256 → GET /documents/{sha} 可查全文并 rehash
    pub docref: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct LabSeriesOut {
    /// 词典归一后的分析物键;未识别项=None 且绝不与已识别项混组(内核分组语义)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub analyte_key: Option<String>,
    pub group_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loinc: Option<String>,
    /// 参考区间(与点同单位;无法保证则成对 None——不给错单位的一对数)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ref_low: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ref_high: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit_canonical: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ref_low_canonical: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ref_high_canonical: Option<f64>,
    /// true=序列混过不同印刷单位已统一换算(纸上找不到这个数字,渲染必须言明)
    pub values_converted: bool,
    pub any_abnormal: bool,
    pub points: Vec<LabPointOut>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct LabsReply {
    pub person_id: String,
    /// 参与聚合的文档数(仅 text 层齐备者;raw-only 无文本可析,MVP 不计入 notes_withheld)
    pub documents_scanned: usize,
    pub series_count: usize,
    pub total_points: usize,
    pub series: Vec<LabSeriesOut>,
}

/// block_in_place 内同步调用。一人一箱语义下 vault 即档案全集，跨报告趋势在此自然成立(D-002)。
pub fn labs_aggregate(vault: &Vault, person_id: &str) -> Result<LabsReply> {
    use chrono::NaiveDate;
    struct Row { sha: String, date: Option<NaiveDate>, text: String, json: Option<String> }
    let mut rows: Vec<Row> = Vec::new();
    for tl in vault.timeline()? {
        if vault.ocr_page_numbers(tl.document_id)?.is_empty() {
            continue; // raw-only：无文本层不可解析(worker 补 OCR 后自动进入聚合)
        }
        let doc = match vault.document_by_id(tl.document_id)? { Some(d) => d, None => continue };
        let sf = match vault.source_file_by_id(doc.source_file_id)? { Some(s) => s, None => continue };
        rows.push(Row {
            sha: sf.content_hash.clone(),
            date: tl.doc_date.map(|d| d.date_naive()),
            text: vault.ocr_text(tl.document_id).map_err(|e| anyhow::anyhow!("ocr_text: {e}"))?,
            json: vault.extraction_json(tl.document_id).map_err(|e| anyhow::anyhow!("{e}"))?,
        });
    }
    let docs: Vec<parser::SourceDoc<'_>> = rows.iter().enumerate()
        .map(|(i, r)| parser::SourceDoc {
            index: i,
            date: r.date,
            text: &r.text,
            doc_type: None,      // aggregate 本身忽略此字段(仅医生摘要路由用,D4查证源码注释)，传 None=如实
            title: None,         //     ——同上，不为「看起来完整」而喂多余数据(D7)
            extraction_json: r.json.as_deref(),
        })
        .collect();
    let agg = parser::aggregate(&docs);
    let series: Vec<LabSeriesOut> = agg.labs.iter().map(|s| LabSeriesOut {
        analyte_key: s.analyte_key.clone(),
        group_name: s.group_name.clone(),
        loinc: s.loinc.clone(),
        ref_low: s.ref_low,
        ref_high: s.ref_high,
        unit_canonical: s.unit_canonical.clone(),
        ref_low_canonical: s.ref_low_canonical,
        ref_high_canonical: s.ref_high_canonical,
        values_converted: s.values_converted,
        any_abnormal: s.any_abnormal,
        points: s.points.iter().map(|p| LabPointOut {
            date: p.date.map(|d| d.format("%Y-%m-%d").to_string()),
            value: p.value,
            unit: p.unit.clone(),
            value_canonical: p.value_canonical,
            flag: p.flag.clone(),
            unverified: p.unverified,
    // source 越界理论上不可达(parser 由本数组生成)；兜底取空串——GATE-L 要求 docref ∈ 已知sha集合，空串必被揪出且**不在服务端 panic**(D4:测量面与故障域分离)
            docref: rows.get(p.source).map(|r| r.sha.clone()).unwrap_or_default(),
        }).collect(),
    }).collect();
    let total_points = series.iter().map(|s| s.points.len()).sum();
    Ok(LabsReply {
        person_id: person_id.to_string(),
        documents_scanned: rows.len(),
        series_count: series.len(),
        total_points,
        series,
    })
}
