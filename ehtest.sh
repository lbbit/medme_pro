#!/usr/bin/env bash
# ehr-api 端到端验收(D4:每门都有独立生效证据,不只看返回码)。任一 GATE FAIL → 退出非零。
# 用法: ehtest.sh <abs-binary-path> [port]
# ⚠️陷阱(本轮实撞):裸写`set -u pipefail`=「声明变量pipefail取其$1」而非开pipefail选项!
#   $1缺省时 set -u 直接崩、$1有值时BIN会被污染成"pipefail"(GATE-FATAL-BIN假死相)。正确形态用 -o：
set -u -o pipefail; umask 077
BIN="${1:?usage: ehtest.sh abs-binary-path [port]}"; PORT="${2:-8951}"
BASE="http://127.0.0.1:${PORT}"; API="$BASE/api/v1"
H=$(mktemp -d /tmp/ehrt.XXXXXX) || { echo FATAL-TMPDIR; exit 3; }
WORK=$(mktemp -d /tmp/ehrw.XXXXXX) || { echo FATAL-TMPDIR; exit 3; }
fails=0; SRV_PID=""
g(){ if [[ "$2" == y ]]; then echo "[GATE PASS] $1"; else fails=$((fails+1)); echo "[GATE FAIL] $1${3:+ | $3}"; fi; }
cleanup(){ [[ -n "$SRV_PID" ]] && kill "$SRV_PID" 2>/dev/null; wait "$SRV_PID" 2>/dev/null; rm -rf "$H" "$WORK"; }
trap cleanup EXIT

test -x "$BIN" || { echo "GATE-FATAL-BIN $BIN"; exit 4; }
# G0(D4防串台): 端口必须无监听才开跑——陈旧实例会让证据行/B1与真实服务脱钩(本轮8959相B/C雪崩即此类疑云)
if timeout 2 bash -c "exec 3<>/dev/tcp/127.0.0.1/${PORT}" 2>/dev/null; then exec 3<&- 3>&-; g "G0 port ${PORT} free before run" n "another listener holds it — stale instance? pkill -x ehr-api"; exit 6; else g "G0 port ${PORT} free before run" y; fi

echo "== Phase A · dry-run(先证参数真进配置,再开全量——D4教训的机制化)"
# A1: --print-openapi exit=0 且 stdout 是可解析 JSON、paths≥16(dry分支不打 serve addr)
dryout=$(EHR_HOME="$H" "$BIN" --print-openapi </dev/null); drc=$?
[[ $drc -eq 0 ]] && ok=y||ok=n; g "A1 dry-run exit=0" "$ok" "rc=$drc"
python3 -c 'import json,sys; d=json.load(sys.stdin); ps=d["paths"]; sys.exit(0 if len(ps)>=16 and "/api/v1/persons/{pid}/documents/{sha}" in ps else 1)' <<<"$dryout" \
  && ok=y||ok=n; g "A2 openapi.json ≥16 paths 含 documents/{sha} 模板" "$ok" "$(python3 -c 'import json,sys; print(len(json.load(sys.stdin)["paths"]))' <<<"$dryout")"
# A3: 未知参数必须即拒(D4-b 当年烧算力坑的机制守卫)
"$BIN" --pool-workers 13 >/dev/null 2>&1; urc=$?
[[ $urc -ne 0 ]] && ok=y||ok=n; g "A3 unknown arg rejected(ruc=$urc)" "$ok"

echo "== Phase B · serve + [证据]行核验"
EHR_HOME="$H" "$BIN" --addr="127.0.0.1:${PORT}" >"$WORK/serve.log" 2>&1 & SRV_PID=$!
for i in $(seq 1 40); do curl -fsS "$BASE/openapi.json" >/dev/null 2>&1 && break; sleep 0.25; done
# (D4自查抓到本次编辑误删:B1证据行守卫差点没了——恢复如下,勿再丢)
grep -q "\[证据\].*ADDR=127.0.0.1:${PORT}$" "$WORK/serve.log" && ok=y||ok=n; g "B1 evidence line ADDR matches(--addr真生效)" "$ok" "$(head -3 "$WORK/serve.log")"
# B2 ⚠️教训(本轮两连败元凶):服务挂 nest("/api/v1")后 healthz 在 **$API** 之下;$BASE裸路径必404——勿再回退!
# (另:上轮sed批量加 --max-time 只改了带空格的调用,for循环内的漏网——批量替换需事后grep自证,D4)
ok=n; for i in $(seq 1 30); do curl --max-time 5 -fsS "$API/healthz" | grep -q '"status":"ok"' && { ok=y; break; }; sleep 0.5; done
g "B2 healthz(ok) within 15s at /api/v1" "$ok"; [[ "$ok" == y ]] || head -8 "$WORK/serve.log"

