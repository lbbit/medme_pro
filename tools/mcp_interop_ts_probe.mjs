#!/usr/bin/env node
/**
 * medme-ehr · T36「真·第三方 MCP 客户端互通（TypeScript 官方 SDK 面）」探针。
 *
 * 为什么需要它（D-052 决策②的续作 / PLAN 未销账项②）：
 *   T35 只覆盖了**官方 MCP Python SDK**。官方另有 TypeScript 实现（@modelcontextprotocol/sdk，
 *   本仓库宿主生态就是 TS/Node）⇒「官方 SDK 能不能吃」在 TS 面**仍未验**。
 *   两个实现共享规范，但**解析器不同**（TS 用 zod/v4，Python 用 pydantic v2）
 *   ⇒ 一个形状只在 Python 侧被拒收、TS 侧却能过（或反之）的缺陷是可能的
 *   ⇒ 只验一个实现 ⇒ 不能声称「协议面合规」。
 *
 * 判据（与 Python 面**同形**，便于双面对照；缺一即红）：
 *   T1 握手可解析（官方 zod 模型）        T2 工具数 == 干跑清单派生
 *   T3 tools/call 真跑通（自己造人）      T4 资源/模板数 == 干跑清单派生
 *   T5 跨面不变量（readResource == callTool 逐字相等）
 *   T6 错误语义可判读（McpError + code；其它异常 = 客户端崩溃 ⇒ 红）
 *
 * 纪律：
 *   · 真进程、真实数据、零 mock；服务端由 shell 包装脚本起。
 *   · **不手抄期望值**：工具数/资源数从 --print-mcp-tools / --print-mcp-resources 派生。
 *   · 官方 SDK 是**门禁侧**依赖，不是产品依赖（D-016 同形）⇒ 装到门禁专用目录，不进仓库。
 */

import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StreamableHTTPClientTransport } from "@modelcontextprotocol/sdk/client/streamableHttp.js";
import { McpError } from "@modelcontextprotocol/sdk/types.js";

let FAILS = 0;

function gate(name, actual, expected) {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  if (a === e) {
    console.log(`  PASS ${name} = ${a}`);
  } else {
    console.log(`  FAIL ${name}: got=${a} want=${e}`);
    FAILS += 1;
  }
}

function note(msg) {
  console.log(`[证据] ${msg}`);
}

/** 逐字比较用的规范形（键排序、紧凑分隔符）。 */
function norm(obj) {
  return JSON.stringify(obj, Object.keys(obj).sort());
}

async function main() {
  const base = process.env.MCP_BASE.replace(/\/+$/, "");
  const url = base + "/mcp";
  const toolsExpected = Number(process.env.MCP_TOOLS_EXPECTED);
  const resourcesExpected = Number(process.env.MCP_RESOURCES_EXPECTED);
  const templatesExpected = Number(process.env.MCP_TEMPLATES_EXPECTED);

  const client = new Client({ name: "medme-ehr-interop-ts-probe", version: "1.0.0" });
  const transport = new StreamableHTTPClientTransport(new URL(url));
  await client.connect(transport);

  // ---- T1：握手（官方 zod 模型解析服务端形状；不合规范会在这里炸）----
  const caps = client.getServerCapabilities();
  const ver = client.getServerVersion();
  gate("T1.initialize.server_name", ver && ver.name, "medme-ehr");
  gate("T1.initialize.tools_capability", !!(caps && caps.tools), true);
  gate("T1.initialize.resources_capability", !!(caps && caps.resources), true);
  // 「不提供 prompts」在 TS 侧表现为属性缺省（undefined）⇒ 归一成 null 与 Python 面同义
  gate("T1.initialize.prompts_not_declared", caps ? caps.prompts ?? null : null, null);

  const tools = await client.listTools();
  gate("T2.tools_count_matches_impl", tools.tools.length, toolsExpected);

  const resources = await client.listResources();
  gate("T4.resources_count_matches_impl", resources.resources.length, resourcesExpected);
  const templates = await client.listResourceTemplates();
  gate("T4.templates_count_matches_impl", templates.resourceTemplates.length, templatesExpected);

  // ---- T3：tools/call 真跑通（自己造人 ⇒ 空库不恒绿）----
  const created = await client.callTool({
    name: "create_person",
    arguments: { name: "TS第三方互通门禁", gender: "female", birthday: "1991-08-08" },
  });
  gate("T3.create_person.not_error", !!created.isError, false);
  const person = created.structuredContent;
  const pid = person && person.id;
  gate("T3.create_person.has_id", !!pid, true);
  note(`created person_id=${pid}`);

  const labs = await client.callTool({ name: "get_labs", arguments: { person_id: pid } });
  gate("T3.get_labs.not_error", !!labs.isError, false);
  gate("T3.get_labs.has_person_id", labs.structuredContent.person_id, pid);
  gate("T3.get_labs.has_series_key", "series" in labs.structuredContent, true);

  // ---- T5：跨面不变量在**第三方客户端**上复现 ----
  const res = await client.readResource({ uri: `ehr://persons/${pid}/labs` });
  const content = res.contents[0];
  gate("T5.read_resource.mime_json", content.mimeType, "application/json");
  gate("T5.read_resource_equals_call_tool", norm(JSON.parse(content.text)), norm(labs.structuredContent));

  // ---- T6：错误语义必须**可判读**（McpError，而不是 zod 崩溃）----
  try {
    await client.readResource({ uri: "ehr://definitely-not-a-resource" });
    gate("T6.unknown_uri.raises_mcperror", "no-exception", "McpError");
  } catch (err) {
    if (err instanceof McpError) {
      gate("T6.unknown_uri.raises_mcperror", "McpError", "McpError");
      gate("T6.unknown_uri.code", err.code, -32005);
    } else {
      gate("T6.unknown_uri.raises_mcperror", err && err.name ? err.name : String(err), "McpError");
    }
  }

  const missingPerson = "00000000-0000-0000-0000-000000000000";
  try {
    await client.readResource({ uri: `ehr://persons/${missingPerson}/labs` });
    gate("T6.unknown_person.raises_mcperror", "no-exception", "McpError");
  } catch (err) {
    if (err instanceof McpError) {
      gate("T6.unknown_person.raises_mcperror", "McpError", "McpError");
      gate("T6.unknown_person.code", err.code, -32001);
    } else {
      gate("T6.unknown_person.raises_mcperror", err && err.name ? err.name : String(err), "McpError");
    }
  }

  await client.close();
  console.log(`[证据] mcp_interop_ts_probe.mjs fails=${FAILS}`);
  process.exit(FAILS === 0 ? 0 : 1);
}

main().catch((err) => {
  console.log(`  FAIL probe_crashed: ${err && err.stack ? err.stack : String(err)}`);
  console.log("[证据] mcp_interop_ts_probe.mjs fails=1");
  process.exit(1);
});
