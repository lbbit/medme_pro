#!/bin/bash
# medme-ehr · 接口文档漂移门禁（D-025 / 目标验收⑤：docs/API.md 人类可读接口文档）
#
# 判据：仓库内 docs/API.md 与「运行时 OpenAPI 渲染出的文档」**逐字一致**。
# 为什么要有这道门：手写接口文档与代码漂移是本项目经典漏账形态（D-016/D-011 教训）。
# 文档若只由人维护，读者（人/agent）无法知道它已过期；由生成器产出 + 门禁断言逐字一致，
# 才能保证「文档 = 事实」而不是「文档 = 曾经的事实」。
#
# 用法: ./api-doc-check.sh   （不需要真服务；走 dry-run 打印模式）
set -u
ROOT="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT"
export PATH="$HOME/.cargo/bin:$PATH"
. "$ROOT/ci/env.sh"
fail=0
ok(){ echo "PASS $1"; }
bad(){ echo "FAIL $1"; fail=$((fail+1)); }

# 优先用已构建二进制；没有就现场构建（零新依赖，只用本仓库代码）
BIN="$EHR_DBG_BIN"
if [ ! -x "$BIN" ]; then
  echo "… building ehr-api (first run)"
  (cd ehr-api && cargo build 2>&1 | tail -5) || { echo "FAIL build"; exit 1; }
fi
[ -x "$BIN" ] || { echo "FAIL no binary at $BIN"; exit 1; }

HOME_TMP=$(mktemp -d)
trap 'rm -rf "$HOME_TMP"' EXIT
# EHR_HOME 必须存在（dry-run 校验 home 可达但不创建）——用临时空目录，纯 dry-run 不落任何状态
export EHR_HOME="$HOME_TMP/home"
mkdir -p "$EHR_HOME"

# --write：就地重新生成 docs/API.md（生成物入库，仓库里的那份就是唯一一份）
if [ "${1:-}" = "--write" ]; then
  "$BIN" --print-api-doc > docs/API.md 2> "$HOME_TMP/err.log" || { echo "FAIL generate"; exit 1; }
  echo "wrote docs/API.md ($(wc -c < docs/API.md) bytes)"
  exit 0
fi

# A1 dry-run 打印文档必须成功退出、stdout 非空、stderr 有 [证据] 行（D1 惯例）
"$BIN" --print-api-doc > "$HOME_TMP/gen.md" 2> "$HOME_TMP/err.log"
rc=$?
[ "$rc" = "0" ] && ok "A1 --print-api-doc exit=0" || bad "A1 exit=$rc"
BYTES=$(wc -c < "$HOME_TMP/gen.md")
[ "$BYTES" -gt 3000 ] && ok "A1 stdout bytes=$BYTES" || bad "A1 stdout bytes=$BYTES"
grep -q '\[证据\].*print-api-doc' "$HOME_TMP/err.log" && ok "A1 stderr 有 [证据] 行" || bad "A1 stderr 缺 [证据] 行"
# 证据行走 stderr ⇒ stdout 必须是纯 Markdown（无 [证据] 前缀污染）
# 只看行首（文档正文里会提到「[证据] 行」字样，那是说明不是污染）
grep -q '^\[证据\]' "$HOME_TMP/gen.md" && bad "A1 stdout 被证据行污染" || ok "A1 stdout 无证据行污染"

# A2 未知参数必须被拒（D4-b：参数写错≠参数生效）
"$BIN" --print-api-dox > /dev/null 2>&1
[ $? -ne 0 ] && ok "A2 未知参数非零退出" || bad "A2 未知参数被接受"

# A3 仓库内 docs/API.md 必须存在且与生成物逐字一致
if [ ! -f docs/API.md ]; then
  bad "A3 docs/API.md 不存在（先跑 ./api-doc-check.sh --write 或 ehr-api --print-api-doc > docs/API.md）"
else
  if diff -u docs/API.md "$HOME_TMP/gen.md" > "$HOME_TMP/diff.txt" 2>&1; then
    ok "A3 docs/API.md 与生成物逐字一致（$(wc -c < docs/API.md) bytes）"
  else
    bad "A3 docs/API.md 与生成物漂移（diff 见下）"
    head -40 "$HOME_TMP/diff.txt"
  fi
fi

# A4 文档必须覆盖 OpenAPI 全部端点（用生成物自带的「端点一览」表逐条核对 openapi.json）
"$BIN" --print-openapi > "$HOME_TMP/openapi.json" 2>/dev/null
python3 - "$HOME_TMP/openapi.json" "$HOME_TMP/gen.md" <<'PY'
import json,sys
doc=json.load(open(sys.argv[1])); md=open(sys.argv[2],encoding='utf-8').read()
missing=[]; n=0
for p,item in doc["paths"].items():
    for m,op in item.items():
        if not isinstance(op,dict): continue
        n+=1
        if f"`{m.upper()} {p}`" not in md: missing.append(f"{m.upper()} {p}")
print("openapi operations=%d  missing_in_doc=%d" % (n,len(missing)))
for x in missing: print("MISS",x)
sys.exit(1 if missing else 0)
PY
[ $? -eq 0 ] && ok "A4 文档覆盖 OpenAPI 全部端点" || bad "A4 文档漏端点"

# A5 文档必须显式标注 stub 端点（D5：状态码不许粉饰）
# D-021：「stub→501」字面量**不再手抄**——从实现派生清单取 `stub_mark`
# （同一事实两份实现的又一处：改标记串时不必同步改本脚本）。
STUB_MARK=$("$BIN" --print-rest-manifest 2>/dev/null | python3 -c 'import json,sys;print(json.load(sys.stdin)["stub_mark"])')
python3 - "$HOME_TMP/openapi.json" "$HOME_TMP/gen.md" "$STUB_MARK" <<'PY'
import json,sys
doc=json.load(open(sys.argv[1])); md=open(sys.argv[2],encoding='utf-8').read()
mark=sys.argv[3]
stubs=[]
for p,item in doc["paths"].items():
    for m,op in item.items():
        if isinstance(op,dict) and mark in (op.get("summary") or ""): stubs.append(f"{m.upper()} {p}")
print("stub endpoints=%d" % len(stubs))
bad=[s for s in stubs if f"`{s}`" not in md or "stub" not in md]
for b in bad: print("UNMARKED",b)
sys.exit(1 if bad else 0)
PY
[ $? -eq 0 ] && ok "A5 stub 端点在文档中均被标注" || bad "A5 stub 端点未标注"

echo "fails=$fail"
exit $([ "$fail" -eq 0 ] && echo 0 || echo 1)
