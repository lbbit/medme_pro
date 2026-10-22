#!/usr/bin/env python3
"""体检报告 PDF → LLM 结构化抽取 → 回写 extraction 的端到端取证脚本（T31）。

为什么需要它（PLAN 未销账项「体检报告 PDF 列布局 labs=0」）：
  真实体检报告是**列布局**（项目 | 结果 | 单位 | 参考区间 | 提示），
  上游 `parser::extract_labs` 的正则面向**化验单**（一行一个项目）⇒ 抽出来 0 条。
  ⇒ 缺口不在「正则不够好」，而在**缺一条「把 PDF 文本送进 LLM」的端到端路径**。

本脚本做的事（每一步都留证据）：
  ① 从 ehr-api 取文档文本层（GET /documents/{sha}）；
  ② 按**上游同一份** system prompt（upstream-medme/packages/deid/prompts/extract_v1_system.txt）
     + 同一份请求参数（extract_params.json）调用 LLM（OpenAI 兼容 /chat/completions）；
  ③ 把模型输出原样 PUT 回 `PUT /documents/{sha}/extraction`（服务端跑上游 deid::verify 逐字闸）；
  ④ 复跑 `GET /persons/{pid}/labs`，打印抽出来的指标条数。

用法：
  EHR_API_BASE=http://127.0.0.1:19031/api/v1 \
  LLM_BASE_URL=https://.../v1  LLM_API_KEY=sk-...  LLM_MODEL=... \
  python3 tools/pdf_llm_extract.py --pid <UUID> --sha <sha256> [--dry-run]

--dry-run：只做 ①②（打请求、打印模型原始输出与 labs 条数），**不回写**。
"""
import argparse
import json
import os
import pathlib
import sys
import urllib.error
import urllib.request

REPO = pathlib.Path(__file__).resolve().parent.parent
PROMPTS = REPO / "upstream-medme" / "packages" / "deid" / "prompts"


def http_json(url, *, method="GET", payload=None, headers=None, timeout=300):
    data = None if payload is None else json.dumps(payload).encode()
    req = urllib.request.Request(url, data=data, headers=headers or {}, method=method)
    if data is not None:
        req.add_header("Content-Type", "application/json")
    with urllib.request.urlopen(req, timeout=timeout) as r:
        body = r.read().decode()
    return json.loads(body) if body else None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--pid", required=True)
    ap.add_argument("--sha", required=True)
    ap.add_argument("--base", default=os.environ.get("EHR_API_BASE", "http://127.0.0.1:19031/api/v1"))
    ap.add_argument("--llm-base", default=os.environ.get("LLM_BASE_URL", ""))
    ap.add_argument("--model", default=os.environ.get("LLM_MODEL", ""))
    ap.add_argument("--max-tokens", type=int, default=0)
    ap.add_argument("--dry-run", action="store_true")
    args = ap.parse_args()

    key = os.environ.get("LLM_API_KEY", "")
    if not (args.llm_base and args.model and key):
        print("[ERROR] 需要 LLM_BASE_URL / LLM_MODEL / LLM_API_KEY 三个都非空", file=sys.stderr)
        return 2

    base = args.base.rstrip("/")
    if not base.endswith("/api/v1"):
        base = base + "/api/v1"
    token = os.environ.get("EHR_API_TOKEN", "")
    hdr = {"Authorization": f"Bearer {token}"} if token else {}

    # ① 文本层
    det = http_json(f"{base}/persons/{args.pid}/documents/{args.sha}", headers=hdr)
    text = det.get("text") or ""
    print(f"[证据] 文本层 chars={len(text)} doc_type={det.get('doc_type')}")
    if not text.strip():
        print("[ERROR] 该文档没有文本层，无法逐字核验（服务端也会拒）")
        return 1

    # ② LLM 抽取（prompt/参数 = 上游同一份文件，字节相同 ⇒ 行为可比）
    system = (PROMPTS / "extract_v1_system.txt").read_text(encoding="utf-8")
    params = json.loads((PROMPTS / "extract_params.json").read_text(encoding="utf-8"))["text"]
    max_tokens = args.max_tokens or params["max_tokens"]
    body = {
        "model": args.model,
        "messages": [{"role": "system", "content": system}, {"role": "user", "content": text}],
        "response_format": {"type": "json_object"},
        "temperature": 0,
        "max_tokens": max_tokens,
    }
    print(f"[证据] 请求 model={args.model} max_tokens={max_tokens} text_chars={len(text)}")
    out = http_json(
        args.llm_base.rstrip("/") + "/chat/completions",
        method="POST",
        payload=body,
        headers={"Authorization": f"Bearer {key}"},
    )
    choice = out["choices"][0]
    raw = choice["message"]["content"]
    print(f"[证据] finish_reason={choice.get('finish_reason')} usage={out.get('usage')}")
    try:
        parsed = json.loads(raw)
    except Exception as e:
        print(f"[ERROR] 模型输出不是 JSON：{e}\n{raw[:800]}")
        return 1
    labs = parsed.get("labs") or []
    print(f"[证据] 模型 labs 条数={len(labs)} doc_type={parsed.get('doc_type')!r} doc_date={parsed.get('doc_date')!r}")
    for item in labs[:12]:
        print("   ", json.dumps(item, ensure_ascii=False))

    if args.dry_run:
        print("[dry-run] 未回写 extraction")
        return 0

    # ③ 回写（服务端跑 deid::verify 逐字闸 + 落库校验后的结果）
    reply = http_json(
        f"{base}/persons/{args.pid}/documents/{args.sha}/extraction",
        method="PUT",
        payload={
            "result_json": json.dumps(parsed, ensure_ascii=False),
            "backend": f"llm-http:{args.llm_base}",
            "model_version": args.model,
            "mode": "text",
            "schema": 1,
        },
        headers=hdr,
    )
    print("[证据] 回写回执 " + json.dumps(reply, ensure_ascii=False))

    # ④ 复跑 labs 投影，看聚合面是否真的多出指标
    labs_reply = http_json(f"{base}/persons/{args.pid}/labs", headers=hdr)
    print(
        f"[证据] labs 投影 series={labs_reply.get('series_count')} points={labs_reply.get('total_points')} "
        f"docs_scanned={labs_reply.get('documents_scanned')}"
    )
    for s in labs_reply.get("series", [])[:12]:
        print("   ", s.get("group_name"), s.get("unit_canonical"), "points=", len(s.get("points", [])))
    return 0


if __name__ == "__main__":
    sys.exit(main())
