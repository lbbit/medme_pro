#!/usr/bin/env python3
# medme-ehr · T23「文档面」门禁（内层实现；由 doc-contract-check.sh 调用）
#
# 判据（D-009 判据形状原则：判据形状必须能抓住真正的失败模式）：
#   ① 正向：rest-contracts.md 承诺的**每个字段名**必须出现在真响应的**任意层级**里
#      —— 若「文档承诺了但真响应没有」，则按 OpenAPI 的 `required` 分两种成因：
#        · schema 说它是 required ⇒ **实现的 bug**（真响应缺必填字段）
#        · schema 说它是可选 ⇒ **文档过度承诺**（键可缺省，文档却没说）
#   ② 反向：真响应里出现的每个键（**任意层级**，自由 JSON 载荷除外）必须被文档提及
#      —— 「文档没说、实际有」同样是漂移（agent 按文档写代码 ⇒ 漏字段）
#   ③ 端点覆盖：OpenAPI 派生的每个 operation 都必须在文档表里有一行（漏端点 ⇒ FAIL）
#
# 为什么要新写这道门（D-039 未销账①「文档面未逐条核」的正面处置）：
#   D-037~D-039 把「stub 清单」「REST 路径数」「响应形状 vs OpenAPI」三处硬编码治成了「实现派生 + 机器核对」，
#   但 **rest-contracts.md 的「返回关键字段」列仍是人读文档里的手抄实现细节**——它抄的是当时的实现形状，
#   实现改了它不会红（没有门）⇒ 它会**悄悄变假**（agent 照文档取字段 → 取不到 → 数据全空，D-016 家族的病）。
#
# 用法: doc_contract_check.py <PORT> <rest-contracts.md> <ehr-api 二进制路径>
import json, os, re, subprocess, sys, urllib.request, urllib.parse, urllib.error

PORT, RDOC, BIN = sys.argv[1], sys.argv[2], sys.argv[3]
B = f"http://127.0.0.1:{PORT}/api/v1"
fails = 0
work = "/tmp/doc-gate-work"
os.makedirs(work, exist_ok=True)


def req(method, path, body=None):
    data = None
    hdr = {}
    if body is not None:
        data = json.dumps(body, ensure_ascii=False).encode()
        hdr["content-type"] = "application/json"
    r = urllib.request.Request(B + path, data=data, headers=hdr, method=method)
    try:
        with urllib.request.urlopen(r) as f:
            return json.loads(f.read())
    except urllib.error.HTTPError as e:
        try:
            return json.loads(e.read() or b"{}")
        except Exception:
            return {}


def upload(p, path):
    out = subprocess.run(
        ["curl", "-s", "-X", "POST", B + f"/persons/{p}/documents", "-F", f"file=@{path}"],
        capture_output=True, text=True).stdout
    try:
        return json.loads(out)
    except Exception:
        return {}


FREE_FORM = {"payload", "event_payload", "text", "raw", "json", "snippet", "detail"}


def deep_keys(o, acc, inside_free=False):
    """任意层级出现过的键名；自由 JSON 载荷（payload/text/raw…）内部的键**不算**
    （那是用户数据，不是服务端契约的一部分）"""
    if isinstance(o, dict):
        for k, v in o.items():
            if inside_free:
                # 自由载荷内部：整个子树都跳过（连键名都不算）
                continue
            acc.add(k)
            deep_keys(v, acc, k in FREE_FORM)
    elif isinstance(o, list):
        for e in o:
            deep_keys(e, acc, inside_free)
    return acc


def deep_values(o, acc, inside_free=False):
    """响应里出现过的**标量值**（字符串/数字/布尔）——文档里写「恒 `active`」「`regex`/`llm`」
    这类**枚举值**时，它不是字段名，但也不是「文档在骗人」⇒ 值单独收一份，判据分开处理"""
    if isinstance(o, dict):
        for k, v in o.items():
            if inside_free:
                continue
            if isinstance(v, (str, int, float, bool)) and not isinstance(v, bool):
                acc.add(str(v))
            deep_values(v, acc, k in FREE_FORM)
    elif isinstance(o, list):
        for e in o:
            deep_values(e, acc, inside_free)
    return acc


