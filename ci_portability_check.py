#!/usr/bin/env python3
"""medme-ehr · T28/T29「CI 可移植性」门：门禁脚本（**含 Python 内层实现**）不许写死机器专属路径。

为什么需要它（PLAN 未销账项①「统一入口尚未接入 CI」的正面处置）：
  要把 `run-all-gates.sh` 放进 GitHub Actions，就必须先回答：
  **这些脚本在别人的机器上跑得起来吗？**
  T28 审计实测到 3 类真缺陷：
    ① 硬编码机器专属绝对路径（`/home/lbb/...`）⇒ CI runner 上必然跑不起来；
    ② 产物路径有**两份实现**（debug 侧走 $CARGO_TARGET_DIR，release 侧写死 $HOME/...）
       ⇒ 改一处忘一处 ⇒ 门禁静默跑旧产物；
    ③ 启动的产物 ≠ 构建的产物（`auth-e2e.sh` 用相对路径 ./target/debug/...）
       ⇒ 它测的是另一份（可能很旧的）产物，而门禁是绿的。

T29 补的洞（PLAN 未销账项⑮「判据未覆盖 Python 内层实现」）：
  T28 的门只扫 `*.sh` ⇒ **每道门的 Python 内层实现完全在覆盖之外**。
  实测负向证据：往 `skill_doc_field_check.py` 注入 `/home/lbb/medme-shared-target/...`
  ⇒ 门**照样绿** ⇒ 门有洞。且这个洞**藏着 2 处真缺陷**（见「真缺陷」段）。

判据（五条独立方向，缺一即算不通过）：
  A **无机器专属绝对路径**：任何受检文件里不得出现 `/home/<用户名>/` 这类绑定到某台机器的路径。
    放行 `$HOME/...`、`${HOME}/...`、`/tmp/...`（可移植写法）；**容器内路径**（/home/ehr/）
    是镜像内固定路径（跨机器一致）⇒ 不算缺陷。
  B **产物路径单一来源**：不许自己拼 `.../debug/ehr-api` / `.../release/ehr-api`
    ⇒ 必须用 `$EHR_BIN` / `$EHR_DBG_BIN` / `$EHR_REL_BIN`（由 ci/env.sh 统一提供）。
  C **起真进程的门禁都 source 唯一来源**：每个起真进程的脚本必须 source `ci/env.sh`
    ⇒ 漏 source ⇒ 拿不到统一路径 ⇒ 命中即红。
  D **产物根目录不得有第二份实现**：除 `ci/env.sh`（唯一来源）外，任何文件都不得写死
    `medme-shared-target` 这个默认根 ⇒ 否则「门禁跑哪份产物」又变成两份实现。
  E **产物路径变量不得自带默认值**：`.py` 里不得写 `os.environ.get("EHR_BIN", <默认路径>)`
    ⇒ 那是**绕过唯一来源的第二份实现**（且默认值往往指向某台机器上的旧产物）
    ⇒ 必须「没注入就明确失败」，不许悄悄用兜底值。

退出码：0 = 全通过；1 = 有不通过项（逐条打印）。
"""
import ast
import os
import re
import sys
import tokenize

ROOT = os.path.dirname(os.path.abspath(__file__))

# 被检查的文件：仓库根的门禁脚本 + 根的 Python 内层实现 + ci/ 下的脚本 + SKILL 随附脚本
#   + **CI workflow**（T29：workflow 也是「一份实现」的载体，它写死产物根目录
#     同样会让「门禁跑哪份产物」分裂成两份 —— 实测 ci-gates.yml 就写死过一份）。
SCRIPT_GLOBS = [("", "*.sh"), ("", "*.py"), ("ci", "*.sh"), ("ci", "*.py"),
              # T31：tools/ 下的取证脚本也是「一份实现」的载体（它调 REST/产物）
              ("tools", "*.py")]
EXTRA_FILES = [os.path.join("skills", "medme-ehr", "scripts", "ehr.sh")]
WORKFLOW_GLOB = os.path.join(".github", "workflows", "*.yml")

