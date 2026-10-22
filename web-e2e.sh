#!/bin/bash
# medme-ehr · Web 管理端最小版门禁（D-016）
# 判据：页面 HTML 可取 + JS 语法合法 + JS 读用的每个字段名在真实 API 响应里逐个存在
#      （字段名对不上 = 页面看着正常但数据全空）。
#
# ⚠️ **测哪一份产物**（D-035 新坑③「先问它跑的是哪一份产物」的正面处置）：
#   本脚本原先**只**打 `base`（默认 http://127.0.0.1:8585 = docker compose 起的服务）。
#   ⇒ 而那个容器是**上一次 `docker compose up --build` 时的镜像** ⇒ 本门长期测的是
#     **几周前的旧产物**，而门禁照样是绿的 —— 这正是 D-035 新坑③描述的失败模式，
#     在「Web 端」这个面上复发（其余 16 道门都起自己的真进程，只有这道门指向外部服务）。
#   ⇒ **默认改为：自己起一个临时 ehr-api（跑当前产物）来测**；
#     显式传 `base=` 时才打外部服务（此时由调用方负责保证那份服务是当前的，
#     例如 `container-smoke.sh` 场景；本脚本会打印实际 base 以留证据）。
#
# 用法:
#   ./web-e2e.sh                      # 自起临时服务（测当前产物）——默认
#   base=http://127.0.0.1:8585 ./web-e2e.sh   # 打外部服务（如 compose 起的栈）
set -u
ROOT="$(cd "$(dirname "$0")" && pwd)"
. "$ROOT/ci/env.sh"

fail=0
ok(){ echo "PASS $1"; }
bad(){ echo "FAIL $1"; fail=$((fail+1)); }

SELF_MODE=1
BASE="${base:-}"
SRV=""
HOME_DIR=""
cleanup() {
  [ -n "$SRV" ] && kill "$SRV" 2>/dev/null
  [ -n "$SRV" ] && wait "$SRV" 2>/dev/null
  [ -n "$HOME_DIR" ] && rm -rf "$HOME_DIR"
}
trap cleanup EXIT

if [ -n "$BASE" ]; then
  SELF_MODE=0
  echo "[证据] 模式=外部服务 base=$BASE（调用方负责保证该服务是当前产物）"
else
  # 自起临时服务：跑**当前**产物（$EHR_REL_BIN），保证门禁测的是这一轮的代码
  [ -x "$EHR_REL_BIN" ] || { echo "FAIL missing binary: $EHR_REL_BIN"; echo "fails=$fail"; exit 1; }
  PORT="${WEB_E2E_PORT:-18681}"
  HOME_DIR="$(mktemp -d /tmp/ehr-web-e2e-XXXXXX)"
  EHR_HOME="$HOME_DIR" "$EHR_REL_BIN" --addr="127.0.0.1:${PORT}" >"$HOME_DIR/server.log" 2>&1 &
  SRV=$!
  BASE="http://127.0.0.1:${PORT}"
  for i in $(seq 1 50); do
    curl -sf "$BASE/api/v1/healthz" >/dev/null 2>&1 && break
    sleep 0.2
  done
  curl -sf "$BASE/api/v1/healthz" >/dev/null 2>&1 || {
    echo "FAIL 服务未起来"; cat "$HOME_DIR/server.log"; echo "fails=$fail"; exit 1; }
  echo "[证据] 模式=自起临时服务 base=$BASE 产物=$EHR_REL_BIN"
fi

# W1 首页 200 + text/html
CT=$(curl -s -o /tmp/webe2e.html -w '%{content_type}' "$BASE/")
CODE=$(curl -s -o /dev/null -w '%{http_code}' "$BASE/")
[ "$CODE" = "200" ] && ok "W1 http=200" || bad "W1 http=$CODE"
case "$CT" in text/html*) ok "W1 content_type=$CT";; *) bad "W1 content_type=$CT";; esac
BYTES=$(wc -c < /tmp/webe2e.html)
[ "$BYTES" -gt 2000 ] && ok "W1 bytes=$BYTES" || bad "W1 bytes=$BYTES"