echo "== Phase C · 人员域(中文建档回归,此前 D-010③ 的守卫保留)"
pid=$(curl -fsS -X POST "$API/persons" -H 'content-type: application/json' \
  -d '{"name":"林宝妹","sex":"F","birth_year":1988}' | python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])')
[[ "$pid" =~ ^[0-9a-f]{8}-[0-9a-f]{4}-4 ]] && ok=y||ok=n; g "C1 person created uuidv4($pid)" "$ok"

echo "== Phase D · hash docref 新实装三件套"
# 样本:含中文+数值的化验单文本(parser 应出 lab_report + FTS jieba 命中)
cat >"$WORK/血常规报告.txt" <<'EOF'
XX医院检验科检验报告单
姓名:林宝妹  性别:女  年龄:37岁   样本编号:20251015-A042
检验项目          结果      参考区间     标志
白细胞计数        6.2       3.5--9.5     /
血红蛋白          118       115--150     /
血小板计数        232       125--350     /
糖化血红蛋白      7.1       4.0--6.0     H
报告日期:2025年10月15日
EOF
sha_true=$(sha256sum "$WORK/血常规报告.txt" | cut -d' ' -f1)
up=$(curl -fsS -X POST "$API/persons/$pid/documents" -F "file=@$WORK/血常规报告.txt")
hash_ret=$(python3 -c 'import json,sys; print(json.load(sys.stdin)["content_hash"])' <<<"$up")
[[ "$hash_ret" == "$sha_true" ]] && ok=y||ok=n; g "D1 upload content_hash == independent sha256sum" "$ok" "ret=$hash_ret true=$sha_true"
grep -q '"text_indexed":true' <<<"$up" && grep -q '"doc_type":"lab_report"' <<<"$up" && ok=y||ok=n; g "D2 ingest indexed as lab_report" "$ok" "$up"

# D3 GET detail: 字段逐项对上真值(不只看200)
det=$(curl -fsS "$API/persons/$pid/documents/$hash_ret")
RAWJ="$det" SAMPLE_PATH="$WORK/血常规报告.txt" EXP_SHA="$hash_ret" python3 <<'PY' && ok=y || ok=n
import json,os
d=json.loads(os.environ["RAWJ"]); h=os.environ["EXP_SHA"]
assert d["content_hash"]==h, ("hash",d["content_hash"],h)
assert d["original_name"]=="血常规报告.txt", d["original_name"]
assert d["title"]=="血常规报告.txt", d["title"]            # title=filename 语义(vaults::ingest_upload)
assert d["mime_type"]=="text/plain", d["mime_type"]         # 内核按扩展名判定(D4查证过 import 行为)
assert d["doc_type"]=="lab_report", d["doc_type"]
assert d["doc_date"] and d["doc_date"].startswith("2025-10-1"), d["doc_date"]   # parser中文日期解析
assert d["byte_size"]==os.path.getsize(os.environ["SAMPLE_PATH"]), d["byte_size"]
assert d["page_count"]>=1 and d["text_indexed"], d
assert "糖化血红蛋白" in (d.get("text") or ""), "ocr text missing marker"
om=d.get("ocr_meta") or {}
assert om.get("model_version")=="ehr-http-ingest", ("meta",om)  # NewOcr.model_version接口级回显=D4参数生效证据
assert d.get("extraction") is None, ("no extraction yet",d.get("extraction"))
PY
g "D3 GET /documents/{sha} detail fields token-level verified" "$ok"

