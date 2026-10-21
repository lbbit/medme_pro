#!/usr/bin/env python3
# medme-ehr · T26「MCP 工具契约字段面」门禁（内层实现；由 mcp-contract-check.sh 调用）
#
# 判据（D-009「判据形状必须匹配被测事实的性质」）：
#   ① **正向（逐工具）**：tool-contracts.md 为某工具承诺的**每个字段名**，
#      必须出现在**该工具自己**的真响应里（任意层级）—— 文档少写 ⇒ agent 取不到字段。
#   ② **反向（逐工具）**：该工具真响应里出现的键（顶层 + 数组元素一层 + 元素内的对象一层），
#      必须在文档里被提及 —— 文档没写、实际有 ⇒ agent 按文档写代码会漏字段。
#   ③ **工具覆盖**：实现派生的每个工具都必须有文档行（漏工具 ⇒ FAIL）。
#   ④ **可选键**语义：文档里明确写了「缺省/可选」的名字允许**真响应里没有**（不逼文档写假承诺）。
#
# 为什么这道门是新的（D-041 未销账①「SKILL.md / tool-contracts.md 的字段面仍无门」的正面处置）：
#   D-040 只把 **rest-contracts.md**（REST 面）纳入了机器核对；**MCP 面**（tool-contracts.md）
#   的「structuredContent 关键字段」列仍是人读文档里的手抄实现细节 ⇒ 实现改了它不会红。
#   之前的 `skill-contract-check.sh` 只核「工具数/路径数/工具名」，**不核字段**。
#
# 为什么必须「逐工具」而不是「把所有响应拼一起查」（本轮修掉的一个真缺陷）：
#   拼在一起查 ⇒ 字段只要在**任意一个**工具的响应里出现过就算过 ⇒
#   「A 工具的文档承诺了 B 工具才有的字段」照样绿 ⇒ **门在测错对象**（D-037 同族）。
#
# 用法:
#   mcp_contract_check.py <PORT> <tool-contracts.md> <ehr-api 二进制路径> [--dump]
#     --dump  只打印每工具的真字段清单与文档列（dry-run 用），不判失败
import base64
import json
import os
import re
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request

PORT = int(sys.argv[1])
RDOC = sys.argv[2]
BIN = sys.argv[3]
DUMP = "--dump" in sys.argv

BASE = f"http://127.0.0.1:{PORT}"
API = BASE + "/api/v1"
fails = 0


def http(method, url, body=None, ctype="application/json"):
    data = None
    hdr = {}
    if body is not None:
        data = body if isinstance(body, bytes) else json.dumps(body, ensure_ascii=False).encode()
        hdr["content-type"] = ctype
    r = urllib.request.Request(url, data=data, headers=hdr, method=method)
    try:
        with urllib.request.urlopen(r) as f:
            return json.loads(f.read() or b"{}")
    except urllib.error.HTTPError as e:
        try:
            return json.loads(e.read() or b"{}")
        except Exception:
            return {}


def rpc(name, args, i=1):
    payload = {"jsonrpc": "2.0", "id": i, "method": "tools/call",
               "params": {"name": name, "arguments": args}}
    d = http("POST", BASE + "/mcp", payload)
    res = d.get("result", {})
    if res.get("isError"):
        return {"__error__": res}
    return res.get("structuredContent", {})


# ---- 自由载荷：**键本身**是契约的一部分（文档会写 `text`/`payload`/`detail`），
#      但**它内部**的键是用户数据，不是服务端契约（D-040 决策④）⇒ 加键、停递归。
FREE_FORM = {"payload", "event_payload", "text", "raw", "json", "snippet", "detail"}


def keys_all(o, acc=None, inside_free=False, depth=0):
    """所有出现过的键名；自由载荷的**内部**子树跳过（键本身仍计入）"""
    acc = set() if acc is None else acc
    if depth > 6:
        return acc
    if isinstance(o, dict):
        for k, v in o.items():
            acc.add(k)
            if inside_free:
                continue
            keys_all(v, acc, k in FREE_FORM, depth + 1)
    elif isinstance(o, list):
        for e in o:
            keys_all(e, acc, inside_free, depth + 1)
    return acc