# 「起真进程的门禁」= 必须拿到产物路径的那些脚本（需要 source 唯一来源）
NEEDS_ENV = [
    "mcp-e2e.sh", "mcp-contract-check.sh", "doc-contract-check.sh",
    "skill-contract-check.sh", "skill-rest-check.sh", "skill-doc-field-check.sh",
    "auth-e2e.sh", "entries-e2e.sh", "conds-e2e.sh", "meds-e2e.sh",
    "audit-e2e.sh", "api-doc-check.sh", "web-e2e.sh", "ehtest.sh",
    "rebuild-e2e.sh", "rebuild-lock-e2e.sh",
    # T31 新增：抽取路径门禁（体检报告列布局 labs=0）也起真进程 ⇒ 同样必须经唯一来源
    "extraction-e2e.sh",
    # T29 新增：新鲜度门要拿**两份**产物路径（local + shared）⇒ 也必须经唯一来源
    "artifact-freshness-check.sh",
    # T35 新增：第三方 MCP 互通门也起真进程（要产物路径）⇒ 同样必须经唯一来源
    "mcp-interop-e2e.sh",
    # T36 新增：TS 面第三方互通门也起真进程（要产物路径）⇒ 同样必须经唯一来源
    "mcp-interop-ts-e2e.sh",
    # T38 新增：registry 并发写门也起真进程（要产物路径）⇒ 同样必须经唯一来源
    "registry-concurrency-e2e.sh",
    # T40 新增：写侧多轮门也起真进程（要产物路径）⇒ 同样必须经唯一来源
    "write-multiround-e2e.sh",
    # T41 新增：浏览器级冒烟门也起真进程（要产物路径）⇒ 同样必须经唯一来源
    "web-browser-e2e.sh",
]

# 产物路径的「自己拼」形态：含 debug/ehr-api 或 release/ehr-api 的**路径 token**
# ⚠️ T29 修正语义：原先用「token 里含 debug/ehr-api 就红」⇒ 会把
#   `$LOCAL_TARGET/debug/ehr-api`（**变量派生**、来自唯一来源 ci/env.sh）也判红
#   ⇒ 那是**假红**（D-011 家族：门红了先怀疑门）。
#   ⇒ 真正的缺陷是「**字面量**产物路径」= 没有任何变量引用的路径 token
#   （如 `~/medme-shared-target/release/ehr-api`、`./target/debug/ehr-api`）。
#   变量派生的写法（$EHR_BIN / $LOCAL_TARGET/debug/ehr-api）由唯一来源保证 ⇒ 放行。
ARTIFACT_PATH_RE = re.compile(r"[^\s\"'()]*\b(?:debug|release)/ehr-api\b")
# 路径 token 里出现变量引用（$VAR / ${VAR}）⇒ 变量派生 ⇒ 不算「自己拼」
VAR_REF_RE = re.compile(r"\$\{?[A-Za-z_][A-Za-z0-9_]*\}?")

# T29 判据 D：默认产物根目录名（只允许出现在唯一来源 ci/env.sh 里）
TARGET_ROOT_NAME = "medme-shared-target"

# T29 判据 E：产物路径变量自带兜底默认值 = 第二份实现
ENV_WITH_DEFAULT_RE = re.compile(
    r"os\.environ\.get\(\s*[\"'](EHR_BIN|EHR_DBG_BIN|EHR_REL_BIN|CARGO_TARGET_DIR|EHR_LOCAL_TARGET_DIR)[\"']\s*,")

# **豁免**：唯一来源自身（ci/env.sh）就是产物路径的定义处，不是「又一份实现」
EXEMPT_ARTIFACT = {"ci/env.sh"}

# **豁免（判据 D）**：本门自己必须**写出**那个根目录名才能检出它 ⇒ 不豁免就会自己红
EXEMPT_TARGET_ROOT = {"ci/env.sh", "ci_portability_check.py"}

# 容器内路径不是「宿主机专属路径」：它是**镜像内**的固定路径（deploy/Dockerfile 定的），
# 在任何机器上都一样 ⇒ 不算 portability 缺陷。判据只看**宿主机**路径。
# 容器内约定的根目录：/home/ehr（deploy/docker-compose.yml 的 EHR_HOME）
CONTAINER_HOME = "/home/ehr"


def is_container_path(path):
    """判断一个 /home/... 路径是不是**容器内**路径（镜像内固定，跨机器一致）。"""
    return path == CONTAINER_HOME or path.startswith(CONTAINER_HOME + "/")


def files():
    out = []
    for sub, pat in SCRIPT_GLOBS:
        d = os.path.join(ROOT, sub) if sub else ROOT
        if not os.path.isdir(d):
            continue
        for f in sorted(os.listdir(d)):
            if f.endswith(pat.split("*")[-1]):
                out.append(os.path.join(sub, f) if sub else f)
    for rel in EXTRA_FILES:
        if os.path.exists(os.path.join(ROOT, rel)):
            out.append(rel)
    # CI workflow（YAML 的注释以 # 开头 ⇒ 复用 shell 的 is_comment 规则即可）
    wfdir = os.path.join(ROOT, ".github", "workflows")
    if os.path.isdir(wfdir):
        for f in sorted(os.listdir(wfdir)):
            if f.endswith((".yml", ".yaml")):
                out.append(os.path.join(".github", "workflows", f))
    return out