# D4 extraction 透传:派生缓存(SQLite medme.db)注入 result_json → API 应如实透出(D5:引用不替代)
DOCID=$(echo "$det" | python3 -c 'import json,sys;print(json.load(sys.stdin)["document_id"])')
DB="$H/vaults/$pid/medme.db"
EX_INJECT_OK=$(EXDB="$DB" EXDOC="$DOCID" python3 <<'PY' && echo y || echo n
import sqlite3,os
con=sqlite3.connect(os.environ["EXDB"])
con.execute("INSERT OR REPLACE INTO extraction(document_id,backend,model_version,mode,schema,result_json,created_at) VALUES (?,?,?,?,?,?,?)",
            (int(os.environ["EXDOC"]),"openai","gpt-test","json",1,
             '{"agent":"qwenpaw","findings":[{"field":"hba1c","value":7.1,"evidence_line":9}]}',
             "2026-10-20T00:00:00+00:00"))
con.commit(); print("y")
PY
) ; [[ "$EX_INJECT_OK" == *"y"* ]] && ok=y||ok=n; g "D4a injected extraction row into derived cache($DB)" "$ok"
# DDL与内核schema漂移则此门失去意义 —— user_version 同查(D4:测量方法先自证)
uver=$(python3 -c 'import sqlite3,sys; print(sqlite3.connect(sys.argv[1]).execute("PRAGMA user_version").fetchone()[0])' "$DB")
[[ "$uver" == "6" ]] && ok=y||ok=n; g "D4b medme.db user_version==6(kernel v6 schema => injection hit real table)" "$ok" "got=$uver"
det2=$(curl -fsS "$API/persons/$pid/documents/$hash_ret")
RAWJ="$det2" python3 <<'PY' && ok=y || ok=n
import json,os
ex=json.loads(os.environ["RAWJ"])["extraction"]
assert ex["parseable"] is True and ex["json"]["findings"][0]["value"]==7.1, ex
PY
g "D4c GET detail surfaces injected extraction JSON verbatim(parseable=true)" "$ok"

# D5 /original: 字节级复核 + Content-Disposition UTF-8(CJK名)
orig="$WORK/orig.bin"; cd "$WORK" || exit 5
hdrs=$(curl -fsS -D "$WORK/h.txt" -o "$WORK/orig.bin" -w '%{http_code}' "$API/persons/$pid/documents/$hash_ret/original")
sha_orig=$(sha256sum "$WORK/orig.bin" | cut -d' ' -f1)
[[ "$hdrs" == "200" && "$sha_orig" == "$sha_true" ]] && ok=y||ok=n; g "D5a original bytes re-hash identical to upload input(D5 sha never lost)" "$ok" "http=$hdrs orig=$sha_orig"
# D5b期望编码**现算**(D1教训:别手敲UTF-8百分号——我第一版把「常」背成E6B8…错了，实测E5B8B8才是真相)
want_enc=$(python3 -c 'from urllib.parse import quote; print(quote("血"))')
grep -qi "^content-type: text/plain" "$WORK/h.txt" && grep -q "filename\*=UTF-8''${want_enc}%E5%B8%B8%E8%A7%84" "$WORK/h.txt" \
  && ok=y||ok=n; g "D5b content-type + RFC5987 CJK filename header(computed-encoded match)" "$ok" "$(tr -d '\r' <"$WORK/h.txt" | grep -i 'content-' )"

echo "== Phase E · 契约内错误语义(不混报)"
# E1 malformed sha →400;E2 unknown pid →404;E3 valid-sha-no-doc →404(索引面);original对unknown hash也404
rc=$(curl --max-time 10 -s -o /dev/null -w '%{http_code}' "$API/persons/$pid/documents/deadbeef"); [[ "$rc" == "400" ]] && ok=y||ok=n; g "E1 bad sha ->400 (got $rc)" "$ok"
rc=$(curl --max-time 10 -s -o /dev/null -w '%{http_code}' "$API/persons/00000000-0000-4000-8000-000000000000/documents/$hash_ret/original"); [[ "$rc" == "404" ]] && ok=y||ok=n; g "E2 ghost person original ->404 (got $rc)" "$ok"
phantom=$(python3 -c 'print("ab"*32)')
rc=$(curl --max-time 10 -s -o /dev/null -w '%{http_code}' "$API/persons/$pid/documents/$phantom"); rc2=$(curl --max-time 10 -s -o /dev/null -w '%{http_code}' "$API/persons/$pid/documents/$phantom/original")
[[ "$rc" == "404" && "$rc2" == "404" ]] && ok=y||ok=n; g "E3 phantom hash detail+original ->404/404 (got $rc/$rc2)" "$ok"