def keys_contract(o):
    """反向判据的范围：顶层键 + 数组元素一层 + 元素内对象一层（更深的算自由载荷/噪音）"""
    keys = set()
    if not isinstance(o, dict):
        return keys
    for k, v in o.items():
        if k in FREE_FORM:
            continue
        keys.add(k)
        if isinstance(v, list) and v and isinstance(v[0], dict):
            for kk, vv in v[0].items():
                if kk in FREE_FORM:
                    continue
                keys.add(kk)
                if isinstance(vv, dict):
                    for k3 in vv:
                        if k3 not in FREE_FORM:
                            keys.add(k3)
    return keys


def scalar_values(o, acc, inside_free=False, depth=0):
    """响应里出现过的标量值（字符串/数字/布尔）——用于识别「枚举值」"""
    if depth > 8:
        return acc
    if isinstance(o, dict):
        for k, v in o.items():
            if inside_free:
                continue
            if isinstance(v, (str, int, float)) and not isinstance(v, bool):
                acc.add(str(v))
            scalar_values(v, acc, k in FREE_FORM, depth + 1)
    elif isinstance(o, list):
        for e in o:
            scalar_values(e, acc, inside_free, depth + 1)
    return acc


def split_row(s):
    """按**未转义**的 `|` 切表格行（文档里 `create`\\|`update` 用了转义竖线）"""
    cells, cur, esc = [], "", False
    for ch in s.strip().strip("|"):
        if esc:
            cur += ch
            esc = False
        elif ch == "\\":
            cur += ch
            esc = True
        elif ch == "|":
            cells.append(cur.strip())
            cur = ""
        else:
            cur += ch
    cells.append(cur.strip())
    return cells