def split_row(s):
    """按**未转义**的 `|` 切表格行（文档里 `mode`(`text`\\|`image`) 用了转义竖线）"""
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


def doc_fields(cell):
    """从「返回关键字段」列抽出名字集合，分两类：
      · **顶层名**（不在括号里）—— 一定是字段名（必查）
      · **括号里的名字** —— 可能是**子字段名**（`values[]`(`analyte_key`/...)）
        也可能是**枚举值**（`status`(恒 `active`)、`origin`(`regex`/`llm`)）——
        **解析阶段无法可靠区分** ⇒ 不猜：交由判据阶段用「OpenAPI 里它是不是属性名」来裁决
        （OpenAPI 声明的属性名 ⇒ 必查；不是 ⇒ 枚举值，跳过）。
    （D-011 家族纪律：门不能靠「猜」来判定，必须让判据能区分「文档写错」与「门自己解析错」。）"""
    top = ""
    paren = ""
    depth = 0
    for i, ch in enumerate(cell):
        if ch in "（(":
            depth += 1
            if depth == 1:
                continue
        elif ch in "）)":
            depth -= 1
            if depth == 0:
                continue
        if depth == 0:
            top += ch if ch not in "（(" else " "
        else:
            paren += ch
    top_names = {f for f in re.findall(r"`([A-Za-z_][A-Za-z0-9_]*)\s*(?:\[\])?`", top)}
    paren_names = {f for f in re.findall(r"`([A-Za-z_][A-Za-z0-9_]*)\s*(?:\[\])?`", paren)}
    return top_names, paren_names


def doc_optional(cell):
    """「可选键：」之后的名字是**可缺省**的（整个键可能不存在）"""
    if "可选键" not in cell:
        return set()
    tail = cell.split("可选键", 1)[1]
    return {f for f in re.findall(r"`([A-Za-z_][A-Za-z0-9_]*)\s*(?:\[\])?`", tail)}


# ---- OpenAPI：解析每个 operation 的 200/201 schema（required 集合）
with urllib.request.urlopen(f"http://127.0.0.1:{PORT}/openapi.json") as f:
    OAPI = json.load(f)
COMPS = OAPI.get("components", {}).get("schemas", {})


def resolve(sch):
    while isinstance(sch, dict) and "$ref" in sch:
        name = sch["$ref"].split("/")[-1]
        sch = COMPS.get(name, {})
    return sch or {}


def schema_props(op_path, method, code):
    op = OAPI["paths"].get(op_path, {}).get(method.lower(), {})
    resp = op.get("responses", {}).get(str(code), {})
    content = resp.get("content", {})
    for ct, c in content.items():
        if ct == "application/json":
            sch = resolve(c.get("schema", {}))
            props = set(sch.get("properties", {}).keys())
            req = set(sch.get("required", []))
            # 数组信封：元素 schema 的 required 也算（顶层数组的 item）
            items = resolve(sch.get("items", {})) if "items" in sch else {}
            if items:
                props |= set(items.get("properties", {}).keys())
                req |= set(items.get("required", []))
            return props, req
    return set(), set()


def schema_all_props(sch, acc):
    """schema 树里出现过的**全部属性名**（含嵌套）"""
    sch = resolve(sch)
    for k in sch.get("properties", {}):
        acc.add(k)
    for k, v in sch.get("properties", {}).items():
        schema_all_props(v, acc)
        for sub in ("items", "oneOf", "anyOf", "allOf"):
            if sub in v:
                items = v[sub] if sub != "items" else [v[sub]]
                for it in (items if isinstance(items, list) else [items]):
                    schema_all_props(it, acc)
    if "items" in sch:
        schema_all_props(sch["items"], acc)
    for sub in ("oneOf", "anyOf", "allOf"):
        for it in sch.get(sub, []):
            schema_all_props(it, acc)
    return acc


def op_schema(op_path, method, code):
    op = OAPI["paths"].get(op_path, {}).get(method.lower(), {})
    resp = op.get("responses", {}).get(str(code), {})
    for ct, c in resp.get("content", {}).items():
        if ct == "application/json":
            return c.get("schema", {})
    return {}