def read(rel):
    with open(os.path.join(ROOT, rel), encoding="utf-8") as fh:
        return fh.read().split("\n")


def is_comment(line):
    """shell 风格：整行以 # 开头 = 注释。"""
    return line.strip().startswith("#")


def py_noncode_lines(rel):
    """返回 Python 文件里「不算代码」的行号集合（注释行 + docstring 行）。

    为什么需要它（T29 的判据基础）：
      `is_comment` 只认 `#` 开头 ⇒ 对 Python 的**模块 docstring**（`\"\"\"...\"\"\"`）无效
      ⇒ docstring 里写的历史说明（`/home/lbb/...`）会被误判成缺陷
      ⇒ 门就会**假红**（D-011 家族：门红了先怀疑门）。
      ⇒ 这里用 ast 精确取 docstring 节点 + tokenize 取 COMMENT，只排除「真·非代码」。
    """
    skip = set()
    path = os.path.join(ROOT, rel)
    with open(path, "rb") as fh:
        raw = fh.read()
    # --- 注释行（tokenize）---
    try:
        for tok in tokenize.tokenize(io_bytes(raw)):
            if tok.type == tokenize.COMMENT:
                skip.add(tok.start[0])
    except Exception:
        pass
    # --- docstring（ast）---
    try:
        tree = ast.parse(raw.decode("utf-8"))
    except Exception:
        return skip
    for node in ast.walk(tree):
        if isinstance(node, (ast.Module, ast.FunctionDef, ast.AsyncFunctionDef, ast.ClassDef)):
            body = getattr(node, "body", None)
            if body and isinstance(body[0], ast.Expr) and isinstance(body[0].value, ast.Constant) \
                    and isinstance(body[0].value.value, str):
                s = body[0].value
                for ln in range(s.lineno, (s.end_lineno or s.lineno) + 1):
                    skip.add(ln)
    return skip


def py_code_lines(rel):
    """返回 {行号: 该行的**代码部分**}（去掉行尾注释、去掉纯注释行与 docstring 行）。

    ⚠️ T31 修掉的**真缺陷**（负向实测发现）：
      原实现（`py_noncode_lines`）用 tokenize 的 COMMENT token 行号**整行**跳过，
      于是**代码行尾挂的注释**（`X = "/home/lbb/..."  # 负向注入`）会把**整行**标成
      「非代码」⇒ 那一行里的机器专属路径/产物路径**逃过所有判据** ⇒ 门是空的。
      ⇒ 改为**逐行**取「该行注释起点列之前的代码部分」：
        · 注释在代码**之前**（整行只有注释）⇒ 该行不参与判据（纯说明，保留原语义）；
        · 注释在代码**之后**（行尾注释）⇒ 只判**代码部分**（注释文字不算事实）；
        · docstring 行 ⇒ 仍按 ast 精确排除（它是文档，不是可执行事实）。
      ⇒ **判据判的是「事实」而不是「提到事实的字符串」**（D-046 纪律的延续）。
    """
    out = {}
    path = os.path.join(ROOT, rel)
    with open(path, "rb") as fh:
        raw = fh.read()
    try:
        toks = list(tokenize.tokenize(io_bytes(raw)))
    except Exception:
        return None
    text = raw.decode("utf-8").split("\n")
    # --- docstring 行（ast）---
    doc_lines = set()
    try:
        tree = ast.parse(raw.decode("utf-8"))
    except Exception:
        return None
    for node in ast.walk(tree):
        if isinstance(node, (ast.Module, ast.FunctionDef, ast.AsyncFunctionDef, ast.ClassDef)):
            body = getattr(node, "body", None)
            if body and isinstance(body[0], ast.Expr) and isinstance(body[0].value, ast.Constant) \
                    and isinstance(body[0].value.value, str):
                s = body[0].value
                for ln in range(s.lineno, (s.end_lineno or s.lineno) + 1):
                    doc_lines.add(ln)
    # --- 每行的注释起点列 ---
    comment_col = {}
    for tok in toks:
        if tok.type == tokenize.COMMENT:
            comment_col.setdefault(tok.start[0], tok.start[1])
    # --- 每行第一个有效 token 的起点列（判断该行「是不是只有注释」）---
    first_code_col = {}
    for tok in toks:
        if tok.type in (tokenize.COMMENT, tokenize.ENDMARKER, tokenize.NL,
                        tokenize.NEWLINE, tokenize.INDENT, tokenize.DEDENT):
            continue
        ln = tok.start[0]
        if ln not in first_code_col:
            first_code_col[ln] = tok.start[1]
    for ln, col in first_code_col.items():
        if ln in doc_lines:
            continue
        cc = comment_col.get(ln)
        if cc is not None and cc <= col:
            continue  # 整行只有注释 ⇒ 不参与判据
        line = text[ln - 1] if ln - 1 < len(text) else ""
        if cc is not None:
            line = line[:cc]  # 去掉行尾注释 ⇒ 只判代码部分
        out[ln] = line
    return out


