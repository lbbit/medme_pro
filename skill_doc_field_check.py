#!/usr/bin/env python3
# medme-ehr · T27「SKILL.md 字段面」门禁（内层实现；由 skill-doc-field-check.sh 调用）
#
# 为什么需要这道门（D-043 未销账①「SKILL.md 的字段描述面仍无门」的正面处置）：
#   D-040 把 **rest-contracts.md**（REST 面字段承诺）、D-043 把 **tool-contracts.md**（MCP 面字段承诺）
#   纳入了机器核对；但 **SKILL.md**（agent 真正读到的第一手说明）里承诺的字段名仍是
#   人读文档里的手抄实现细节 ⇒ 实现改了它不会红 ⇒ agent 照它取字段 → 取不到 → 数据全空
#   （D-016 家族的病，主角换成 SKILL.md）。
#   既有门 `skill-contract-check.sh` 只核**工具数/路径数/工具名**；`skill-rest-check.sh` 核的是
#   **它自己手写的期望字段表**（又是一份手抄清单，且不来自 SKILL.md）
#   ⇒ **SKILL.md 自身的字段承诺至今无门**。
#
# 判据（五条独立方向，缺一即算不通过）：
#   A **表格正向（逐命令）**：SKILL.md「字段承诺表」为某命令承诺的每个字段名，
#     必须出现在**该命令自己**的真响应里（逐命令核对，**不把多个响应拼一起查**——
#     D-043 决策②的纪律迁移：拼一起查 ⇒ 「A 命令承诺 B 命令的字段」照样绿）。
#   B **表格反向（逐命令）**：该命令真响应里出现的**契约级键**
#     （顶层 + 数组元素一层 + 元素内对象一层）必须被表格提及 ⇒ 表格漏字段 = agent 少取字段。
#   C **散文与表格一致（逐命令）**：命令速查表（散文）里提到的字段名，
#     必须在**同一命令**的「字段承诺表」行里出现 ⇒ 散文承诺了表格没有的字段 ⇒ 红。
#   D **命令面真实（双向）**：SKILL.md 提到的每个 `ehr.sh <命令>` 必须 helper 真支持
#     （真源 = `ehr.sh --help`，不手抄）；helper 支持的每个命令都必须被 SKILL.md 提到。
#   E **数量真实**：SKILL.md 写的 MCP 工具数与「读侧 N + 写侧 M」必须等于
#     **实现派生**的值（真源 = `ehr-api --print-mcp-tools`）。
#
# 关键纪律（沿用 D-011 家族 / D-043 决策③）：
#   - **枚举值不是字段名**：`status:ok` 的 `ok`、`source=registry` 的 `registry` 是**值** ⇒
#     只有当该名字**不是任何响应的键、却等于某个响应的标量值**时才排除（打印 INFO 依据）。
#   - **自由载荷内部不参与断言**：`text`/`payload`/`detail`/`raw`/`json` 的**内部**是用户数据。
#   - **缺省语义**：表格里标了「缺省」的字段允许真响应里没有（不逼文档写假承诺）。
#
# 用法:
#   skill_doc_field_check.py <PORT> <SKILL.md> <ehr.sh 路径> [--dump]
import json
import os
import re
import subprocess
import sys
import tempfile
import time
import urllib.request

PORT = int(sys.argv[1])
SKILL = sys.argv[2]
HELPER = sys.argv[3]
DUMP = "--dump" in sys.argv

BIN = os.path.expanduser(os.environ.get("EHR_BIN", "~/medme-shared-target/release/ehr-api"))
BASE = f"http://127.0.0.1:{PORT}"
fails = 0

# ---- helper 支持的命令（真源：--help 输出，不手抄）
help_out = subprocess.run([HELPER, "--help"], capture_output=True, text=True).stdout
COMMANDS = sorted(set(re.findall(r"^  ([a-z][a-z0-9-]*)", help_out, re.M)))
if not COMMANDS:
    print("helper --help 未解析出命令清单")
    sys.exit(1)