# ---- 解析文档表
doc = {}
prev_fields = set()
for line in open(RDOC, encoding="utf-8"):
    s = line.strip()
    if not s.startswith("|"):
        continue
    cells = split_row(s)
    if len(cells) < 3:
        continue
    m = re.match(r"^(GET|POST|PUT|DELETE)\s+(\S+)$", cells[0].replace("`", ""))
    if not m:
        continue
    method, path = m.group(1), m.group(2).split("?")[0]
    cell = cells[2]
    top_names, paren_names = doc_fields(cell)
    if "同上" in cell and prev_fields:
        top_names = set(prev_fields)
        paren_names = set()
    optional = doc_optional(cell)
    doc[(method, path)] = (top_names, paren_names, optional)
    prev_fields = top_names | paren_names

# ---- 造真实数据（每类都造，让字段真的出现）
pid = req("POST", "/persons", {"name": "文档门禁甲", "gender": "male", "birthday": "1990-01-02"})["id"]
open(f"{work}/lab.txt", "w").write("姓名：钱八\n检查项目：血常规\n白细胞计数 12.3 10^9/L 偏高\n")
open(f"{work}/rx.txt", "w").write("处方\n阿莫西林胶囊 0.5g bid\n")
open(f"{work}/dx.txt", "w").write("姓名：钱八\n出院记录\n主要诊断：2型糖尿病(E11.9)\n")
for f in ("lab.txt", "rx.txt", "dx.txt"):
    upload(pid, f"{work}/{f}")
req("POST", f"/persons/{pid}/entries/self-measurement",
    {"values": [{"analyte_key": "bp_systolic", "value": 128, "unit": "mmHg"}],
     "taken_at": "2026-10-20T08:30:00+08:00"})
req("POST", f"/persons/{pid}/entries/note", {"text": "门禁笔记"})
req("POST", f"/persons/{pid}/entries/profile-event",
    {"kind": "flare", "package": "sle", "at": "2026-10-19", "payload": {"note": "x"}})
docs = req("GET", f"/persons/{pid}/documents")["items"]
sha = docs[0]["content_hash"]
detail = req("GET", f"/persons/{pid}/documents/{sha}")
ext_body = json.dumps({"labs": [{"name": "白细胞计数", "value": "12.3", "unit": "10^9/L"}]}, ensure_ascii=False)
ext = req("PUT", f"/persons/{pid}/documents/{sha}/extraction",
          {"result_json": ext_body, "backend": "gate", "model_version": "g1", "mode": "text"})
up = upload(pid, f"{work}/lab.txt")

real = {
    ("GET", "/healthz"): req("GET", "/healthz"),
    ("GET", "/capabilities"): req("GET", "/capabilities"),
    ("GET", "/persons"): req("GET", "/persons"),
    ("POST", "/persons"): req("POST", "/persons", {"name": "文档门禁乙", "gender": "female"}),
    ("GET", "/persons/{pid}/timeline"): req("GET", f"/persons/{pid}/timeline"),
    ("GET", "/persons/{pid}/search"): req("GET", f"/persons/{pid}/search?q=" + urllib.parse.quote("白细胞")),
    ("GET", "/persons/{pid}/labs"): req("GET", f"/persons/{pid}/labs"),
    ("GET", "/persons/{pid}/medications"): req("GET", f"/persons/{pid}/medications"),
    ("GET", "/persons/{pid}/conditions"): req("GET", f"/persons/{pid}/conditions"),
    ("GET", "/persons/{pid}/measurements"): req("GET", f"/persons/{pid}/measurements"),
    ("GET", "/persons/{pid}/documents"): req("GET", f"/persons/{pid}/documents"),
    ("GET", "/persons/{pid}/documents/{sha}"): detail,
    ("POST", "/persons/{pid}/documents"): up,
    ("POST", "/persons/{pid}/entries/self-measurement"): req(
        "POST", f"/persons/{pid}/entries/self-measurement",
        {"values": [{"analyte_key": "bp_diastolic", "value": 82, "unit": "mmHg"}]}),
    ("POST", "/persons/{pid}/entries/note"): req("POST", f"/persons/{pid}/entries/note", {"text": "门禁笔记2"}),
    ("POST", "/persons/{pid}/entries/profile-event"): req(
        "POST", f"/persons/{pid}/entries/profile-event",
        {"kind": "flare", "package": "sle", "at": "2026-10-19", "payload": {}}),
    ("PUT", "/persons/{pid}/documents/{sha}/extraction"): ext,
    ("GET", "/audit"): req("GET", "/audit"),
}

