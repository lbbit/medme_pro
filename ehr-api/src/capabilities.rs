// medme-ehr · ehr-api · **能力清单**（ingest 能力 = 单一事实来源）
//
// 为什么有这个模块（D2：一个语义只允许一处实现）：
//   「本构建能吃哪些文件类型、哪种类型走哪条路、哪种能力有没有链接进来」
//   这件事此前散落在**三处**：①`vaults::ingest_upload` 的 if/ensure 判据；
//   ②OpenAPI 里 `501` 的 description 文案；③门禁/文档里手写的「ocr worker 未做」。
//   ⇒ 同一事实三份实现：加一种格式要改三处，漏一处就红，且**红得看不出是漏改还是真坏了**。
//   本模块把「**路由判据**」收到一处（`route_for_ext`），能力清单**从它派生**。
//
// 与 T15/T16 同形（照抄 D-032/D-033 的做法）：**真源 = 实现**，派生机器可读清单
// ⇒ 门禁与文档**从实现取值**，而不是各自硬编码一份快照。
//
// 诚实边界（D5）：**「链接进 OCR 引擎」≠「OCR 一定能出文本」**。
//   - `ocr_available()` 只回答「这个二进制**链**了 PP-OCRv5 ONNX 引擎」。
//   - 模型权重是**运行期**从 ModelScope 下载到 `$OAR_HOME`（默认 `~/.oar`）的
//     ⇒ 首次调用可能因「模型未就绪」而失败 ⇒ 那时**如实报**（`stored_raw_only=true`
//     + messages 说明），**不假装**识别成功。
//   - 引擎的 ONNX Runtime 静态库要求 **glibc ≥ 2.38**（C++23 ABI 符号）⇒ 宿主
//     Ubuntu 22.04（glibc 2.35）**链不过** ⇒ OCR 走可选 feature，默认构建**如实拒**。
use serde::Serialize;

/// 一条 ingest 路由（按扩展名分派到哪条处理路径）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// txt / pdf：走 `parser::extract` 文本层路线（PDF 无文本层 ⇒ 如实 raw-only）。
    TextLayer,
    /// png / jpg / jpeg / tiff / tif：走 `pipeline::ingest` 的图片分支（PP-OCRv5 ONNX）。
    OcrImage,
    /// dcm：DICOM 元数据解析（上游 `dicom::parse_meta`）——**本服务尚未接线**。
    Dicom,
}

impl Route {
    pub fn id(self) -> &'static str {
        match self {
            Route::TextLayer => "text-layer",
            Route::OcrImage => "ocr-image",
            Route::Dicom => "dicom",
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Route::TextLayer => "文本层解析(parser::extract)",
            Route::OcrImage => "图片 OCR(PP-OCRv5 ONNX / pipeline::ingest)",
            Route::Dicom => "DICOM 元数据(dicom::parse_meta)",
        }
    }
}

/// **路由判据的单一事实来源**：扩展名 → 路由。
/// `ingest_upload` 与能力清单**都读它** ⇒ 判据只有一处。
pub fn route_for_ext(ext: &str) -> Route {
    let ext = ext.to_ascii_lowercase();
    match ext.as_str() {
        "txt" | "pdf" => Route::TextLayer,
        "png" | "jpg" | "jpeg" | "tiff" | "tif" => Route::OcrImage,
        "dcm" => Route::Dicom,
        _ => Route::TextLayer, // 未知扩展名 → 走 ensure 拒（见 ingest_upload 的 unsupported 分支）
    }
}

/// 本扩展名是否属于「已知且有明确路由」的类型（未知类型不在能力清单里）。
pub fn is_known_ext(ext: &str) -> bool {
    let ext = ext.to_ascii_lowercase();
    matches!(
        ext.as_str(),
        "txt" | "pdf" | "png" | "jpg" | "jpeg" | "tiff" | "tif" | "dcm"
    )
}

/// 本二进制**是否链接了** OCR 引擎（feature `ocr`）。
///
/// ⚠️ 这**只**说明「链了」，不保证「模型已就绪」——见模块头注释的诚实边界。
pub fn ocr_available() -> bool {
    cfg!(feature = "ocr")
}

/// 本二进制**是否支持**某条路由（= 路由已知 **且** 其依赖能力已链接）。
pub fn route_enabled(route: Route) -> bool {
    match route {
        Route::TextLayer => true,
        Route::OcrImage => ocr_available(),
        // DICOM：上游 `dicom::parse_meta` 是纯 Rust（无 codecs）⇒ 能力上可用，
        // 但本服务的 ingest 尚未接线 ⇒ 仍报「需要 dicom worker」（如实，不假装）。
        Route::Dicom => false,
    }
}

/// 一条 ingest 路由在能力清单里的对外形状（**显式建模**，D-038）。
///
/// 为什么要有这个 struct：此前整份清单是 `json!` 拼出来的 `Value`
/// ⇒ **OpenAPI 里根本没有它的形状**（`GET /capabilities` 的 200 无 body 声明）
/// ⇒ 生成的客户端拿不到类型，只能猜里面有什么。
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct CapabilityRouteOut {
    /// 路由 id（`text-layer`/`ocr-image`/`dicom`）
    pub id: String,
    /// 人类可读名
    pub name: String,
    /// 走这条路由的扩展名
    pub exts: Vec<String>,
    /// 本构建**是否启用**这条路由
    pub enabled: bool,
    /// 接不了的原因（**可执行**：写明怎么开）；enabled=true 时为 null
    pub reject_reason: Option<String>,
}

