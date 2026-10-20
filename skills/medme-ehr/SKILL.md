---
name: medme-ehr
description: "查询与维护 medme-ehr 多人电子健康档案（ehr-api REST）：按人列时间线、检索病历原文、看检验指标趋势、读文档 OCR 文本；也可用建档/传原件写入档案。用于「张三的化验单怎么样」「他最近一次体检有什么异常」「查一下某人某项指标的变化」「把这份化验单存进某人的档案」这类问题。"
metadata:
  qwenpaw:
    emoji: "🗂️"
    requires: {}
---

# medme-ehr 档案工具集（REST 优先 · MCP 可选）

服务端是 `ehr-api` 的 REST API（OpenAPI 自动生成，16 路径 / 18 operation，base = `$EHR_API_BASE/api/v1`）。
**本 skill 只需一个能执行 shell 的 agent**（Copilot CLI / codex / QwenPaw / 手机端脚本），零注册、零守护进程。

## 前置：先定人，再查档

服务端**只认 person_id（UUID），不认姓名**。所以标准起手式永远是：

1. 列出全部在档人：`ehr.sh persons` → 每人一条 `id`/`name`/`birthday`/`gender`/`id_note`
2. 用户说的名字与列表里的 `name` 对不上 ⇒ **先向用户确认是哪一位**，不要瞎猜 id
3. 列表里根本没这个人 ⇒ 明确告诉用户「该服务里没有这个人」，不要换 id 重试

## 命令速查（`ehr.sh` = scripts/ehr.sh，纯 curl，输出 JSON）

| 用户想干什么 | 命令 | 注意 |
|---|---|---|
| 服务活着吗 | `ehr.sh health` | `status:ok` |
| 「有哪些人 / 找一下某人」 | `ehr.sh persons` | 永远的第一步 |
| 「什么时候做过什么检查」 | `ehr.sh timeline PID` | 文档级，`doc_date` 倒序，无日期件排最后 |
| 「报告里提到 X 吗」 | `ehr.sh search PID "白细胞"` | 中文全文检索，命中片段带 `[高亮]` |
| 「白细胞这段时间怎么变的」 | `ehr.sh labs PID` | 跨文档趋势；每点带 H/L 标记与 `docref` 证据 |
| 「他在吃什么药 / 剂量多少」 | `ehr.sh medications PID` | 用药跨度；`latest_dose` 为最近剂量，逐次 `mentions[].docref` 可溯原件 |
| 「他上传了哪些原件」 | `ehr.sh documents PID` | `content_hash` = 原件 sha256 = `docref` |
| 「他自己在家量了血压/血糖？」 | `ehr.sh measurements PID [--kind self_measurement]` | 家庭自测值；**与医院指标永不同组**（`labs` 里 `self_measured=true` 一眼可辨）；区间带 `ref_source` 出处 |
| 「记一条今天头晕」 | `ehr.sh note PID "今天有点头晕"` | 纯文本笔记，原样落库；写后 `measurements --kind note` 可回读 |
| 「记一次病程发作」 | `ehr.sh profile-event PID --kind flare --package sle --at 2026-10-22` | `--at` 是**事件发生那天**（不是记录那天）；`--payload` 可带 JSON |
| 「把这次自测血压存进去」 | `ehr.sh self-measure PID --value bp_systolic=128 --value bp_diastolic=82 --unit mmHg` | 血压给两个 `--value`；生理学不可能的值 ⇒ 服务端 400 |
| 「把这份报告原文给我看」 | `ehr.sh detail PID SHA` | 含 OCR 文本层 / extraction |
| 「下载原件」 | `ehr.sh original PID SHA` | CAS 按 hash 寻址，服务端每次核 sha256 |
| 「给某人建个档」 | `ehr.sh person-create "张三" --gender male --birthday 1990-01-02` | 返回 `id`；建档前先 `persons` 核对同名者 |
| 「把这张化验单存进档案」 | `ehr.sh upload PID 文件.txt` | 仅 .txt/.pdf；重传同字节 `deduped=true` |