# ---- ① 正向：文档承诺的字段必须真存在
for key, (top_names, paren_names, optional) in sorted(doc.items()):
    if key not in real:
        continue
    present = deep_keys(real[key], set())
    present |= deep_values(real[key], set())
    op_path = "/api/v1" + key[1] if key[1] != "/" else "/api/v1/"
    props, required = schema_props(op_path, key[0], 200 if key[0] != "POST" else 201)
    if not props:
        props, required = schema_props(op_path, key[0], 200)
    declared_all = schema_all_props(op_schema(op_path, key[0], 200 if key[0] != "POST" else 201), set())
    # 括号里的名字：**是 OpenAPI 属性 ⇒ 必查**（子字段名）；**不是 ⇒ 枚举值**，跳过
    must = set(top_names) | {n for n in paren_names if n in declared_all}
    miss = sorted(f for f in must if f not in present and f not in optional)
    if miss:
        bug = [f for f in miss if f in required]
        opt = [f for f in miss if f not in required]
        detail_msg = []
        if bug:
            detail_msg.append(f"实现缺必填字段(OpenAPI required):{bug}")
        if opt:
            detail_msg.append(f"文档过度承诺(键可缺省但未标可选):{opt}")
        print(f"  FAIL doc_fields_missing {key[0]} {key[1]}: " + "; ".join(detail_msg))
        fails += 1
    else:
        print(f"  PASS doc_fields_present {key[0]} {key[1]} ({len(must)} 个承诺字段)")

# ---- ② 反向：真响应里出现的键，必须被文档覆盖
#     ⚠️ 只对**OpenAPI 声明过的属性**做反向断言——真响应里出现但 OpenAPI 没声明的键，
#     那是「实现 vs schema 漂移」（由 tests/response_shape.rs 负责），不是文档的锅。
for key, val in sorted(real.items()):
    top_names, paren_names, optional = doc.get(key, (set(), set(), set()))
    op_path = "/api/v1" + key[1] if key[1] != "/" else "/api/v1/"
    code = 201 if (key[0] == "POST" and key[1] == "/persons") else 200
    declared = schema_all_props(op_schema(op_path, key[0], code), set())
    if not declared:
        declared = schema_all_props(op_schema(op_path, key[0], 200), set())
    present = deep_keys(val, set())
    known = top_names | paren_names | optional
    extra = sorted(k for k in present if k in declared and k not in known)
    if extra:
        print(f"  FAIL doc_missing_fields {key[0]} {key[1]}: {extra}")
        fails += 1
    else:
        print(f"  PASS doc_covers_response {key[0]} {key[1]}")

# ---- ③ 端点覆盖
#     ⚠️ `--print-rest-manifest` 是 **dry-run** 但**仍要求 EHR_HOME 可用**（D-039 记录过的坑）
#     ⇒ 显式给它一个临时 home，否则子进程报 "EHR_HOME not accessible" ⇒ 拿不到 JSON。
env = dict(os.environ)
env.setdefault("EHR_HOME", work)
mani = json.loads(subprocess.run([BIN, "--print-rest-manifest"], capture_output=True, text=True, env=env).stdout)
ops = set()
for pl in mani["paths_list"]:
    m, p = pl.split(" ", 1)
    p = p.replace("/api/v1", "", 1) or "/"
    ops.add((m, p))
undoc = sorted(o for o in ops if o not in doc)
if undoc:
    print(f"  FAIL doc_missing_endpoints: {undoc}")
    fails += 1
else:
    print(f"  PASS doc_covers_endpoints ({len(ops)} 个 operation 全有文档行)")
extra_doc = sorted(k for k in doc if k not in ops and k[1] not in ("/", "/openapi.json", "/mcp"))
if extra_doc:
    print(f"  FAIL doc_has_unknown_endpoints: {extra_doc}")
    fails += 1
else:
    print("  PASS doc_has_no_unknown_endpoints")

print(f"[证据] doc_contract_check fails={fails}")
sys.exit(1 if fails else 0)
