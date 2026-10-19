# MedMe 上游勘察报告（阶段 0，2026-10-19）

源码：`upstream-medme/` @ main（tarball codeload 快照）。除注明"待验证"外均可复现。

## 1. 结论速览（对 PLAN 假设逐条销账）

| # | 假设 | 结果 |
|---|------|------|
| 1 | crates 不依赖 Tauri，可作纯库给服务端用 | ✅ **证实**。workspace members = packages/{core-model,parser,profile,deid,terminology,pipeline,ocr,dicom,sync} + apps/cli + src-tauri；Tauri 只在 src-tauri。core-model 仅 rusqlite(bundled)/serde/jieba-rs/sha2/hmac/chrono；pipeline 只 link core-model/parser/ocr/dicom |
| 2 | vault=普通目录 → 一人一 vault 实现多人隔离 | ✅ **证实**。`Vault::open(root)`；事件/CAS/db 全在 root 下。且 `Event` 枚举**没有 subject/person 字段**——单人假设是刻意的(D3)，加 person 维度进内核不如外层分 vault |
| 3 | services/api 可否复用 | ❌ **不可复用但定位清楚了**：它是上游自己云端的「账号+加密同步」Flask 骨架(阿里云 FC/OSS/PG)，“服务端只见密文”，根本不解析健康数据。我们的 ehr-api 与它正交；其 JWT access/refresh 双 token 模式值得照抄 |
| 4 | 事件 schema 有无预留 patient 概念 | ❌ 无（见 #2，按设计如此） |
| 5 | FTS5+jieba 中文检索可复用 | ✅ `query.rs: Vault::search(query,limit)`、`tokenize.rs`(jieba)、`text.rs`；读 API 直接可用 |
| 6 | OCR pipeline 服务端可行性 | ⚠️ Linux 走 PP-OCRv5 ONNX(`ocr` crate default feature onnx?) —— **待编译验证**依赖重量(ONNX Runtime)。缓解：ingest 接口先支持"仅存原件+文本直录"，OCR 作为可选异步 worker |

## 2. 可直接复用的 API 面（我们后端的核心增值来自聚合它们）

```rust
// 写
pipeline::ingest(&vault, path) -> IngestOutcome{New|Backfilled|Deduped|StoredNoText|InstanceAttached}
Vault::import(...)+parser 各入口(手动条目: SelfMeasurement / Note / ProfileEvent——零新 Event 变体)
// 读
Vault::timeline() / Vault::search(q, limit) / audit_log()
parser::{labs(化验趋势), meds(用药), conditions(病程/疾病档案), aggregate(就诊聚合), handoff, self_entry}
profile::materialize(...) // 患者画像 PatientProfile
imaging/DICOM(slices by study_uid)、sync_io、share(E2E 分享导出)
```

CLI(medme-cli)只是这三个动作的薄壳(~102 行) → **方案 B(HTTP 调 CLI)不必要**。

## 3. 值得借力的上游资产

- `docs/003_Core_Data_Model.md`、`009_Encounter_Model.md`、`015_Trends_and_Timelines.md`：数据模型权威文档，我们的查询接口语义直接继承。
- ADR-0010「云端 LLM 抽取」已有 `ExtractionAdded` 事件（结果 JSON→CAS 引用哈希，可重跑覆盖）——AI agent 写入结构化发现时有现成机制：**agent 结论也应走这条"引用而非替代原件"的路**。
- profile/deid/terminology crates：病程档案、去标识化、术语表 —— 多人场景 deid 尤其有用（如做研究导出）。
- `examples/sample-records/`：POC 语料现成。
- 红线(README/CONTRIBUTING)：结构化抽取必须确定性、数值只取自原件——API 设计中要给"来源锚点"(document_ref/hash) everywhere。

## 4. 单人假设的改造成本评估

需要新增的都是**外层**事情，内核几乎零改动：
1. person registry（我们自己的 SQLite 表或独立 vault-of-vaults 清单）
2. 跨人聚合读层（逐 vault 查 + 汇聚；人数=家庭规模个位数~十位级，无需过度设计）
3. API 鉴权与 role（本人/家属/admin）
4. ingest 串行化：每 vault 的 SQLite 派生库是单写者 → **同一人的并发写需服务端按 person 排队**（待 POC 验证 WAL/busy timeout 行为）

## 5. 环境与部署事实

- 本机无 Rust 工具链（阶段 0 编译证明未完成，装机即可解除，非架构问题）。
- rust-toolchain.toml 固定 stable；rusqlite bundled→ Docker 镜像可免系统 sqlite。
- `ocr`(ONNX PP-OCRv5)/`dicom`(OpenJPEG/CharLS C++)是重量件：Dockerfile 要带 cmake+clang；亦可 MVP 先不开 ocr feature。
- services/api、claim-signer 均为阿里云 FC「自定义运行时」形态（Flask），与我们 docker-compose 诉求无关。

## 6. 风险登记

| 风险 | 等级 | 缓解 |
|---|---|---|
| AGPL：对外服务须开源衍生品 | 已知约束 | 全线 AGPL 运营(DECISIONS D-003) |
| 上游 fast-moving(928 commits)，本地 fork 漂移 | 中 | 我们的 crate 放独立目录(`server/`)只 path-depend upstream packages/*，不改上游文件；改进走 PR |
| OCR ONNX 在服务端体积/GPU-less 性能 | 中 | MVP 文本/PDF 直入优先；OCR worker 独立容器按 feature gate |
| vault SQLite 并发写模型未验证 | 低 | POC 压测 per-person 串行队列兜底 |
| Rust 编译时间(Docker 首次全量~十几分钟) | 低 | 多阶段构建 + cargo layer cache |