/// ingest 能力清单（**从 `route_for_ext` 派生**，不是手抄表）。
///
/// 字段说明：
/// - `ocr_available`：本二进制是否链接了 OCR 引擎（feature 派生）。
/// - `ocr_engine`：引擎名（没链上时为 null）。
/// - `routes[]`：每条路由的 `exts`（由判据反查得到）/`enabled`（能力是否就绪）。
/// - `accepted_exts`：**本构建真的能吃**的扩展名（路由已知 ∧ 路由 enabled）。
/// - `rejected_exts`：**已知但本构建接不了**的扩展名（含原因，供调用方与文档引用）。
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct CapabilityManifest {
    /// 本二进制是否**链接了** OCR 引擎（feature `ocr` 派生）。
    /// ⚠️ 只说明「链了」，**不保证模型权重已就绪**（诚实边界见模块头注释）。
    pub ocr_available: bool,
    /// OCR 引擎名（没链接时为 null）
    pub ocr_engine: Option<String>,
    pub routes: Vec<CapabilityRouteOut>,
    /// **本构建真能吃**的扩展名
    pub accepted_exts: Vec<String>,
    /// **已知但本构建接不了**的扩展名（原因在 `routes[].reject_reason` 里）
    pub rejected_exts: Vec<String>,
}

pub fn capability_manifest() -> CapabilityManifest {
    let all_known = [
        "txt", "pdf", "png", "jpg", "jpeg", "tiff", "tif", "dcm",
    ];
    let mut routes: Vec<CapabilityRouteOut> = Vec::new();
    let mut accepted: Vec<String> = Vec::new();
    let mut rejected: Vec<String> = Vec::new();
    for r in [Route::TextLayer, Route::OcrImage, Route::Dicom] {
        let exts: Vec<String> = all_known
            .iter()
            .copied()
            .filter(|e| route_for_ext(e) == r)
            .map(|s| s.to_string())
            .collect();
        let enabled = route_enabled(r);
        for e in &exts {
            if enabled {
                accepted.push(e.clone());
            } else {
                rejected.push(e.clone());
            }
        }
        routes.push(CapabilityRouteOut {
            id: r.id().to_string(),
            name: r.name().to_string(),
            exts,
            enabled,
            reject_reason: if enabled { None } else { Some(reject_reason(r).to_string()) },
        });
    }
    CapabilityManifest {
        ocr_available: ocr_available(),
        ocr_engine: if ocr_available() {
            Some("ppocr-v5(onnx) via pipeline::ingest".to_string())
        } else {
            None
        },
        routes,
        accepted_exts: accepted,
        rejected_exts: rejected,
    }
}

/// 某条路由「已知但本构建接不了」的**如实原因**（写进回执 messages 与清单）。
pub fn reject_reason(route: Route) -> &'static str {
    match route {
        Route::TextLayer => "",
        Route::OcrImage => {
            "this build has no OCR engine (feature `ocr` off; the ONNX Runtime it links \
             requires glibc>=2.38) — 怎么开：cargo build --release --features ocr（需 glibc>=2.38 宿主/容器）"
        }
        Route::Dicom => "needs dicom worker (stage-3+, not wired yet)",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_is_derived_from_the_same_judgement_as_ingest() {
        // 判据与清单同源：清单里的 accepted_exts 逐个必须被 `route_for_ext` 判成 enabled 的路由。
        let man = capability_manifest();
        for e in man.accepted_exts.iter() {
            let r = route_for_ext(e);
            assert!(route_enabled(r), "accepted ext {e} but route {:?} not enabled", r);
        }
        for e in man.rejected_exts.iter() {
            let r = route_for_ext(e);
            assert!(!route_enabled(r), "rejected ext {e} but route {:?} enabled", r);
            assert!(!reject_reason(r).is_empty(), "rejected ext {e} has no reason");
        }
    }

    #[test]
    fn default_build_rejects_images_honestly() {
        // 默认构建（无 ocr feature）必须**如实拒**图片，而不是假装支持。
        // 门禁宿主是 Ubuntu 22.04(glibc 2.35)⇒ 默认构建**永远**是 ocr off。
        if !cfg!(feature = "ocr") {
            assert!(!ocr_available());
            let r = route_for_ext("jpg");
            assert_eq!(r, Route::OcrImage);
            assert!(!route_enabled(r));
            assert!(reject_reason(r).contains("no OCR engine"));
        }
    }

    #[test]
    fn unknown_extension_is_not_in_the_capability_list() {
        // 未知类型不在清单里（清单只列「有明确路由的类型」）⇒ 不宣称支持。
        assert!(!is_known_ext("exe"));
        assert!(is_known_ext("TXT")); // 大小写不敏感
    }
}