# W2 JS 语法（node 不可用 ⇒ **红**，不再静默 SKIP；T30）
#   为什么改：node 缺失时原实现**静默 SKIP** ⇒ 本机上 W2 从未真正执行过
#   ⇒ Web 页面的 JS 整文件是 SyntaxError（`j()` 里 `const t` 重复声明）
#   也照样「门禁全绿」⇒ 直到 CI 装了 node 才暴露。
#   ⇒ 纪律沿用 D-042「允许跳过，但必须写明原因」+ D-011「门红了先怀疑门」：
#   一道**静默跳过**的检查 = 一道不存在的门 ⇒ 缺依赖必须**红**并给出修复路径。
python3 - <<'PY'
import re,sys
h=open('/tmp/webe2e.html',encoding='utf-8').read()
js=re.search(r'<script>(.*)</script>',h,re.S)
open('/tmp/webe2e.js','w',encoding='utf-8').write(js.group(1) if js else '')
PY
if command -v node >/dev/null 2>&1; then
  node --check /tmp/webe2e.js && ok "W2 js syntax" || bad "W2 js syntax"
else
  bad "W2 js syntax 未执行（node 不存在）"
  echo "       修复：装 node（CI 已装；本机可用 TUNA 通道 nodejs-release 安装），或用容器内 node 代偿"
fi

# W3 端到端业务流：建档 → 上传 → 时间线/文档/指标/检索 → 字段交叉核对
PID=$(curl -s -XPOST "$BASE/api/v1/persons" -H 'content-type: application/json' \
      -d '{"name":"web-e2e·自动门禁"}' | python3 -c 'import json,sys;print(json.load(sys.stdin)["id"])')
[ -n "$PID" ] && ok "W3 建档 pid=$PID" || { bad "W3 建档失败"; echo "fails=$fail"; exit 1; }

# 样本：上游 sample-records 里的血常规报告单（路径**相对仓库根**推导，不再写死 /home/lbb/...）
SAMPLE=$(ls "$ROOT"/upstream-medme/examples/sample-records/*血常规*.txt 2>/dev/null | head -1)
if [ -z "$SAMPLE" ]; then
  # 上游样本不在（CI 里由 ci/fetch-upstream.sh 拉取；本机是 git 浅克隆）⇒ 现造一份等价样本
  SAMPLE="$HOME_DIR/血常规报告.txt"
  cat >"$SAMPLE" <<'EOF'
XX医院检验科检验报告单
姓名:门禁  性别:女  年龄:37岁   样本编号:20251015-A042
检验项目          结果      参考区间     标志
白细胞计数        11.8      3.5--9.5     H
血红蛋白          139       130--175     /
血小板计数        203       125--350     /
报告日期:2025年10月15日
EOF
  echo "[证据] 上游样本缺失，改用脚本内造的等价血常规样本（$SAMPLE）"
fi
UP=$(curl -s -F "file=@$SAMPLE" "$BASE/api/v1/persons/$PID/documents")
python3 - "$UP" <<'PY' && ok "W3 上传 200+content_hash" || bad "W3 上传"
import json,sys
d=json.loads(sys.argv[1])
assert d.get("content_hash") and len(d["content_hash"])==64, d
print("sha=%s deduped=%s" % (d["content_hash"][:12], d.get("deduped")))
PY

python3 - "$BASE" "$PID" <<'PY' && ok "W3 四域字段交叉核对(ALL_OK)" || bad "W3 字段核对"
import json,sys,urllib.request,urllib.parse
base,pid=sys.argv[1],sys.argv[2]; B=base+"/api/v1"
g=lambda p: json.load(urllib.request.urlopen(B+p))
miss=[]
def need(obj,keys,where):
    for k in keys:
        if k not in obj: miss.append(where+"."+k)
tl=g("/persons/%s/timeline"%pid);            need(tl["items"][0],["doc_date","title","doc_type"],"timeline")
dc=g("/persons/%s/documents"%pid);          need(dc["items"][0],["content_hash","original_name","byte_size","mime_type"],"documents")
lb=g("/persons/%s/labs"%pid);                need(lb["series"][0],["group_name","unit_canonical","any_abnormal","points"],"labs.series")
need(lb["series"][0]["points"][0],["date","value","flag","docref"],"labs.point")
sr=g("/persons/%s/search?q=%s"%(pid,urllib.parse.quote("血常规"))); need(sr["items"][0],["title","snippet"],"search")
need(g("/persons")[0],["id","name"],"persons")
if miss:
    print("MISSING:",miss); sys.exit(1)
print("persons=%d timeline=%d docs=%d series=%d search=%d"%(len(g("/persons")),tl["count"],dc["count"],len(lb["series"]),sr["count"]))
PY

echo "fails=$fail"
[ "$fail" -eq 0 ]