def io_bytes(raw):
    """tokenize.tokenize 需要 readline 迭代器。"""
    import io
    return io.BytesIO(raw).readline


def main():
    fails = 0
    all_files = files()
    print("[证据] 受检文件 %d 个：%s" % (len(all_files), " ".join(all_files)))

    for rel in all_files:
        lines = read(rel)
        is_py = rel.endswith(".py")
        skip = py_noncode_lines(rel) if is_py else set()
        # T31：Python 侧改为**逐行取代码部分**（去行尾注释）⇒ 行尾注释不再让整行逃逸
        py_code = py_code_lines(rel) if is_py else None

        for i, line in enumerate(lines, 1):
            if is_py:
                if py_code is not None:
                    if i not in py_code:
                        continue
                    line = py_code[i]
                elif i in skip:
                    continue
            else:
                if is_comment(line):
                    continue

            # ---- 判据 A：无机器专属绝对路径 ----
            m = re.search(r"/home/([A-Za-z0-9_.-]+)/", line)
            if m and not is_container_path(m.group(0)):
                print("FAIL %s:%d 机器专属绝对路径 /home/%s/ ⇒ CI runner 上跑不起来"
                      % (rel, i, m.group(1)))
                print("      %s" % line.strip())
                fails += 1

            # ---- 判据 B：产物路径单一来源 ----
            if "ci/env.sh" in line or rel in EXEMPT_ARTIFACT:
                continue
            for tok in ARTIFACT_PATH_RE.findall(line):
                if VAR_REF_RE.search(tok):
                    continue  # 变量派生（来自唯一来源）⇒ 放行
                print("FAIL %s:%d 自己拼了产物路径 ⇒ 产物路径必须只有一份实现（ci/env.sh）"
                      % (rel, i))
                print("      %s" % line.strip())
                fails += 1

            # ---- 判据 D：产物根目录不得有第二份实现 ----
            if rel in EXEMPT_ARTIFACT or rel in EXEMPT_TARGET_ROOT:
                continue
            if TARGET_ROOT_NAME in line:
                print("FAIL %s:%d 写死了产物根目录 %s ⇒ 默认值只允许存在于唯一来源 ci/env.sh"
                      % (rel, i, TARGET_ROOT_NAME))
                print("      %s" % line.strip())
                fails += 1

            # ---- 判据 E：产物路径变量不得自带兜底默认值 ----
            if is_py:
                mm = ENV_WITH_DEFAULT_RE.search(line)
                if mm:
                    print("FAIL %s:%d 产物路径变量 %s 自带兜底默认值 ⇒ 绕过唯一来源的第二份实现"
                          "（没注入就应明确失败）" % (rel, i, mm.group(1)))
                    print("      %s" % line.strip())
                    fails += 1

        # ---- 判据 C：起真进程的门禁必须 source 唯一来源 ----
        # ⚠️ T29 修正：原先只判「文件里出现过 ci/env.sh 这个字符串」⇒ **注释里提一句也算数**
        #   ⇒ 负向实测：把 `artifact-freshness-check.sh` 的 source 行删掉（只留注释里的
        #   「唯一来源（ci/env.sh）」）⇒ 门**照样绿** ⇒ 判据 C 是空的。
        #   ⇒ 改为**只看真正的 source 语句**（非注释行，且以 `.`/`source` 开头）。
        base = os.path.basename(rel)
        if base in NEEDS_ENV:
            real_sources = [ln for ln in lines
                            if not is_comment(ln) and "ci/env.sh" in ln
                            and re.search(r"(^|\s)(\.|source)\s", ln)]
            if not real_sources:
                print("FAIL %s 起了真进程却没 source ci/env.sh ⇒ 拿不到统一产物路径"
                      "（注释里提到不算）" % rel)
                fails += 1

    print("[证据] ci-portability-check.sh fails=%d" % fails)
    return 1 if fails else 0


if __name__ == "__main__":
    sys.exit(main())