ENV_LIKE = re.compile(r"^[A-Z][A-Z0-9_]{2,}$")
FILE_LIKE = re.compile(r"^[\w./-]*\.[a-z]{2,4}$")
FLAG_LIKE = re.compile(r"^--?[a-z][a-z0-9-]*$")
VERB_LIKE = {"GET", "POST", "PUT", "DELETE", "PATCH", "HTTP", "JSON", "MCP", "REST",
             "LLM", "PDF", "TXT", "OK", "TRUE", "FALSE", "SHA", "PID", "OP", "NAME", "Q", "FILE"}

FREE_FORM = {"text", "payload", "event_payload", "raw", "json", "snippet", "detail"}


def http(method, url, body=None):
    data = json.dumps(body).encode() if body is not None else None
    hdr = {"content-type": "application/json"} if data else {}
    r = urllib.request.Request(url, data=data, headers=hdr, method=method)
    with urllib.request.urlopen(r) as f:
        return json.loads(f.read() or b"{}")


def sh(env, *args):
    r = subprocess.run([HELPER] + list(args), env=env, capture_output=True, text=True)
    try:
        return json.loads(r.stdout)
    except Exception:
        return {"__raw__": r.stdout[:200]}


def contract_keys(o, depth=0):
    """契约级键：顶层 + 数组元素一层 + 元素内对象一层（自由载荷的**内部**不参与）"""
    out = set()
    if isinstance(o, list):
        if o and isinstance(o[0], dict):
            out |= contract_keys(o[0], depth + 1)
        return out
    if not isinstance(o, dict):
        return out
    for k, v in o.items():
        out.add(k)
        if k in FREE_FORM:
            continue
        if isinstance(v, list) and v and isinstance(v[0], dict):
            out |= contract_keys(v[0], depth + 1)
        elif isinstance(v, dict) and depth < 2:
            # 顶层对象的**子对象**也是契约的一部分（如 `ocr_meta` / `extraction` / `verify_detail`）
            for k2, v2 in v.items():
                out.add(k2)
                if k2 in FREE_FORM:
                    continue
                if isinstance(v2, list) and v2 and isinstance(v2[0], dict):
                    out |= contract_keys(v2[0], depth + 2)
    return out


def all_keys(o, acc=None, inside_free=False, depth=0):
    acc = set() if acc is None else acc
    if depth > 8:
        return acc
    if isinstance(o, dict):
        for k, v in o.items():
            acc.add(k)
            if not inside_free:
                all_keys(v, acc, k in FREE_FORM, depth + 1)
    elif isinstance(o, list):
        for e in o:
            all_keys(e, acc, inside_free, depth + 1)
    return acc


def scalar_values(o, acc=None, inside_free=False, depth=0):
    acc = set() if acc is None else acc
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