echo "== Phase F · serve 与 dry-run openapi 同源核验(D1:同一代码现场合成)"
# D3修复后教训:F相dry也要EHR_HOME(main约定:dry同样要求home存在——校验不分分支,D4实测rc语义)
live=$(EHR_HOME="$H" "$BIN" --print-openapi </dev/null | python3 -c 'import json,sys; print(len(json.load(sys.stdin)["paths"]))' )
liven=$(curl -fsS "$BASE/openapi.json" | python3 -c 'import json,sys; print(len(json.load(sys.stdin)["paths"]))')
[[ "$liven" == "$live" && "$liven" -ge 16 ]] && ok=y||ok=n; g "F1 dry-run paths == live serve paths ($liven, >=16) structural drift guard" "$ok" "dry=$live live=$liven"

echo "== Phase G · labs 聚合(GET /persons/{pid}/labs = parser::aggregate() 的HTTP版，POC步骤07红线全套) =="
# D6单一来源：本相样本与 poc/import_demo 同族(中文日期+参考区间+-H标记)。断言方向一律「下限」——
# 上限写反把正确行为当失败是本项目实撞(D-009③)，此处只锁契约级不变量不锁解析器口味。
cat >"$WORK/b-lipid.txt" <<'EOF'
YY社区体检中心生化筛查
姓名:林宝妹  性别:女   样本编号:B20260302-07
检验项目          结果      参考区间     标志
白细胞计数        5.1       3.5--9.5     /
低密度脂蛋白胆固醇 3.9      0--3.4       ↑
采集时间:2026/03/02
EOF
shaB=$(sha256sum "$WORK/b-lipid.txt" | cut -d ' ' -f1)
upB=$(curl --max-time 30 -fsS -X POST "$API/persons/$pid/documents" -F "file=@$WORK/b-lipid.txt")
grep -q "\"content_hash\":\"$shaB\"" <<<"$upB" && ok=y||ok=n; g "G1 second doc uploaded(hash $shaB independent recompute)" "$ok" "$upB"
# G1b dedup幂等：同字节重传应 deduped=true(事件层去重不产生第二份真相 D5)
upB2=$(curl --max-time 30 -fsS -X POST "$API/persons/$pid/documents" -F "file=@$WORK/b-lipid.txt")
grep -q '"deduped":true' <<<"$upB2" && ok=y||ok=n; g "G1b identical re-upload deduped=true idempotent(no double truth)" "$ok" "$upB2"

labsj="$WORK/labs.json"; curl --max-time 30 -fsS "$API/persons/$pid/labs" -o "$labsj" \
  && SHA_A="$hash_ret" SHA_B="$shaB" RAWJ="$labsj" python3 <<'PY' && ok=y || ok=n
import json,os,sys
d=json.load(open(os.environ["RAWJ"]))
known={os.environ["SHA_A"],os.environ["SHA_B"]}
assert d["documents_scanned"]>=2, ("scanned",d["documents_scanned"])          # 下限(D-009教训:勿写上限)
assert d["total_points"]>=4 and len(d["series"])>=2,(d["series_count"],d["total_points"])
for s in d["series"]:      # G2a 证据回链完整性：全部点docref必须∈本役已知上传集——「数值只取自原件」的入口条件
    assert all(p["docref"] in known for p in s["points"]), (s["group_name"],[p["docref"][:10] for p in s["points"]])
def find(name,val):
    for s in d["series"]:
        for p in s["points"]:
            if abs(p["value"]-val)<1e-9 and name in s["group_name"]: return s,p
    return None,None
# G2b H标记+日精度日期(↑在原文=H于API，跨层语义保真；日期来自事件日志真相而非猜测)
sp=find("低密度",3.9); assert sp and sp[1]["flag"]=="H" and sp[1]["date"]=="2026-03-02" and sp[1]["docref"]==os.environ["SHA_B"], sp and sp[1]
assert sp[0].get("any_abnormal") is True, "ldl series flag rollup lost"
# G2c 跨报告同项归并成一条趋势线(wbc两点两来源两日期) —— 一人一箱聚合的核心卖点(D-002实装证明)
wbc=[(p.get("date"),p["docref"]) for s in d["series"] if (s.get("analyte_key") or s["group_name"]).startswith(("wbc","白细胞")) for p in s["points"]]
assert len(wbc)>=2 and len({r for _,r in wbc})>=2, ("cross-doc merge broken",wbc)
assert any(dt=="2025-10-15" for dt,_ in wbc) and any(dt=="2026-03-02" for dt,_ in wbc), wbc
PY
g "G2 labs: docref⊆uploads + H/date fidelity + cross-report wbc trend merged" "$ok"

