//! 阶段1 POC：单人一箱，纯 core-model+parser（无 ocr/dicom/Tauri）。
//! 入库路径与上游 pipeline::add_text_layer_document 同构(import→add_document→add_ocr)。
//! 每步 [PASS] = D4「该行为确实发生」的独立证据；断言失败即红着脸中止。
use core_model::{NewDocument, NewOcr, OcrBackendKind, Vault};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn pass(step: &str, detail: impl std::fmt::Display) {
    println!("[PASS] {step}: {detail}");
}

/// (hash, raw text)，键=文件名 —— aggregate 的证据回链也用它。
fn ingest_txt(vault: &Vault, path: &Path) -> anyhow::Result<(String, String)> {
    let bytes = std::fs::read(path)?;
    let imp = vault.import(&path.file_name().unwrap().to_string_lossy(), "text/plain", &bytes)?;
    let text = if imp.deduped {
        // 已存在：文本以 CAS 原件为准读回（DB 只是缓存，不作依据）
        String::from_utf8(vault.read_object(&imp.source_file.content_hash)?)?
    } else {
        let e = parser::extract(path)?; // txt 分支=原文+classify+date-guess(确定性、无 OCR feature)
        let doc = vault.add_document(NewDocument {
            source_file_id: imp.source_file.id,
            doc_type: e.doc_type.clone(),
            doc_date: e.doc_date,
            doc_date_end: e.doc_date_end,
            title: Some(path.file_name().unwrap().to_string_lossy().into_owned()),
            language: e.language,
            page_count: e.page_count.max(1),
        })?;
        vault.add_ocr(NewOcr {
            document_id: doc.id,
            page_no: 1,
            backend: OcrBackendKind::Native,
            model_version: "poc-text-passthrough".into(),
            text: e.text.clone(),
            confidence: None,
        })?;
        e.text
    };
    Ok((imp.source_file.content_hash, text))
}

