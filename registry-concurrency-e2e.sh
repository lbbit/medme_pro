#!/usr/bin/env bash
# medme-ehr · T38「person registry 并发写完整性」门禁（真进程 + 真 HTTP）。
#
# 为什么需要它（T38 实测抓到的**真缺陷**，非推演）：
#   `PersonStore::append()` 原先用 `serde_json::to_writer(&mut f, ev)` 直接往真相层
#   `registry/persons.jsonl` 写 ⇒ 那**不是一次写**，而是几十次小 write syscall
#   ⇒ 两个并发建档请求的这些小写会**交错落盘** ⇒ 真相层出现「半行 + 半行」的交错垃圾。
#   实测（60 并发 POST /persons）：**59 行里 29 行是交错垃圾** ⇒ 此后
#   `GET /persons` / `/timeline` / 审计视图等**所有**读面**永久 500**
#   （真相层被写坏，且**不可自愈** —— 只有人工去修 JSONL）。
#   ⇒ 本门在**真进程 + 真 HTTP** 层复现并钉死它。
#
# 判据（缺一即红）：
#   C1 真相层每行都是**完整 JSON**（行内不破碎）—— 直接验落盘字节（不看 API 的说法）
#   C2 行数 == 并发数（没有半条、没有丢条）
#   C3 读面仍 200 且条数 == 并发数（**不**因并发写而永久 500）
#   C4 落盘的 id 集合 == 建档返回的 id 集合（无重复、无丢）
#
# 纪律（D-050 家族）：**派生的期望值不得为空** —— 并发数>0、行数>0 才允许参与比较。
#
# 用法: ./registry-concurrency-e2e.sh [PORT]   默认 18696
set -uo pipefail
PORT="${1:-18696}"
ROOT="$(cd "$(dirname "$0")" && pwd)"
. "$ROOT/ci/env.sh"
BIN="$EHR_BIN"
fails=0
note() { echo "[证据] $*"; }
ok()   { echo "  PASS $1"; }
bad()  { echo "  FAIL $1"; fails=$((fails+1)); }

[ -x "$BIN" ] || { echo "missing binary: $BIN"; exit 1; }

H="$(mktemp -d)"; mkdir -p "$H/vaults"
SRV=""
cleanup() {
  [ -n "$SRV" ] && kill "$SRV" 2>/dev/null
  [ -n "$SRV" ] && wait "$SRV" 2>/dev/null
  rm -rf "$H"
}
trap cleanup EXIT

EHR_HOME="$H" "$BIN" --addr="127.0.0.1:${PORT}" >"$H/serve.log" 2>&1 &
SRV=$!
BASE="http://127.0.0.1:${PORT}"
for i in $(seq 1 50); do curl -sf "$BASE/api/v1/healthz" >/dev/null 2>&1 && break; sleep 0.2; done
curl -sf "$BASE/api/v1/healthz" >/dev/null 2>&1 || { echo "FAIL 服务未起来"; cat "$H/serve.log"; echo "fails=$fails"; exit 1; }
note "服务已起 base=$BASE 产物=$BIN"

N="${EHR_REGISTRY_CONC:-60}"
note "并发建档数 N=$N"

# ---- 真并发：N 个 curl 后台同时打 POST /persons ----
# ⚠️ **不能用裸 `wait`**（本轮实撞）：裸 wait 会等**所有**后台作业 ⇒ 包括那个
#   一直活着的 ehr-api 服务 ⇒ 门**永远挂住**（首跑即被超时打断）。
#   ⇒ 只收集 curl 的 PID，再 `wait` 这些 PID。
# ⚠️ 每个响应写**各自的文件**（本轮实撞）：并发 `>>` 追加到**同一个**文件 ⇒
#   各 curl 的输出会互相交错 ⇒ 拼出来的行不是合法 JSON ⇒ 「建档返回的 id 集合」
#   只收到 15/60 个 ⇒ 那条判据就成了**假弱**（看起来在核对，实际大部分没核到）。
#   ⇒ 一请求一文件 ⇒ 零交错。
CONC_DIR="$H/created"
mkdir -p "$CONC_DIR"
CURL_PIDS=()
for i in $(seq 1 "$N"); do
  ( curl -s -X POST "$BASE/api/v1/persons" -H 'content-type: application/json' \
      -d "{\"name\":\"并发建档-$i\"}" -o "$CONC_DIR/resp-$i.json" ) &
  CURL_PIDS+=($!)
