#!/bin/bash
# 容器化 e2e 抽查（阶段3 server 最小版验收）
# D4 教训入账（探针自身要被怀疑，本轮两次）：
#  ①python3 不在 rust:1-slim-bookworm 里 → Dockerfile 探针改纯 shell（build4 已修）
#  ②openapi.json 是压缩单行，任何缩进/分隔敏感的正则都会静默误数 →
#     一律用宿主 python3 真正 json.load 后取 len(paths)，不再猜格式。
set -u
B=http://127.0.0.1:8585
fail=0
ck(){ if [ "$2" = "$3" ]; then echo "[PASS] $1 ($2)"; else echo "[FAIL] $1: got=$2 want=$3"; fail=$((fail+1)); fi; }

echo "== 1 healthz =="
H=$(curl -s -m 5 "$B/api/v1/healthz"); echo "$H"
ck "healthz" "$(printf '%s' "$H"|python3 -c 'import json,sys;print(json.load(sys.stdin)["status"])')" "ok"

echo "== 2 建档(中文名) =="
R=$(curl -s -m 5 -X POST "$B/api/v1/persons" -H 'content-type: application/json' -d '{"name":"容器验收·李四"}')
ID=$(printf '%s' "$R"|python3 -c 'import json,sys;print(json.load(sys.stdin)["id"])')
echo "person_id=$ID"; [ -n "$ID" ] || { echo "[FAIL] id 空"; fail=$((fail+1)); }

echo "== 3 timeline(空档 count==0 且 200) =="
T=$(curl -s -m 5 -w ' HTTP=%{http_code}' "$B/api/v1/persons/$ID/timeline"); echo "$T"
ck "timeline.count" "$(printf '%s' "$T"|python3 -c 'import json,sys;print(json.loads(sys.stdin.read().rsplit(" HTTP",1)[0])["count"])')" "0"
ck "timeline.http" "$(printf '%s' "$T"|grep -o 'HTTP=[0-9]*')" "HTTP=200"

echo "== 4 服务端 openapi paths =="
N=$(curl -s -m 5 "$B/openapi.json" | python3 -c 'import json,sys;print(len(json.load(sys.stdin)["paths"]))')
# T16 · D-032 未销账⑩：期望路径数**从镜像自身派生**（--print-rest-manifest），不再硬编码 16。
#   为什么：16 此前散在 5 处，真源在同一份代码里 ⇒ 同一事实五份实现。现在实现=真源。
EXP=$(docker run --rm --user 0 -e EHR_HOME=/tmp/h --entrypoint /usr/local/bin/ehr-api deploy-ehr-api --print-rest-manifest 2>/dev/null \
  | python3 -c 'import json,sys;print(json.load(sys.stdin)["paths"])')
ck "openapi.paths" "$([ "$N" = "$EXP" ] && echo ok)" "ok"; echo "paths=$N expected=$EXP (derived, not hardcoded)"

echo "== 5 镜像自身 dry-run(不经宿主二进制) =="
M=$(docker run --rm --user 0 -e EHR_HOME=/tmp/h --entrypoint /usr/local/bin/ehr-api deploy-ehr-api --print-openapi 2>/dev/null \
  | python3 -c 'import json,sys;print(len(json.load(sys.stdin)["paths"]))')
ck "image.dry-run.paths" "$([ "$M" = "$EXP" ] && echo ok)" "ok"; echo "image_paths=$M expected=$EXP (derived, not hardcoded)"

echo "== 6 volume 落盘真相层(append-only JSONL 末行) =="
docker exec deploy-ehr-api-1 tail -n 1 /home/ehr/registry/persons.jsonl

echo "== 7 证据行与实际 bind 同源(D1) =="
docker compose -f deploy/docker-compose.yml logs ehr-api 2>&1 | grep -m1 '证据'

echo "== RESULT fails=$fail =="
exit $fail