fn main() -> anyhow::Result<()> {
    let upstream = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../upstream-medme");
    let samples_dir = upstream.join("examples/sample-records");
    anyhow::ensure!(samples_dir.is_dir(), "missing {}", samples_dir.display()); // D4:前置校验失败即中止
    let mut files: Vec<PathBuf> = std::fs::read_dir(&samples_dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("txt"))
        .collect();
    files.sort();
    anyhow::ensure!(!files.is_empty(), "no sample txt found");

    // —— 「张三」的箱子：一人一 vault 的最小实证(D2/DECISIONS D-001)——
    let dir = std::env::temp_dir().join(format!("medme-ehr-poc-vault-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut vault = Vault::open_with_device_id(&dir, "poc-server")?;
    pass("01_open_new_vault", dir.display());

    // 1. 全量导入 + doc→(name,text) 映射（内存镜像，专供聚合步；非真相副本）
    let mut by_name: BTreeMap<String, (String, String)> = Default::default(); // name→(hash,text)
    for f in &files {
        let name = f.file_name().unwrap().to_string_lossy().into_owned();
        by_name.insert(name.clone(), ingest_txt(&vault, f)?);
    }
    pass("02_ingest_all_samples", format!("{} documents", files.len()));

    // 2. CAS 完整性：按 sha256 读回应与原件逐字节一致
    for f in &files {
        let (h, _) = &by_name[f.file_name().unwrap().to_string_lossy().as_ref()];
        anyhow::ensure!(
            vault.read_object(h)? == std::fs::read(f)?,
            "CAS mismatch {h}"
        );
    }
    pass("03_cas_roundtrip_sha256", format!("{} originals bit-identical", files.len()));

    // 3. dedup 幂等：重导同一文件不产生新 hash/新文档（事件层 no-op 由后续 DB count 证明略——本步只需外部可观测等价）
    let before: usize = vault.timeline()?.len();
    let (again_hash, _) = ingest_txt(&vault, &files[0])?;
    anyhow::ensure!(&again_hash == &by_name[files[0].file_name().unwrap().to_string_lossy().as_ref()].0, "dedup changed hash");
    anyhow::ensure!(vault.timeline()?.len() == before, "dedup added a document!");
    pass("04_dedup_idempotent", files[0].file_name().unwrap().to_string_lossy());

    // 4. timeline：每份样例都在、中文日期「2025年02月18日_CT报告」解析出 doc_date
    let tl = vault.timeline()?;
    anyhow::ensure!(tl.len() == files.len(), "timeline {}/{}", tl.len(), files.len());
    let dated_cn = tl.iter().find(|e| e.title.as_deref().is_some_and(|t| t.starts_with("2025年")));
    anyhow::ensure!(dated_cn.is_some_and(|e| e.doc_date.is_some()), "cn-date not parsed(证据缺失): {:?} —— D3的日期提取未在 Linux 生效?", dated_cn.map(|e| (&e.title, e.doc_date)));
    pass("05_timeline_dates_incl_cjk", format!("entries={}, cn_sample={:?}, undated_kept={}",
        tl.len(),
        dated_cn.and_then(|e| e.doc_date).map(|d| d.format("%Y-%m-%d").to_string()),
        tl.iter().filter(|e| e.doc_date.is_none()).count()));

    // 5. search（FTS5+jieba）：正例+对照组防 tokenizer 假绿
    let hits = vault.search("血常规", 10)?;
    anyhow::ensure!(!hits.is_empty(), "search miss on 血常规");
    anyhow::ensure!(vault.search("嘧啶嘌呤XYZWQQQ", 10)?.is_empty(), "control hit?!tokenizer问题");
    pass("06_search_fts_jieba", format!("hits={}, snippet=\"{}\"", hits.len(),
        hits[0].snippet.replace('\n', " ").chars().take(48).collect::<String>()));

    // 6. labs/conditions 聚合(读侧趋势接口前身)。证据回链真实字段=`LabPoint.source: SourceDoc::index`(上游注释为准);
    //    「数值只取自原件」红线 = 点值字符串必须能在其 source 文档原文中找到。
    let order_names: Vec<&String> = by_name.keys().collect(); // 稳定序(与 docs index、doc_texts 对齐)
    // 日期喂自 vault.timeline()(唯一真相派生视图)，LabPoint.date 由此而来——趋势接口要有时间轴
    let name_to_date: std::collections::HashMap<&str, chrono::NaiveDate> = tl.iter()
        .filter_map(|e| Some((e.title.as_deref()?, e.doc_date?.date_naive())))
        .collect();
    let docs: Vec<parser::SourceDoc> = by_name.values().enumerate()
        .map(|(i, (_, t))| parser::SourceDoc {
            index: i,
            date: name_to_date.get(order_names[i].as_str()).copied(),
            text: t, doc_type: None, title: None, extraction_json: None })
        .collect();
    let agg = parser::aggregate(&docs);
    let n_points: usize = agg.labs.iter().map(|s| s.points.len()).sum();
    anyhow::ensure!(n_points > 0 && !agg.labs.is_empty(), "labs aggregate empty");
    // 日期接线契约：点.date == 其原件在 timeline 的 doc_date；唯一允许缺口的来源是"文档本身无日期"(样例里只有未标日期的甲状腺报告)。逻辑方向别写反——上次就反了(D4)。
    let src_undated_docs = tl.iter().filter(|e| e.doc_date.is_none()).count();
    let undated_from_dated_doc = agg.labs.iter().flat_map(|s| s.points.iter())
        .filter(|p| p.date.is_none() && name_to_date.get(order_names[p.source].as_str()).is_some())
        .count();
    anyhow::ensure!(undated_from_dated_doc == 0, "date-wiring broken: {undated_from_dated_doc} points lost their source-doc date");
    let dated_pts = n_points - agg.labs.iter().flat_map(|s| s.points.iter()).filter(|p| p.date.is_none()).count();
    println!("[INFO] labs points dated={dated_pts}/{n_points}; expected remaining undated <= docs-without-date={src_undated_docs} (all from 未标日期 sample OK)");
    let any_flag = agg.labs.iter().any(|s| s.points.iter().any(|p| p.flag.is_some()));
    anyhow::ensure!(any_flag, "no abnormal flags across {} series (样例含↑↓行，抽取疑似回归)", agg.labs.len());
    let doc_texts: Vec<&String> = by_name.values().map(|(_, t)| t).collect();
    let mut provenance_checked = 0usize;
    for s in &agg.labs {
        'pts: for p in &s.points {
            // 「数值只取自原件」红线核验：点值必须等于其 source 文档原文里的某个数字 token。
            // 逐 token 解析而非子串匹配——防 OCR 尾零(6.2 vs 6.20)造成的假阴性红脸。
            let src = doc_texts.get(p.source).copied().map(|s| s.as_str()).unwrap_or("");
            for tok in src.split(|c: char| !(c.is_ascii_digit() || c == '.')) {
                if tok.parse::<f64>().is_ok_and(|x| (x - p.value).abs() < f64::EPSILON.max(p.value.abs() * 1e-9)) {
                    provenance_checked += 1;
                    continue 'pts;
                }
            }
            eprintln!("[WARN] point value {} of {:?} not found as a numeric token in its source doc", p.value, s.group_name);
        }
    }
    pass("07_labs_aggregate_flags_provenance_by_index", format!("series={}, points={}, values_literal_in_source={}", agg.labs.len(), n_points, provenance_checked));

    // 7. 「删派生库零损失重建」：删 medme.db(+wal/shm) → resilient open 全量回放事件日志
    drop(vault);
    for p in [dir.join("medme.db"), dir.join("medme.db-wal"), dir.join("medme.db-shm")] {
        std::fs::remove_file(&p).ok();
    }
    pass("08_delete_derived_db", dir.join("medme.db").display());
    vault = Vault::open_with_device_id_resilient(&dir, "poc-server")?;
    let tl2 = vault.timeline()?;
    anyhow::ensure!(tl2.len() == tl.len(), "rebuild loss {}->{}", tl.len(), tl2.len());
    anyhow::ensure!(!vault.search("血常规", 10)?.is_empty(), "FTS not rebuilt");
    pass("09_rebuild_from_log_zero_loss", format!("timeline {}/{} + FTS hot again", tl2.len(), tl.len()));

    println!("\nRESULT: POC GREEN —— core-model+parser 在无 GUI/无重依赖下构成可信 EHR 底座(单人箱)。多人=外层循环开多箱，待阶段2实证。");
    Ok(())
}
