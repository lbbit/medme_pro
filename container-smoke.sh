#!/bin/bash
# 容器化 e2e 抽查（阶段3 server 最小版验收）
# D4 教训入账（探针自身要被怀疑，本轮两次）：
#  ①python3 不在 rust:1-slim-bookworm 里 → Dockerfile 探针改纯 shell（build4 已修）
#  ②openapi.json 是压缩单行，任何缩进/分隔敏感的正则都会静默误数 →
#     一律用宿主 python3 真正 json.load 后取 len(paths)，不再猜格式。
set -u
B=http://127.0.0.1:8585
fail=0
# T46 后服务默认带 token 鉴权(deploy/.env 注入) → smoke 探针必须带 token。
# token 来源优先级: 环境变量 EHR_API_TOKEN > deploy/.env.tokens 解析 > deploy/.env 的 EHR_API_TOKEN
# 全都拿不到 ⇒ 显式报错退出(不静默打 401 假红——那不是被测对象的错)。
if [ -z "${EHR_API_TOKEN:-}" ]; then
  ENVFILE="$(cd "$(dirname "$0")" && pwd)/deploy/.env"
  [ -f "$ENVFILE" ] && EHR_API_TOKEN=$(grep -m1 '^EHR_API_TOKEN=' "$ENVFILE" | cut -d= -f2-)
fi
if [ -z "${EHR_API_TOKEN:-}" ]; then
  # D-042 收紧版再平衡：CI runner 没有 deploy/.env（gitignored）⇒ 这道门**环境不可满足**。
  # 处置：CI 中显式 SKIP 并写明原因（run-all-gates 的 A2 以 fails 计数为准 ⇒ 以 0 退出 + FAIL 行不发），
  #   同时提示本地补跑命令。本地无 token 依然红（那是真配置错误）。
  if [ -n "${CI:-}" ]; then
    echo "[SKIP] container-smoke: CI 无 deploy/.env（token 文件 gitignored）⇒ 鉴权探针不可执行；本地补跑：EHR_API_TOKEN=<token> ./container-smoke.sh"
    exit 0
  fi
  echo "[FAIL] 拿不到 token：服务已开鉴权。设 EHR_API_TOKEN 或确认 deploy/.env 存在"; exit 1
fi
AUTH="Bearer $EHR_API_TOKEN"
echo "[证据] smoke 使用 token 前缀=${EHR_API_TOKEN:0:10}…"
ck(){ if [ "$2" = "$3" ]; then echo "[PASS] $1 ($2)"; else echo "[FAIL] $1: got=$2 want=$3"; fail=$((fail+1)); fi; }

echo "== 1 healthz =="
H=$(curl -s -m 5 "$B/api/v1/healthz"); echo "$H"
ck "healthz" "$(printf '%s' "$H"|python3 -c 'import json,sys;print(json.load(sys.stdin)["status"])')" "ok"

echo "== 2 建档(中文名) =="
R=$(curl -s -m 5 -X POST "$B/api/v1/persons" -H "Authorization: $AUTH" -H 'content-type: application/json' -d '{"name":"容器验收·李四"}')
ID=$(printf '%s' "$R"|python3 -c 'import json,sys;print(json.load(sys.stdin)["id"])')
echo "person_id=$ID"; [ -n "$ID" ] || { echo "[FAIL] id 空"; fail=$((fail+1)); }

echo "== 3 timeline(空档 count==0 且 200) =="
T=$(curl -s -m 5 -w ' HTTP=%{http_code}' -H "Authorization: $AUTH" "$B/api/v1/persons/$ID/timeline"); echo "$T"
ck "timeline.count" "$(printf '%s' "$T"|python3 -c 'import json,sys;print(json.loads(sys.stdin.read().rsplit(" HTTP",1)[0])["count"])')" "0"
ck "timeline.http" "$(printf '%s' "$T"|grep -o 'HTTP=[0-9]*')" "HTTP=200"

echo "== 4 服务端 openapi paths =="
N=$(curl -s -m 5 "$B/openapi.json" | python3 -c 'import json,sys;print(len(json.load(sys.stdin)["paths"]))')
# T16 · D-032 未销账⑩：期望路径数**从镜像自身派生**（--print-rest-manifest），不再硬编码 16。
#   为什么：16 此前散在 5 处，真源在同一份代码里 ⇒ 同一事实五份实现。现在实现=真源。
EXP=$(docker run --rm --user 0 -e EHR_HOME=/tmp --entrypoint /usr/local/bin/ehr-api deploy-ehr-api --print-rest-manifest 2>/dev/null \
  | python3 -c 'import json,sys;print(json.load(sys.stdin)["paths"])')
# D-050：派生的 expected 若为空 ⇒ 「空 == 空」会被判成 PASS（假绿，本轮实测踩到）。
#   起因：容器内 /tmp/h 不存在 ⇒ ehr-api 报 "EHR_HOME not accessible" ⇒ stdout 空 ⇒
#   EXP 为空、步骤5 的 M 也为空 ⇒ 两个空串相等 ⇒ ck 判 PASS ⇒ 门是空的。
#   ⇒ 纪律：任何**派生的期望值**为空时必须红（期望值本身要被断言），不许拿空值参与比较。
if [ -z "$EXP" ]; then
  echo "[FAIL] image.manifest.paths: 镜像自身 --print-rest-manifest 未产出路径数（期望值为空 ⇒ 后续比较会是假绿）"
  fail=$((fail+1)); EXP=""
fi
ck "openapi.paths" "$([ -n "$N" ] && [ -n "$EXP" ] && [ "$N" = "$EXP" ] && echo ok)" "ok"; echo "paths=$N expected=$EXP (derived, not hardcoded)"

echo "== 5 镜像自身 dry-run(不经宿主二进制) =="
M=$(docker run --rm --user 0 -e EHR_HOME=/tmp --entrypoint /usr/local/bin/ehr-api deploy-ehr-api --print-openapi 2>/dev/null \
  | python3 -c 'import json,sys;print(len(json.load(sys.stdin)["paths"]))')
if [ -z "$M" ]; then
  echo "[FAIL] image.dry-run.paths: 镜像自身 --print-openapi 未产出路径数（值为空 ⇒ 比较会假绿）"
  fail=$((fail+1))
fi
ck "image.dry-run.paths" "$([ -n "$M" ] && [ -n "$EXP" ] && [ "$M" = "$EXP" ] && echo ok)" "ok"; echo "image_paths=$M expected=$EXP (derived, not hardcoded)"

echo "== 6 volume 落盘真相层(append-only JSONL 末行) =="
docker exec deploy-ehr-api-1 tail -n 1 /home/ehr/registry/persons.jsonl

echo "== 7 证据行与实际 bind 同源(D1) =="
docker compose -f deploy/docker-compose.yml logs ehr-api 2>&1 | grep -m1 '证据'

echo "== RESULT fails=$fail =="
exit $fail