# ---- 造真实数据（字段级门必须自己造出能让字段出现的真实数据；否则门恒绿）
work = tempfile.mkdtemp(prefix="mcp-gate-")
os.makedirs(os.path.join(work, "vaults"), exist_ok=True)
env = dict(os.environ, EHR_HOME=work)
srv = subprocess.Popen([BIN, f"--addr=127.0.0.1:{PORT}"], env=env,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
for _ in range(60):
    try:
        urllib.request.urlopen(BASE + "/api/v1/healthz", timeout=1)
        break
    except Exception:
        time.sleep(0.25)
else:
    print("server did not come up")
    sys.exit(1)

person = http("POST", API + "/persons",
              {"name": "契约门禁", "gender": "female", "birthday": "1988-03-02"})
PID = person.get("id", "")
if not PID:
    print("cannot create person")
    srv.terminate()
    sys.exit(1)

docs = {}
for fname, body in [
    ("lab.txt", "姓名：契约门禁\n检查项目：血常规\n白细胞计数 11.2 10^9/L 偏高\n血红蛋白 102 g/L 偏低\n"),
    ("rx.txt", "姓名：契约门禁\n处方\n阿莫西林胶囊 0.5g bid 口服\n"),
    ("dx.txt", "姓名：契约门禁\n出院记录\n主要诊断：2型糖尿病(E11.9)\n"),
]:
    p = os.path.join(work, fname)
    open(p, "w").write(body)
    out = subprocess.run(["curl", "-s", "-X", "POST", f"{API}/persons/{PID}/documents",
                          "-F", f"file=@{p}"], capture_output=True, text=True).stdout
    try:
        docs[fname] = json.loads(out).get("content_hash")
    except Exception:
        docs[fname] = None
http("POST", f"{API}/persons/{PID}/entries/self-measurement",
     {"values": [{"analyte_key": "bp_systolic", "value": 128, "unit": "mmHg"},
                 {"analyte_key": "bp_diastolic", "value": 82, "unit": "mmHg"}]})
http("POST", f"{API}/persons/{PID}/entries/note", {"text": "今天有点头晕"})
http("POST", f"{API}/persons/{PID}/entries/profile-event",
     {"kind": "flare", "package": "sle", "at": "2026-10-20", "payload": {"note": "flare"}})

SHA = docs.get("lab.txt")

CALLS = [
    ("list_persons", {}),
    ("get_timeline", {"person_id": PID}),
    ("search_records", {"person_id": PID, "q": "白细胞"}),
    ("get_labs", {"person_id": PID}),
    ("get_medications", {"person_id": PID}),
    ("get_conditions", {"person_id": PID}),
    ("get_measurements", {"person_id": PID}),
    ("get_audit", {}),
    ("get_capabilities", {}),
    ("list_documents", {"person_id": PID}),
    ("get_document_detail", {"person_id": PID, "sha256": SHA}),
    ("create_person", {"name": "契约门禁写侧"}),
    ("upload_document", {"person_id": PID, "filename": "血常规.txt",
                         "content_base64": base64.b64encode(
                             "姓名：契约门禁\n白细胞计数 13.1 10^9/L 偏高\n".encode()).decode()}),
]

MANIFEST = json.loads(subprocess.run([BIN, "--print-mcp-tools"], env=env,
                                      capture_output=True, text=True).stdout)
TOOL_NAMES = MANIFEST["names"]

# ---- 文档解析：逐工具取「structuredContent 关键字段」列
doc_rows = {}
for line in open(RDOC, encoding="utf-8").read().splitlines():
    s = line.strip()
    if not s.startswith("|"):
        continue
    cells = split_row(s)
    if len(cells) < 2:
        continue
    m = re.match(r"^`([A-Za-z_][A-Za-z0-9_]*)`$", cells[0])
    if not m:
        continue
    tool = m.group(1)
    if tool not in TOOL_NAMES or tool in doc_rows:
        continue
    field_cell = cells[-1]
    names, optional = [], set()
    # 名字允许带 `[]` 后缀（`persons[]` 是**容器名**，也属文档承诺的一部分）
    for mm in re.finditer(r"`([A-Za-z_][A-Za-z0-9_]*)\s*(\[\])?`", field_cell):
        nm = mm.group(1)
        names.append(nm)
        window = field_cell[max(0, mm.start() - 60): mm.end() + 80]
        if "缺省" in window or "可选" in window or "仅当" in window:
            optional.add(nm)
    doc_rows[tool] = {"names": names, "optional": optional, "cell": field_cell}

if DUMP:
    for name, args in CALLS:
        r = rpc(name, args)
        d = doc_rows.get(name, {})
        print("=" * 8, name)
        print("  doc     :", d.get("cell", "(无文档行)"))
        print("  docnames:", d.get("names", []))
        print("  real_all:", sorted(keys_all(r)))
        print("  real_own:", sorted(keys_contract(r)))
    srv.terminate()
    sys.exit(0)

# ---- 判据
print(f"[判据] 工具数(实现派生) = {len(TOOL_NAMES)}；文档有行的工具 = {len(doc_rows)}")
for tool in TOOL_NAMES:
    if tool not in doc_rows:
        print(f"  FAIL 工具无文档行: {tool} ⇒ agent 看不到它的契约")
        fails += 1

for name, args in CALLS:
    if name not in doc_rows:
        continue
    r = rpc(name, args)
    if "__error__" in r:
        print(f"  FAIL {name} 调用失败: {json.dumps(r, ensure_ascii=False)[:200]}")
        fails += 1
        continue
    doc = doc_rows[name]
    real_all = keys_all(r)
    real_own = keys_contract(r)
    vals = scalar_values(r, set())
    # 「枚举值」裁决（D-011 家族纪律）：文档里提到的名字若**不是真响应的键**、
    # 却**等于真响应里的某个标量值** ⇒ 它是枚举值（`status`(恒 `active`)、`registry`/`vault`），
    # 不是字段名 ⇒ 不参与正反向判据（否则会逼文档把枚举值写成字段，或门假红）。
    enum_vals = {n for n in doc["names"] if n not in real_all and n in vals}
    doc["names"] = [n for n in doc["names"] if n not in enum_vals]
    if enum_vals:
        print(f"  INFO {name} 枚举值(非字段,已排除): {sorted(enum_vals)}")
    missing = [n for n in doc["names"] if n not in real_all and n not in doc["optional"]]
    if missing:
        print(f"  FAIL {name} 文档承诺但真响应没有的字段: {missing}")
        fails += 1
    else:
        print(f"  PASS {name} 文档承诺字段全部存在({len(doc['names'])} 个)")
    undocumented = sorted(k for k in real_own if k not in doc["names"])
    if undocumented:
        print(f"  FAIL {name} 真响应有但文档未提及的字段: {undocumented}")
        fails += 1
    else:
        print(f"  PASS {name} 真响应字段文档都提及({len(real_own)} 个)")

srv.terminate()
print(f"[证据] mcp-contract-check fails={fails}")
sys.exit(1 if fails else 0)
