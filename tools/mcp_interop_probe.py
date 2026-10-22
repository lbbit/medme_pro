#!/usr/bin/env python3
"""medme-ehr · T35「真·第三方 MCP 客户端互通」探针（官方 MCP Python SDK 消费本服务）。

为什么需要它（PLAN 未销账项③「真·第三方 MCP 客户端互通仍未验」）：
    本仓库自己的 `mcp-e2e.sh` / `mcp-resources-e2e.sh` 都是**自己写 curl**调自己的服务
    ⇒ 只能证明「本实现自洽」，证不了「**官方 SDK 能不能吃**」。
    ⇒ 一个只被自家 curl 门覆盖的协议面，可能整族不合规范而门禁全绿。
    ⇒ 本探针用**官方 MCP Python SDK**（`mcp` 包）当**真·第三方客户端**：
       走 `streamablehttp_client` + `ClientSession`，读官方 pydantic 模型。

判据（缺一即红；输出 `[PASS]/[FAIL]` 与 fails=N 计数，供 shell 门汇总）：
    I1 握手：SDK 能 initialize，且 serverInfo.name == medme-ehr
    I2 tools/list 公布数 == **干跑清单**派生的工具数（不手抄）
    I3 tools/call 真跑通：建档 → get_labs → structuredContent 可解析且字段自洽
    I4 resources/list + resources/templates/list 数量 == 干跑清单派生值
    I5 **跨面不变量在第三方客户端上复现**：read_resource 的文本与
       call_tool 的 structuredContent **逐字相等**（同一份 payload builder）
    I6 **错误语义可判读**：未知 URI ⇒ SDK 抛 **McpError**（不是 ValidationError 崩溃）
       且 `error.code == -32005`；查无此人 ⇒ -32001
       ⚠️ 这一条是本轮**实测抓到的真缺陷**的判据：若服务端把 resources/read 的失败
          回成 `result.isError=true`（tools/call 的形状），官方 SDK 会把它当
          ReadResourceResult 校验 ⇒ 缺 `contents` ⇒ 抛 **ValidationError**
          ⇒ 宿主 agent 拿到的是「客户端崩溃」，错误码白设计了。

纪律：
    · 真进程、真实数据、零 mock；服务端由 shell 包装脚本起。
    · **不手抄期望值**：工具数/资源数一律从 `--print-mcp-tools` / `--print-mcp-resources`
      干跑清单派生（同一事实一份实现）。
    · 依赖：官方 `mcp` Python 包（版本由 shell 包装脚本安装/复用）。
"""

import asyncio
import json
import os
import sys

from mcp import ClientSession
from mcp.client.streamable_http import streamablehttp_client
from mcp.shared.exceptions import McpError

FAILS = 0


def gate(name, actual, expected):
    global FAILS
    if actual == expected:
        print("  PASS %s = %s" % (name, actual))
    else:
        print("  FAIL %s: got=%r want=%r" % (name, actual, expected))
        FAILS += 1


def note(msg):
    print("[证据] %s" % msg)


def norm(obj):
    """逐字比较用的规范形（键排序、紧凑分隔符）。"""
    return json.dumps(obj, sort_keys=True, ensure_ascii=False, separators=(",", ":"))


async def main():
    base = os.environ["MCP_BASE"].rstrip("/")
    url = base + "/mcp"
    tools_expected = int(os.environ["MCP_TOOLS_EXPECTED"])
    resources_expected = int(os.environ["MCP_RESOURCES_EXPECTED"])
    templates_expected = int(os.environ["MCP_TEMPLATES_EXPECTED"])

    async with streamablehttp_client(url) as (read, write, _):
        async with ClientSession(read, write) as session:
            init = await session.initialize()
            gate("I1.initialize.server_name", init.serverInfo.name, "medme-ehr")
            # 官方 SDK 把服务端的能力声明解析成 pydantic 模型 ⇒ 形状不合规范会在这里炸
            gate("I1.initialize.tools_capability", init.capabilities.tools is not None, True)
            gate("I1.initialize.resources_capability", init.capabilities.resources is not None, True)
            gate("I1.initialize.prompts_not_declared", init.capabilities.prompts, None)

            tools = await session.list_tools()
            gate("I2.tools_count_matches_impl", len(tools.tools), tools_expected)

            resources = await session.list_resources()
            gate("I4.resources_count_matches_impl", len(resources.resources), resources_expected)
            templates = await session.list_resource_templates()
            gate("I4.templates_count_matches_impl", len(templates.resourceTemplates), templates_expected)

            # ---- I3：tools/call 真跑通（自己造人 ⇒ 不依赖已有数据）----
            created = await session.call_tool(
                "create_person",
                {"name": "第三方互通门禁", "gender": "male", "birthday": "1980-05-05"},
            )
            if created.isError:
                gate("I3.create_person.not_error", created.isError, False)
                raise SystemExit(1)
            person = created.structuredContent
            pid = person.get("id")
            if not pid:
                gate("I3.create_person.has_id", "missing", "uuid")
                raise SystemExit(1)
            note("created person_id=%s" % pid)

            labs = await session.call_tool("get_labs", {"person_id": pid})
            gate("I3.get_labs.not_error", labs.isError, False)
            gate("I3.get_labs.has_person_id", labs.structuredContent.get("person_id"), pid)
            gate("I3.get_labs.has_series_key", "series" in labs.structuredContent, True)

            # ---- I5：跨面不变量在**第三方客户端**上复现 ----
            res = await session.read_resource("ehr://persons/%s/labs" % pid)
            text = res.contents[0].text
            gate("I5.read_resource.mime_json", res.contents[0].mimeType, "application/json")
            gate(
                "I5.read_resource_equals_call_tool",
                norm(json.loads(text)),
                norm(labs.structuredContent),
            )

            # ---- I6：错误语义必须**可判读**（官方 SDK 抛 McpError，而不是 ValidationError 崩溃）----
            try:
                await session.read_resource("ehr://definitely-not-a-resource")
                gate("I6.unknown_uri.raises_mcperror", "no-exception", "McpError")
            except McpError as exc:
                gate("I6.unknown_uri.raises_mcperror", "McpError", "McpError")
                gate("I6.unknown_uri.code", exc.error.code, -32005)
            except Exception as exc:  # 其它异常 = 客户端崩了 ⇒ 门红
                gate("I6.unknown_uri.raises_mcperror", type(exc).__name__, "McpError")

            missing_person = "00000000-0000-0000-0000-000000000000"
            try:
                await session.read_resource("ehr://persons/%s/labs" % missing_person)
                gate("I6.unknown_person.raises_mcperror", "no-exception", "McpError")
            except McpError as exc:
                gate("I6.unknown_person.raises_mcperror", "McpError", "McpError")
                gate("I6.unknown_person.code", exc.error.code, -32001)
            except Exception as exc:
                gate("I6.unknown_person.raises_mcperror", type(exc).__name__, "McpError")

    print("[证据] mcp_interop_probe.py fails=%d" % FAILS)
    return 1 if FAILS else 0


if __name__ == "__main__":
    sys.exit(asyncio.run(main()))