# ================= 造真数据（字段级门必须自己造数据，否则恒绿） =================
work = tempfile.mkdtemp(prefix="skill-doc-gate-")
os.makedirs(os.path.join(work, "vaults"), exist_ok=True)
env = dict(os.environ, EHR_HOME=work, EHR_API_BASE=BASE)
srv = subprocess.Popen([BIN, f"--addr=127.0.0.1:{PORT}"], env=env,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
for _ in range(80):
    try:
        urllib.request.urlopen(BASE + "/api/v1/healthz", timeout=1)
        break
    except Exception:
        time.sleep(0.25)
else:
    print("server did not come up")
    sys.exit(1)

pid = http("POST", BASE + "/api/v1/persons",
           {"name": "契约门禁", "gender": "female", "birthday": "1988-03-02"}).get("id", "")
docs = {}
for fname, body in [
    ("lab.txt", "姓名：契约门禁\n检查项目：血常规\n白细胞计数 11.2 10^9/L 偏高\n血红蛋白 102 g/L 偏低\n"),
    ("rx.txt", "姓名：契约门禁\n处方\n阿莫西林胶囊 0.5g bid 口服\n"),
    ("dx.txt", "姓名：契约门禁\n出院记录\n主要诊断：2型糖尿病(E11.9)\n"),
]:
    p = os.path.join(work, fname)
    open(p, "w").write(body)
    out = subprocess.run(["curl", "-s", "-X", "POST", f"{BASE}/api/v1/persons/{pid}/documents",
                          "-F", f"file=@{p}"], capture_output=True, text=True).stdout
    try:
        docs[fname] = json.loads(out).get("content_hash")
    except Exception:
        docs[fname] = None
http("POST", f"{BASE}/api/v1/persons/{pid}/entries/self-measurement",
     {"values": [{"analyte_key": "bp_systolic", "value": 128, "unit": "mmHg"},
                 {"analyte_key": "bp_diastolic", "value": 82, "unit": "mmHg"}]})
http("POST", f"{BASE}/api/v1/persons/{pid}/entries/note", {"text": "今天有点头晕"})
http("POST", f"{BASE}/api/v1/persons/{pid}/entries/profile-event",
     {"kind": "flare", "package": "sle", "at": "2026-10-20", "payload": {"note": "flare"}})
SHA = docs.get("lab.txt")
extp = os.path.join(work, "ext.json")
open(extp, "w").write(json.dumps({"labs": [{"name": "白细胞计数", "value": "11.2", "unit": "10^9/L"}]}))

CALLS = {
    "health": ["health"],
    "capabilities": ["capabilities"],
    "persons": ["persons"],
    "timeline": ["timeline", pid],
    "search": ["search", pid, "白细胞"],
    "labs": ["labs", pid],
    "medications": ["medications", pid],
    "documents": ["documents", pid],
    "measurements": ["measurements", pid],
    "detail": ["detail", pid, SHA],
    "extraction": ["extraction", pid, SHA, extp, "--mode", "text", "--backend", "gate",
                   "--model-version", "g1"],
    "audit": ["audit"],
    "person-create": ["person-create", "契约门禁写侧", "--gender", "male"],
    "upload": ["upload", pid, os.path.join(work, "lab.txt")],
    "note": ["note", pid, "今天有点头晕"],
    "profile-event": ["profile-event", pid, "--kind", "flare", "--package", "sle",
                      "--at", "2026-10-20"],
    "self-measure": ["self-measure", pid, "--value", "bp_systolic=128", "--unit", "mmHg"],
}
# 非 JSON 输出的命令（原始字节流）⇒ 不参与字段级判据（内容不是 JSON 契约）
NON_JSON = {"original"}
# 先回写一条 extraction（否则 detail 取证时该文档还没有 extraction 字段 ⇒ 门会假红）
sh(env, "extraction", pid, SHA, extp, "--mode", "text", "--backend", "gate", "--model-version", "g1")
resp = {c: sh(env, *a) for c, a in CALLS.items()}

UNIVERSE_KEYS = set()
UNIVERSE_VALS = set()
OWNER = {}
for c, v in resp.items():
    UNIVERSE_KEYS |= all_keys(v)
    UNIVERSE_VALS |= scalar_values(v)
    for k in contract_keys(v):
        OWNER.setdefault(k, set()).add(c)

# ================= 解析 SKILL.md =================
skill_text = open(SKILL, encoding="utf-8").read()
skill_lines = skill_text.splitlines()
CMD_RE = re.compile(r"\behr\.sh\s+([a-z][a-z0-9-]*)")
SPAN_RE = re.compile(r"`([^`\n]+)`")

# ---- ①「字段承诺表」：机器可解析面（表头固定为「命令 | 字段」）
table = {}          # command -> set(field)
table_optional = {}  # command -> set(field)
in_table = False
for line in skill_lines:
    if line.strip().startswith("## "):
        in_table = "字段承诺表" in line
        continue
    if not in_table or not line.strip().startswith("|"):
        continue
    cells = [c.strip() for c in line.strip().strip("|").split("|")]
    if len(cells) < 2:
        continue
    m = re.match(r"^`ehr\.sh\s+([a-z][a-z0-9-]*)[^`]*`", cells[0])
    if not m:
        continue
    cmd = m.group(1)
    names, optional = set(), set()
    for mm in re.finditer(r"`([A-Za-z_][A-Za-z0-9_]*(?:\[\])?)`", cells[1]):
        nm = mm.group(1).replace("[]", "")
        names.add(nm)
        window = cells[1][max(0, mm.start() - 40): mm.end() + 60]
        if "缺省" in window or "可选" in window or "仅当" in window:
            optional.add(nm)
    table.setdefault(cmd, set()).update(names)
    table_optional.setdefault(cmd, set()).update(optional)

# ---- ②「命令速查」散文面：每行命令 + 该行提到的字段名
prose = {}      # command -> set(field)
prose_lines = {}  # command -> set(line_no)
for lineno, line in enumerate(skill_lines, 1):
    m = CMD_RE.search(line)
    if not m:
        continue
    cmd = m.group(1)
    if cmd not in COMMANDS:
        continue
    spans = [(x.start(), x.end(), x.group(1)) for x in SPAN_RE.finditer(line)]
    after = [s for s in spans if s[1] > m.end() and s[0] >= m.end()]
    names = set()
    for _s0, _s1, span in after:
        raw = span.strip()
        if "|" in raw or raw.startswith("-"):
            continue
        if re.fullmatch(r"[\w./-]*\.[a-z]{2,4}", raw):
            continue
        left = raw
        for op in ("=", ":"):
            if op in raw:
                cand_left, _ = raw.split(op, 1)
                if re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", cand_left.strip()):
                    left = cand_left
                    break
        for tok in re.findall(r"[A-Za-z_][A-Za-z0-9_]*(?:\[\])?(?:\.[A-Za-z_][A-Za-z0-9_]*)?", left):
            base = tok.split(".")[-1].replace("[]", "")
            if base in COMMANDS or base in VERB_LIKE:
                continue
            if ENV_LIKE.match(base) or FILE_LIKE.match(base) or FLAG_LIKE.match(base):
                continue
            names.add(base)
    if names:
        prose.setdefault(cmd, set()).update(names)
        prose_lines.setdefault(cmd, set()).add(lineno)

if DUMP:
    print("[helper 命令]", COMMANDS)
    print("[表格行]", {k: sorted(v) for k, v in table.items()})
    print("[散文]", {k: sorted(v) for k, v in prose.items()})
    for c in sorted(resp):
        print("  real", c, sorted(contract_keys(resp[c])))
    srv.terminate()
    sys.exit(0)

# ================= 判据 =================
print(f"[判据] helper 命令数={len(COMMANDS)}；表格覆盖命令={len(table)}；散文承诺字段的命令={len(prose)}")

# ---- A/B：逐命令正向 + 反向
for cmd, names in sorted(table.items()):
    if cmd in NON_JSON:
        print(f"  PASS {cmd} 非 JSON 输出（原始字节流）⇒ 跳过字段级判据")
        continue
    r = resp.get(cmd)
    if r is None:
        print(f"  FAIL {cmd} 表格有此命令但本门未取证（门自己漏了）")
        fails += 1
        continue
    real_all = all_keys(r)
    real_contract = contract_keys(r)
    enum_vals = {n for n in names if n not in real_all and n not in UNIVERSE_KEYS and n in UNIVERSE_VALS}
    if enum_vals:
        print(f"  INFO {cmd} 枚举值(非字段,已排除): {sorted(enum_vals)}")
    fields = sorted(n for n in names if n not in enum_vals)
    missing = [n for n in fields if n not in real_all and n not in table_optional.get(cmd, set())]
    if missing:
        print(f"  FAIL {cmd} 表格承诺但真响应没有的字段: {missing}")
        fails += 1
    else:
        print(f"  PASS {cmd} 表格承诺字段全部真实存在({len(fields)} 个)")
    undocumented = sorted(k for k in real_contract if k not in names and k not in enum_vals)
    if undocumented:
        print(f"  FAIL {cmd} 真响应有但表格未提及的字段: {undocumented}")
        fails += 1
    else:
        print(f"  PASS {cmd} 真响应字段表格都提及({len(real_contract)} 个)")

# 表格必须覆盖 helper 的每个命令（否则该命令的字段承诺无门）
uncovered = sorted(set(COMMANDS) - set(table))
if uncovered:
    print(f"  FAIL 表格未覆盖的命令（其字段承诺无门）: {uncovered}")
    fails += 1
else:
    print(f"  PASS 表格覆盖 helper 全部命令({len(COMMANDS)} 个)")

# ---- C：散文与表格一致（逐命令）
for cmd, names in sorted(prose.items()):
    tnames = table.get(cmd, set())
    extra = []
    for n in sorted(names):
        if n in tnames:
            continue
        # **跨命令引用**是允许的：若该字段真属于别的命令，且**同一行点名了那个命令**
        # ⇒ agent 能知道该去哪取 ⇒ 合法（否则 agent 无处可寻）。
        owners = OWNER.get(n, set())
        mentioned = False
        for ln in prose_lines.get(cmd, set()):
            txt = skill_lines[ln - 1]
            if any(re.search(rf"\b{re.escape(o)}\b", txt) for o in owners):
                mentioned = True
                break
        if not mentioned:
            extra.append(n)
    if extra:
        print(f"  FAIL {cmd} 散文提到但表格未列、且该行未点名拥有它的命令: {extra}")
        fails += 1
    else:
        print(f"  PASS {cmd} 散文字段与表格一致({len(names)} 个)")

# ---- D：命令面真实（双向）
doc_cmds = set(CMD_RE.findall(skill_text))
phantom = sorted(doc_cmds - set(COMMANDS))
if phantom:
    print(f"  FAIL SKILL.md 提到 helper 不支持的命令: {phantom}")
    fails += 1
else:
    print(f"  PASS SKILL.md 提到的命令都真实存在({len(doc_cmds)} 个)")
unmentioned = sorted(set(COMMANDS) - doc_cmds)
if unmentioned:
    print(f"  FAIL helper 支持但 SKILL.md 未提及的命令: {unmentioned}")
    fails += 1
else:
    print(f"  PASS helper 的每个命令都被 SKILL.md 提及({len(COMMANDS)} 个)")

# ---- E：数量真实（实现派生）
manifest = json.loads(subprocess.run([BIN, "--print-mcp-tools"], env=env,
                                     capture_output=True, text=True).stdout)
exp_count, exp_read, exp_write = manifest["count"], manifest["read"], manifest["write"]
m = re.search(r"(\d+)\s*个工具", skill_text)
if not m:
    print("  FAIL SKILL.md 未写「N 个工具」")
    fails += 1
elif int(m.group(1)) != exp_count:
    print(f"  FAIL SKILL.md 工具数={m.group(1)} ≠ 实现派生 {exp_count}")
    fails += 1
else:
    print(f"  PASS SKILL.md 工具数={m.group(1)} == 实现派生")
m = re.search(r"读侧\s*(\d+)\s*\+\s*写侧\s*(\d+)", skill_text)
if not m:
    print("  FAIL SKILL.md 未写「读侧 N + 写侧 M」")
    fails += 1
elif (int(m.group(1)), int(m.group(2))) != (exp_read, exp_write):
    print(f"  FAIL SKILL.md 读写分栏=({m.group(1)},{m.group(2)}) ≠ 实现派生 ({exp_read},{exp_write})")
    fails += 1
else:
    print(f"  PASS SKILL.md 读写分栏=({m.group(1)},{m.group(2)}) == 实现派生")

srv.terminate()
print(f"[证据] skill-doc-field-check fails={fails}")
sys.exit(1 if fails else 0)
