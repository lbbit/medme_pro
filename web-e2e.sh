#!/bin/bash
# medme-ehr · Web 管理端最小版门禁（D-016）
# 判据：容器内/起栈后的真实服务上，页面 HTML 可取 + JS 语法合法 + JS 读用的每个字段名
#      在真实 API 响应里逐个存在（字段名对不上=页面看着正常但数据全空）。
# 用法: base=http://127.0.0.1:8585 web-e2e.sh
set -u
BASE="${base:-http://127.0.0.1:8585}"
fail=0
ok(){ echo "PASS $1"; }
bad(){ echo "FAIL $1"; fail=$((fail+1)); }

# W1 首页 200 + text/html
CT=$(curl -s -o /tmp/webe2e.html -w '%{content_type}' "$BASE/")
CODE=$(curl -s -o /dev/null -w '%{http_code}' "$BASE/")
[ "$CODE" = "200" ] && ok "W1 http=200" || bad "W1 http=$CODE"
case "$CT" in text/html*) ok "W1 content_type=$CT";; *) bad "W1 content_type=$CT";; esac
BYTES=$(wc -c < /tmp/webe2e.html)
[ "$BYTES" -gt 2000 ] && ok "W1 bytes=$BYTES" || bad "W1 bytes=$BYTES"

# W2 JS 语法（node 不可用则跳过并明说，不假装通过）
python3 - <<'PY'
import re,sys
h=open('/tmp/webe2e.html',encoding='utf-8').read()
js=re.search(r'<script>(.*)</script>',h,re.S)
open('/tmp/webe2e.js','w',encoding='utf-8').write(js.group(1) if js else '')
PY
if command -v node >/dev/null 2>&1; then
  node --check /tmp/webe2e.js && ok "W2 js syntax" || bad "W2 js syntax"
else
  echo "SKIP W2 js syntax (node 不存在；已用容器内 playwright 镜像 node --check 代偿，见 DECISIONS D-016)"
fi

# W3 端到端业务流：建档 → 上传 → 时间线/文档/指标/检索 → 字段交叉核对
PID=$(curl -s -XPOST "$BASE/api/v1/persons" -H 'content-type: application/json' \
      -d '{"name":"web-e2e·自动门禁"}' | python3 -c 'import json,sys;print(json.load(sys.stdin)["id"])')
[ -n "$PID" ] && ok "W3 建档 pid=$PID" || { bad "W3 建档失败"; echo "fails=$fail"; exit 1; }

SAMPLE=$(ls /home/lbb/medme-ehr/upstream-medme/examples/sample-records/*血常规*.txt 2>/dev/null | head -1)
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