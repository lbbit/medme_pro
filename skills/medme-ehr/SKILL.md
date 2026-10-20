---
name: medme-ehr
description: "查询与维护 medme-ehr 多人电子健康档案（ehr-api）：按人列时间线、检索病历原文、看检验指标趋势、读文档 OCR 文本；也可用建档/传原件写入档案。用于「张三的化验单怎么样」「他最近一次体检有什么异常」「查一下某人某项指标的变化」「把这份化验单存进某人的档案」这类问题。"
metadata:
  qwenpaw:
    emoji: "🗂️"
    requires: {}
---

# medme-ehr 档案工具集（读侧 6 + 写侧 2）

服务端是 `ehr-api` 的 MCP 端点 `POST /mcp`（JSON-RPC 2.0）。以下 8 个工具就是全部能力。

## 前置：person_id 只能来自 list_persons

**除 `create_person` / `list_persons` 外的所有工具都要 `person_id`（UUID），且服务端不接受姓名**。找不到就是 `-32001`（查无此人）。
所以标准起手式永远是：

1. `list_persons` —— 拿到全部在档人的 `person_id` / `name` / `birthday` / `gender` / `id_note`
2. 用户说的名字若与列表里的 `name` 匹配不上：**先向用户确认是哪一位**，不要瞎猜 id
3. 列表里根本没有这个人 ⇒ 明确告诉用户「该服务里没有这个人」，不要换 id 重试

## 何时用哪个工具

| 用户想干什么 | 工具 | 注意 |
|---|---|---|
| 「有哪些人 / 找一下某人」 | `list_persons` | 永远的第一步 |
| 「什么时候做过什么检查」「病历时间线」 | `get_timeline` | 文档级，`doc_date` 倒序，不含指标明细 |
| 「体检报告里有没有提到 X」「XX 是什么情况」 | `search_records` | 中文全文检索（FTS5+jieba），命中片段带 `[高亮]` |
| 「血压/血糖/白细胞这段时间怎么变的」 | `get_labs` | 跨文档趋势 `series`/`points`，每点带 H/L 异常标记与 `docref` 证据 |
| 「他上传了哪些报告原件」 | `list_documents` | `content_hash` = 原件 sha256，就是 `docref` |
| 「把这份报告的原文/抽取结果给我看」 | `get_document_detail` | 按 `sha256` 取 OCR 文本层与抽取结果 |
| 「给某人建个档」 | `create_person` | 返回 `id`（=person_id）；**建档前先 list_persons 核对是否已有同名者** |
| 「把这张化验单存进某人的档案」 | `upload_document` | 原件要 **base64 编码**后传 `content_base64`；只支持 txt/pdf |

## 组合套路

- **「总结某人所有异常」**：`get_labs` 看 flag，再对异常项用 `search_records` 找原文佐证，报告里给出 `docref`。
- **「最近一次体检说了什么」**：`get_timeline` 取第一条 → `get_document_detail` 读全文。
- **「跟去年比有什么变化」**：`get_labs` 的同 `unit_canonical` series 逐点对比，别自己拿两份 text 做差值。
- **「先建档再存这份化验单」**：`create_person` 拿 person_id → `upload_document` 传原件 → `search_records` 回查确认写进去了。

## 硬规矩（来自服务端证据链，别绕过）

1. **透传不猜**：`text` / `extraction` 是原件 OCR 层的内容，原样引用（**两者都可能整个键不存在**——该文档没有文本层或未抽取，不是报错；`extraction.parseable=false` 时内容在 `raw` 里）。数值以 `get_labs` 的结构化 point 为准，不要从 OCR 文本里抠数字当结论。
2. **可溯源**：给用户结论时带上证据来源（docref / 文档日期），否则用户无法复核。
3. **检索零结果 ≠ 没病**：`search_records` 命中 0 时先确认是不是用词不对（如「心肌梗死」vs「心梗」），再下「查无此记录」结论。
4. **不是医疗器械**：本服务只做资料整理与检索，不给诊断结论、不建议用药。
5. **错误码即行为指引**：`-32602` 参数非法（改参数重试，别重试打转）、`-32601` 工具名/方法不存在（别调）、`-32001` 查无此人（换人或问用户）、`-32002` 内部错误（告知用户稍后再试）、`-32003` 文件类型不支持（换 txt/pdf）。
6. **写侧也有边界**：`upload_document` 只吃 txt/pdf、单件 ≤32MB；原件永远进 CAS 永不丢，但 PDF 解析不了时只能存原件、建立不了检索文本层（返回 `stored_raw_only=true`）。

## 契约细节

各工具的完整 inputSchema、返回字段与错误码语义见 `references/tool-contracts.md`（与服务端 `ehr-api/src/mcp.rs` 的 `tool_schemas()` 一一对应，改动须同步）。