done
for p in "${CURL_PIDS[@]}"; do wait "$p" 2>/dev/null; done

# ---- C1/C2：真相层落盘字节逐行验（不看 API 的说法）----
python3 - "$H/registry/persons.jsonl" "$N" <<'PY' && ok "C1+C2 真相层每行完整 JSON 且行数==$N" || bad "C1+C2 真相层完整性"
import json,sys
path,n=sys.argv[1],int(sys.argv[2])
raw=open(path,encoding='utf-8').read()
lines=[l for l in raw.split('\n') if l.strip()]
# D-050：派生的期望值不得为空 —— 行数为 0 时必须红（不许「0==0」假绿）
if not lines:
    print("FAIL 真相层为空（期望 %d 行）"%n); sys.exit(1)
bad_lines=[]
for i,l in enumerate(lines):
    try: json.loads(l)
    except Exception as e: bad_lines.append((i+1,str(e),l[:160]))
if bad_lines:
    print("FAIL %d/%d 行是交错/损坏的垃圾（真相层被并发写打坏）:"%(len(bad_lines),len(lines)))
    for ln,e,raw_ in bad_lines[:5]:
        print("  line %d: %s\n    raw: %s"%(ln,e,raw_))
    sys.exit(1)
if len(lines)!=n:
    print("FAIL 行数 %d != 并发数 %d（有半条或丢条）"%(len(lines),n)); sys.exit(1)
print("lines=%d intact=ok"%len(lines))
PY

# ---- C3/C4：读面仍可用 + id 集合一致 ----
python3 - "$BASE" "$CONC_DIR" "$N" <<'PY' && ok "C3+C4 读面 200 且条数/id 集合与建档一致" || bad "C3+C4 读面一致性"
import json,sys,glob,os,urllib.request
base,outdir,n=sys.argv[1],sys.argv[2],int(sys.argv[3])
try:
    with urllib.request.urlopen(base+"/api/v1/persons",timeout=30) as r:
        code=r.status; listed=json.load(r)
except Exception as e:
    print("FAIL 读面不可用（并发写把真相层打坏了？）: %r"%e); sys.exit(1)
if code!=200:
    print("FAIL 读面 http=%s"%code); sys.exit(1)
if not listed:
    print("FAIL 读面返回空（D-050：派生的期望值不得为空）"); sys.exit(1)
if len(listed)!=n:
    print("FAIL 读面条数 %d != 并发数 %d"%(len(listed),n)); sys.exit(1)
# 建档返回的 id 集合 vs 读面 id 集合（每个响应一个文件 ⇒ 零交错）
created=set(); unparsed=0
for f in glob.glob(os.path.join(outdir,"resp-*.json")):
    try: d=json.load(open(f,encoding='utf-8'))
    except Exception: unparsed+=1; continue
    if isinstance(d,dict) and d.get("id"): created.add(d["id"])
if unparsed:
    print("FAIL %d 个建档响应不是合法 JSON"%unparsed); sys.exit(1)
if len(created)!=n:
    print("FAIL 建档返回的 id 数 %d != 并发数 %d（建档响应缺失/重复）"%(len(created),n)); sys.exit(1)
ids={p["id"] for p in listed}
if not created.issubset(ids):
    print("FAIL 建档返回的 id 有 %d 个不在读面里（丢条）"%len(created-ids)); sys.exit(1)
if len(ids)!=len(listed):
    print("FAIL 读面出现重复 id"); sys.exit(1)
print("listed=%d created_ids=%d unique=%d"%(len(listed),len(created),len(ids)))
PY

note "真相层: $H/registry/persons.jsonl"
echo "fails=$fails"
[ "$fails" -eq 0 ]
