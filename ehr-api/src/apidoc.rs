// docs/API.md 的**生成器**（目标验收⑤：人类可读接口文档）。
//
// 为什么是「生成」而不是「手写」：手写的接口文档与代码漂移是本项目的经典漏账形态
// （D-016 教训：页面看着正常但数据全空；D-011 教训：期望值算错看起来像产品 bug）。
// 手写文档在第一次改路由时就会失真，而读者（人/agent）不会知道它已经过期。
// ⇒ 唯一事实源 = 运行时 OpenAPI（/openapi.json，服务端每次现合成）；
//   文档 = 从它渲染出来的 Markdown，且由 api-doc-check.sh 门禁断言「与生成物逐字一致」。
//
// 诚实性要求（D5：状态码不许粉饰）：CONTRACT 里尚未实装的端点在 OpenAPI 里就是 501，
// 渲染出的文档必须**显式标注 stub**，不能让人以为调了就能用。
use serde_json::Value;

/// 渲染人类可读 Markdown 文档。
pub fn render(doc: &Value) -> String {
    let mut s = String::new();
    let info = &doc["info"];
    s.push_str("# medme-ehr · API 接口文档（人类可读）\n\n");
    s.push_str("> ⚠️ **本文件是自动生成的，请勿手改。**\n");
    s.push_str("> 唯一事实源 = 服务运行时 OpenAPI（`GET /openapi.json`，服务端每次现合成）。\n");
    s.push_str("> 重新生成：`EHR_HOME=<dir> ehr-api --print-api-doc > docs/API.md`；\n");
    s.push_str("> 漂移门禁：`./api-doc-check.sh`（断言仓库内本文件与生成物**逐字一致**）。\n\n");

    // ---- 1. 概览 ----
    s.push_str("## 1. 概览\n\n");
    s.push_str(&format!(
        "- 标题：{}\n- 版本：`{}`\n- 许可：{}\n",
        info["title"].as_str().unwrap_or("?"),
        info["version"].as_str().unwrap_or("?"),
        doc["info"]["license"]["name"].as_str().unwrap_or("?")
    ));
    if let Some(d) = info["description"].as_str() {
        s.push_str(&format!("- 说明：{d}\n"));
    }
    s.push_str("- 根路径：所有业务端点都在 `/api/v1` 前缀下；`/openapi.json`（本机文档）、`/mcp`（MCP 协议面）、`/`（Web 最小版）不在该前缀内。\n");
    s.push_str("- 数据真相：**原件 CAS + 事件日志**是唯一真相；派生索引（SQLite/FTS）可随时重建（`ehr-api --rebuild-index`）。\n\n");

    // ---- 2. 认证 ----
    s.push_str("## 2. 认证\n\n");
    let sec = &doc["components"]["securitySchemes"]["bearerAuth"];
    if sec.is_object() {
        s.push_str(&format!(
            "- 方案：`{}` / `{}`（OpenAPI securitySchemes: `bearerAuth`）\n",
            sec["type"].as_str().unwrap_or("?"),
            sec["scheme"].as_str().unwrap_or("?")
        ));
        if let Some(d) = sec["description"].as_str() {
            s.push_str(&format!("- 说明：{d}\n"));
        }
    }
    s.push_str("- 生效方式：请求头 `Authorization: Bearer <token>`。\n");
    s.push_str("- scope：`ro` 只读 / `rw` 读写；只读 token 调写侧 ⇒ **403**（HTTP）或 JSON-RPC **`-32004`**（MCP）。\n");
    s.push_str("- 未配置 token ⇒ **open 模式**（全部放行），启动 `[证据]` 行会显式打印 `auth=open(tokens=0) ⚠️`。\n\n");

    // ---- 3. 状态码 ----
    s.push_str("## 3. 状态码与错误约定\n\n");
    s.push_str("| 码 | 含义 | 客户端动作 |\n|---|---|---|\n");
    s.push_str("| 200 | 成功 | — |\n");
    s.push_str("| 201 | 已创建（POST /persons） | — |\n");
    s.push_str("| 400 | 参数非法（缺 `q`、`limit` 非法、空 `name`、坏 JSON） | 改参数再试 |\n");
    s.push_str("| 401 | 缺 token / token 错 | 取或换 token |\n");
    s.push_str("| 403 | token 有效但 scope 不足（只读 token 想写） | 换读写 token |\n");
    s.push_str("| 404 | 人/文档不存在（或已归档） | 检查 `pid` / `sha` |\n");
    s.push_str("| 413 | 上传超出大小上限 | 压缩或分片（当前不支持） |\n");
    s.push_str("| 415 | 不支持的类型（`.dcm` 等，OCR/DICOM worker 未做） | 换格式 |\n");
    s.push_str("| 500 | 内部错误 | 看服务端日志 |\n");
    s.push_str("| 501 | **该端点尚未实装（stub）** | 用已实装端点或等待实现 |\n");
    s.push_str("| 503 | 派生索引正在重建中（响应带 `Retry-After: 5`） | 等待后重试 |\n\n");
    s.push_str("错误响应体统一为 `{\"error\": \"<原因>\"}`。\n\n");

    // ---- 4. 端点 ----
    s.push_str("## 4. 端点一览\n\n");
    s.push_str("| 方法 | 路径 | 说明 | 状态 |\n|---|---|---|---|\n");
    let paths = doc["paths"].as_object().cloned().unwrap_or_default();
    let mut ops: Vec<(String, String, String, bool)> = Vec::new(); // (method, path, summary, is_stub)
    for (p, item) in &paths {
        let map = match item.as_object() {
            Some(m) => m,
            None => continue,
        };
        for (m, op) in map {
            if !op.is_object() {
                continue;
            }
            let summary = op["summary"].as_str().unwrap_or("").replace('\n', " ");
            let is_stub = summary.contains(crate::STUB_MARK);
            ops.push((m.to_uppercase(), p.clone(), summary, is_stub));
        }
    }
    ops.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
    for (m, p, summary, is_stub) in &ops {
        s.push_str(&format!(
            "| `{}` | `{}` | {} | {} |\n",
            m,
            p,
            summary,
            if *is_stub { "⚠️ stub（501）" } else { "✅ 已实装" }
        ));
    }
    s.push('\n');

    // ---- 5. 端点详解 ----
    s.push_str("## 5. 端点详解\n\n");
    for (m, p, summary, is_stub) in &ops {
        let op = &paths[p][m.to_lowercase()];
        s.push_str(&format!("### `{} {}`\n\n", m, p));
        s.push_str(&format!("{}\n\n", summary));
        if *is_stub {
            s.push_str("> ⚠️ **本端点当前返回 501（尚未实装）**——下方是已固定的契约形状，实现后语义不再变。\n\n");
        }
        // 鉴权
        let public = op["security"].as_array().map(|a| a.is_empty()).unwrap_or(false);
        s.push_str(&format!(
            "- 鉴权：{}\n",
            if public { "公开（免 token）" } else { "需要 Bearer token" }
        ));
        // 路径/查询参数
        if let Some(params) = op["parameters"].as_array() {
            if !params.is_empty() {
                s.push_str("\n**参数**\n\n| 名 | 位置 | 必填 | 说明 |\n|---|---|---|---|\n");
                for pr in params {
                    s.push_str(&format!(
                        "| `{}` | {} | {} | {} |\n",
                        pr["name"].as_str().unwrap_or("?"),
                        pr["in"].as_str().unwrap_or("?"),
                        if pr["required"].as_bool().unwrap_or(false) { "是" } else { "否" },
                        pr["description"].as_str().unwrap_or("").replace('\n', " ")
                    ));
                }
            }
        }
        // 请求体
        if let Some(rb) = op.get("requestBody") {
            let ct = rb["content"].as_object().map(|c| {
                c.keys().cloned().collect::<Vec<_>>().join(", ")
            });
            let schema_ref = rb["content"]
                .as_object()
                .and_then(|c| c.values().next())
                .map(|v| v["schema"].clone())
                .unwrap_or(Value::Null);
            s.push_str(&format!(
                "\n**请求体**（content-type: {}）\n\n```json\n{}\n```\n\n",
                ct.unwrap_or_default(),
                pretty_schema(&schema_ref)
            ));
        }
        // 响应
        if let Some(resp) = op["responses"].as_object() {
            s.push_str("**响应**\n\n| 码 | 说明 |\n|---|---|\n");
            for (code, r) in resp {
                s.push_str(&format!(
                    "| `{}` | {} |\n",
                    code,
                    r["description"].as_str().unwrap_or("").replace('\n', " ")
                ));
            }
            s.push('\n');
        }
        // curl 示例
        s.push_str(&format!("\n**示例**\n\n```bash\n{}\n```\n\n", curl_example(m, p, op)));
    }

    // ---- 6. 数据模型 ----
    s.push_str("## 6. 数据模型（components.schemas）\n\n");
    if let Some(schemas) = doc["components"]["schemas"].as_object() {
        let mut names: Vec<&String> = schemas.keys().collect();
        names.sort();
        for n in names {
            let sc = &schemas[n];
            s.push_str(&format!("### `{}`\n\n", n));
            if let Some(d) = sc["description"].as_str() {
                s.push_str(&format!("{d}\n\n"));
            }
            if let Some(props) = sc["properties"].as_object() {
                s.push_str("| 字段 | 类型 | 必填 | 说明 |\n|---|---|---|---|\n");
                let required: Vec<&str> = sc["required"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
                    .unwrap_or_default();
                let mut pnames: Vec<&String> = props.keys().collect();
                pnames.sort();
                for f in pnames {
                    let fd = &props[f];
                    let ty = schema_type(fd);
                    s.push_str(&format!(
                        "| `{}` | {} | {} | {} |\n",
                        f,
                        ty,
                        if required.contains(&f.as_str()) { "是" } else { "否" },
                        fd["description"].as_str().unwrap_or("").replace('\n', " ")
                    ));
                }
                s.push('\n');
            }
        }
    }

    // ---- 7. 未实装端点 ----
    let stubs: Vec<&(String, String, String, bool)> = ops.iter().filter(|o| o.3).collect();
    if !stubs.is_empty() {
        s.push_str("## 7. 尚未实装（501）的端点\n\n");
        s.push_str("| 方法 | 路径 | 计划语义 |\n|---|---|---|\n");
        for (m, p, summary, _) in &stubs {
            s.push_str(&format!("| `{}` | `{}` | {} |\n", m, p, summary));
        }
        s.push_str("\n> 端点形状**已固定**（路由已挂、已进 OpenAPI），实现后语义不变；调用它们现在只会得到 501。\n");
    }
    s
}

/// 从 OpenAPI schema 片段里抽出人类可读的类型名。
fn schema_type(v: &Value) -> String {
    if let Some(r) = v["$ref"].as_str() {
        return r.rsplit('/').next().unwrap_or(r).to_string();
    }
    if let Some(arr) = v["items"].as_object() {
        let inner = schema_type(&Value::Object(arr.clone()));
        return format!("array<{inner}>");
    }
    if let Some(one) = v["oneOf"].as_array() {
        let names: Vec<String> = one.iter().map(schema_type).collect();
        return names.join(" | ");
    }
    if let Some(t) = v["type"].as_str() {
        let mut out = t.to_string();
        if let Some(f) = v["format"].as_str() {
            out.push_str(&format!("({f})"));
        }
        return out;
    }
    if v.is_object() && v.as_object().map(|o| o.is_empty()).unwrap_or(false) {
        return "any".into();
    }
    "object".into()
}

/// 请求体 schema 的可读化（$ref 展开成名字即可，完整定义见第 6 节）。
fn pretty_schema(v: &Value) -> String {
    let t = schema_type(v);
    if t.starts_with('{') {
        serde_json::to_string_pretty(v).unwrap_or_else(|_| "{}".into())
    } else {
        format!("{t}")
    }
}

/// 生成 curl 示例：路径参数替换为占位值，查询串带上必填 query 参数。
fn curl_example(method: &str, path: &str, op: &Value) -> String {
    let concrete = path
        .replace("{pid}", "<person_id>")
        .replace("{sha}", "<sha256>");
    let mut url = format!("http://127.0.0.1:8443{}", concrete);
    let mut qs: Vec<String> = Vec::new();
    if let Some(params) = op["parameters"].as_array() {
        for pr in params {
            if pr["in"].as_str() == Some("query") {
                let n = pr["name"].as_str().unwrap_or("");
                let ph = if pr["required"].as_bool().unwrap_or(false) {
                    format!("{n}=<{}>", pr["description"].as_str().unwrap_or(n))
                } else {
                    format!("{n}=<{n}>")
                };
                qs.push(ph);
            }
        }
    }
    if !qs.is_empty() {
        url.push('?');
        url.push_str(&qs.join("&"));
    }
    let mut cmd = format!("curl -X {} \\\n  -H 'Authorization: Bearer $EHR_API_TOKEN' \\\n  '{}'", method, url);
    if let Some(rb) = op.get("requestBody") {
        let is_multipart = rb["content"]
            .as_object()
            .map(|c| c.keys().any(|k| k.contains("multipart")))
            .unwrap_or(false);
        if is_multipart {
            cmd.push_str(" \\\n  -F 'file=@<原件路径>'");
        } else {
            cmd.push_str(" \\\n  -H 'content-type: application/json' \\\n  -d '<JSON body>'");
        }
    }
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn render_covers_every_path_and_method() {
        // ⚠️ json! 宏里嵌套对象字面量容易括号错位（我第一版就写坏过一次）——
        // 改用 from_str 从 JSON 文本解析，失败会立刻指明哪一行。
        let doc: Value = serde_json::from_str(
            r#"{
              "info": {"title":"t","version":"1","description":"d"},
              "components": {"schemas": {"Foo": {"type":"object","properties":{"a":{"type":"string"}},"required":["a"]}}},
              "paths": {
                "/a": {"get": {"summary":"x","responses":{"200":{"description":"ok"}}}},
                "/b/{id}": {"post": {"summary":"stub→501","responses":{"501":{"description":"nope"}}}}
              }
            }"#,
        )
        .expect("fixture json parses");
        let md = render(&doc);
        assert!(md.contains("/a"), "md lacks /a");
        assert!(md.contains("/b/{id}"), "md lacks /b/{{id}}");
        assert!(md.contains("stub（501）"), "stub 必须在文档里显式标注");
        assert!(md.contains("Bearer"), "认证段必须存在");
    }
}