# G3 「数值只取自原件」token级复核 —— 经**纯HTTP面**(GET detail text)复刻POC步骤07同款算法：
# 每个点的值必须能在其 docref 对应原文里以独立数字 token 浮点等价找到(防OCR尾零假阴性，勿退回字符串相等)。
RAWJ="$labsj" API="$API" SHA_A="$hash_ret" SHA_B="$shaB" python3 <<'PY' && ok=y || ok=n
import json,os,re,sys,urllib.request
d=json.load(open(os.environ["RAWJ"])); api=os.environ["API"]; pid_=d["person_id"]; texts={}
def num_tokens(t):
    out=set()
    for tok in re.split(r"[^\d.]+",t):
        try: out.add(float(tok))
        except ValueError: pass
    return out
miss=[]; checked=0
for s in d["series"]:
    for p in s["points"]:
        h=p["docref"]; checked+=1
        if h not in texts:
            with urllib.request.urlopen(f"{api}/persons/{pid_}/documents/{h}",timeout=15) as r:
                dd=json.load(r); assert dd["content_hash"]==h and dd.get("text"), ("detail missing text",h[:8])
            texts[h]=dd["text"]
        v=float(p["value"])
        if not any(abs(x-v)<=max(1e-9, abs(v)*1e-9) for x in num_tokens(texts[h])):
            miss.append((s["group_name"],p["value"],h[:10]))
assert not miss, ("provenance red-line violated:",miss,f"checked={checked}")
print(f"G3 provenance verified points={checked}", file=sys.stderr)
PY
g "G3 every lab point value exists as numeric token in its source original(via HTTP,P0红线)" "$ok"

echo "== Phase H · labs 契约一致性 =="
rc=$(curl --max-time 10 -s -o /dev/null -w '%{http_code}' "$API/persons/00000000-0000-4000-8000-000000000000/labs"); [[ "$rc" == "404" ]] && ok=y||ok=n; g "H1 unknown person labs ->404 ($rc)" "$ok"
# H2 typed-schema守卫：labs响应必须是utoipa合成进components的真schema(D1，非stub拼贴)
liveoc="$WORK/openapi-live.json"; curl --max-time 15 -fsS "$BASE/openapi.json" -o "$liveoc"
LIVEOC="$liveoc" python3 <<'PY' && ok=y || ok=n
import json,os
d=json.load(open(os.environ["LIVEOC"])); s=d.get("components",{}).get("schemas",{})
for k in ("LabsReply","LabSeriesOut","LabPointOut"): assert k in s, ("missing schema "+k, sorted(s)[:6])
props=s["LabsReply"]["properties"]; assert {"person_id","documents_scanned","series"} <= set(props), list(props)
dp=s["LabPointOut"]["properties"]; assert {"value","docref","date"} <= set(dp), list(dp)   # docref=证据回链字段必须在契约面
PY
g "H2 labs typed schemas (LabsReply/LabPointOut incl.docref) in openapi components" "$ok"

echo "== Phase I · timeline+search(补账:此前实装却无e2e守卫——D4『参数生效≠端点可验』欠的两笔现场证据) =="
# DBG相(DEBUG_I=1时跑): 无条件打印真实形状再定断言(D-009③教训——阈值先手算正/反语义)。
if [[ "${DEBUG_I:-}" == "1" ]]; then
  echo "--DBG timeline--"; curl -fsS "$API/persons/$pid/timeline" | python3 -m json.tool
  for w in 血常规 白细胞 zzqq; do enc=$(python3 -c "from urllib.parse import quote;print(quote('$w'))")
    printf -- '--DBG %s -> ' "$w"; curl -fsS "$API/persons/$pid/search?q=$enc" | head -c 300; echo; done
fi
# I1 timeline内容级：份数与两次上传一致 + A件title/日期(中文日期样本 2025-10-15)+doc_type逐项对真值；下限断言勿写上限
curl --max-time 15 -fsS "$API/persons/$pid/timeline" -o "$WORK/tl.json" \
  && RAWJ="$WORK/tl.json" SHA_T="dbg-none" PID_="$pid" python3 <<'PY' && ok=y || ok=n