完整字段契约见 `references/rest-contracts.md`（真源 = 运行时 `GET /openapi.json`）。

## 组合套路

- **「总结某人所有异常」**：`labs` 看 `flag`，再对异常项用 `search` 找原文佐证，报告里带 `docref`。
- **「最近一次体检说了什么」**：`timeline` 取第一条 → `detail` 读全文。
- **「跟去年比变化」**：用 `labs` 同 `unit_canonical` 的 series 逐点对比，别自己拿两份 text 做差值。
- **「这个药是哪儿开的」**：`medications` 拿 `mentions[].docref` → `detail PID SHA` 读原件全文 → 必要时 `original` 下字节核对。
- **「先建档再存化验单」**：`person-create` 拿 id → `upload` → `search` 回查确认写进去了。

## 硬规矩（来自服务端证据链，别绕过）

1. **透传不猜**：`text` / `extraction` 是原件层内容，原样引用（**两者都可能整个键不存在**——该文档没有文本层或未抽取，不是报错；`extraction.parseable=false` 时内容在 `raw` 里）。数值以 `labs` 的结构化 point 为准，**不要从 OCR 文本里抠数字当结论**。
2. **可溯源**：给结论时带上证据来源（`docref` / 文档日期），否则用户无法复核。
3. **检索零结果 ≠ 没病**：`search` 命中 0 时先换短词再试（`白细胞` 而非 `白细胞计数`），再下「查无此记录」结论。
4. **不是医疗器械**：本服务只做资料整理与检索，不给诊断结论、不建议用药。
5. **错误即指引**：400 参数非法（改参数重试）/ 401 缺或错 token（换 `EHR_API_TOKEN`）/ 403 只读 token 想写（换读写 token）/ 404 查无此人（换人或问用户）/ 501 该类型或该路径未实装（换 .txt/.pdf，或改用已实装路径）/ 500 内部错误（告知用户稍后再试）。别重试打转。
6. **写侧边界**：`upload` 只吃 .txt/.pdf；原件永远进 CAS 永不丢，但 PDF 解析失败时只能存原件、建不了检索文本层（返回 `stored_raw_only=true`）。

## 零维护要点（升级不改配置）

- API 地址来自环境变量 `EHR_API_BASE`（默认 `http://127.0.0.1:8585`），**skill 内不写死地址**。
- token 来自 `EHR_API_TOKEN`（T4 已落地：服务端会校验 `Authorization: Bearer`）。**skill 内不写死 token**。
  - 服务端未配置 token（open 模式）时，不带 header 也能用。
  - 收到 **401** = 缺/错 token ⇒ 换 `EHR_API_TOKEN`（**不要**重试打转）。
  - 收到 **403** = token 是只读(ro) ⇒ 需要读写 token 才能建档/上传/写侧 MCP 工具。
- 本 skill 目录随仓库版本走：`skills/medme-ehr/`（SKILL.md + references/ + scripts/），复制即用。

## 可选：MCP 壳

服务端另有 `POST /mcp`（JSON-RPC 2.0，8 个工具：读侧 6 + 写侧 2），供只吃 MCP 的宿主使用。
契约见 `references/tool-contracts.md`。**宿主不支持 MCP 时直接用上面的 REST 命令即可，两者同源。**

REST 命令 ↔ MCP 工具一一对应（同一真相层，任选其一）：

| REST（`ehr.sh`） | MCP 工具 | 读/写 |
|---|---|---|
| `persons` | `list_persons` | 读 |
| `timeline PID` | `get_timeline` | 读 |
| `search PID Q` | `search_records` | 读 |
| `labs PID` | `get_labs` | 读 |
| `medications PID` | *（暂无 MCP 工具，仅 REST）* | 读 |
| `documents PID` | `list_documents` | 读 |
| `detail PID SHA` | `get_document_detail` | 读 |
| `person-create NAME` | `create_person` | 写 |
| `upload PID FILE` | `upload_document` | 写 |

MCP 错误码：`-32602` 参数非法 / `-32601` 未知工具 / `-32001` 查无此人 / `-32002` 内部错误 / `-32003` 文件类型不支持。