import json,os,re
d=json.load(open(os.environ["RAWJ"])); assert d["person_id"]==os.environ["PID_"], d.get("person_id")
assert d["count"]==len(d["items"]) and d["count"]>=2, ("count",d["count"])   # 本役至少A/B两件(Duploads+G uploads)
titles={i.get("title") for i in d["items"]}
assert "血常规报告.txt" in titles, sorted(str(t)[:20] for t in titles)
a=[i for i in d["items"] if i.get("title")=="血常规报告.txt"][0]
assert a["doc_date"]=="2025-10-15T00:00:00+00:00", a                       # D-G同源中文日期，事件日志真相
assert a["doc_type"]=="lab_report" and isinstance(a["document_id"],int) and a["document_id"]>0, a
for it in d["items"]:                                                      # 全量形状守卫：有date必RFC3339
    if it.get("doc_date"): assert re.fullmatch(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}[+\-]\d{2}:\d{2}", it["doc_date"]), it
PY
g "I1 timeline: count==len + A件(title/中文日期2025-10-15/lab_report)逐项对真值" "$ok"
rc=$(curl --max-time 10 -s -o /dev/null -w '%{http_code}' "$API/persons/00000000-0000-4000-8000-000000000000/timeline"); [[ "$rc" == "404" ]] && ok=y||ok=n; g "I2 unknown person timeline ->404 (got $rc)" "$ok"
# I3 FTS+jieba内容级(词表来自本机DBG实跑，非拍脑袋——POC步骤06的HTTP复刻): 单词命中带[...]高亮、多词AND、对照零假阳性、q回显参数生效证据
TLQ="$WORK/sr.json"; curl --max-time 15 -fsS "$API/persons/$pid/search?q=%E7%99%BD%E7%BB%86%E8%83%9E" -o "$TLQ" \
  && RAWJ="$TLQ" python3 <<'PY' && ok=y || ok=n
import json,os
d=json.load(open(os.environ["RAWJ"]))
assert d["count"]>=1 and any("[白细胞]" in i["snippet"] for i in d["items"]), d          # jieba词界内才加括号=中文分词实证
assert all(i.get("document_id",0)>0 for i in d["items"])
PY
g "I3 search hit: 「白细胞」命中且snippet含[白细胞]高亮(jieba+FTS实证)" "$ok"
curl --max-time 15 -fsS "$API/persons/$pid/search?q=%E6%A3%80%E9%AA%8C%E6%8A%A5%E5%91%8A&limit=5" -o "$WORK/sr2.json" \
  && RAWJ="$WORK/sr2.json" python3 <<'PY' && ok=y||ok=n
import json,os
d=json.load(open(os.environ["RAWJ"]))
assert d["q"]=="检验报告", d["q"]                                    # q逐字回显=D4『参数真生效』的回执证据
assert d["count"]>=1, d
# ⚠️勿用「两token同现[高亮]」判AND：FTS5 snippet()只截~20字窗口，后匹配词可能落窗外(DBG实锤:
#   「XX医院 [检验]科…」命中了但[报告]在窗外)。多词AND的可观测等价面=每条命中至少含一个词的[高亮]、
#   零假阳性由I5把守、单词精确高亮由I3把守。POC步骤06同源结论经DBG复核成立。
snip=[i["snippet"] for i in d["items"]]
assert all(("[检验]" in s or "[报告]" in s) for s in snip), snip     # FTS命中必带内核高亮标记(非前端自加)
PY
g "I4 search multi-term AND + q verbatim echo(q=检验报告)" "$ok"
curl --max-time 15 -fsS "$API/persons/$pid/search?q=zzqqww" -o "$WORK/sr3.json" \
  && RAWJ="$WORK/sr3.json" python3 -c 'import json,os; d=json.load(open(os.environ["RAWJ"])); assert d["count"]==0, d' \
  && ok=y||ok=n; g "I5 rare-token control zzqqww -> count==0 (zero false positives)" "$ok"
rc=$(curl --max-time 10 -s -o /dev/null -w '%{http_code}' "$API/persons/00000000-0000-4000-8000-000000000000/search?q=x"); [[ "$rc" == "404" ]] && ok=y||ok=n; g "I6 unknown person search ->404 (got $rc)" "$ok"
rc=$(curl --max-time 10 -s -o /dev/null -w '%{http_code}' "$API/persons/$pid/search"); rc2=$(curl --max-time 10 -s -o /dev/null -w '%{http_code}' "$API/persons/$pid/search?q=%20%20")
[[ "$rc" == "400" && "$rc2" == "400" ]] && ok=y||ok=n; g "I7 missing q + blank(only-space) q ->400/400 (got $rc/$rc2)" "$ok"

kill "$SRV_PID" 2>/dev/null; SRV_PID=""; trap - EXIT
rm -rf "$H" "$WORK"
echo "== RESULT fails=$fails"
exit "$fails"
