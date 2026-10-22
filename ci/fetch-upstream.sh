#!/usr/bin/env bash
# medme-ehr · 拉取上游 medme 源码（固定 commit，见 upstream.pin）
#
# 为什么需要它（D-016 / D-041 纪律「同一事实一份实现」）：
#   `upstream-medme/` 被 .gitignore 排除（只读参考，不进版本库），
#   而 ehr-api 通过 path 依赖引用 upstream-medme/packages/* ⇒ GitHub runner 上没有这份源码，
#   直接 build 必然失败（"failed to read .../packages/core-model/Cargo.toml"）。
#   ⇒ 两个 workflow（ci-image 镜像构建 / ci-gates 门禁）都需要它 ⇒ **必须共用一份实现**，
#   否则「镜像能构建但门禁跑不了」或反之，且两份代码会各自漂移。
#
# 拉哪些内容（最小集）：
#   Cargo.toml / Cargo.lock      —— 上游 workspace 根（packages 的 workspace.dependencies 继承自它）
#   packages/                    —— ehr-api 的 path 依赖本体
#   examples/sample-records/     —— web-e2e.sh 的上传样本（血常规报告单）
#     （只取 sample-records 子集：examples/ 全量 22M，red-team/demo-dataset 用不到）
#
# 通道（D-004~D-007 配方）：GitHub 直连 codeload 优先（runner 在境外）；
#   失败再退 ghfast.top 镜像前缀。两者都失败 ⇒ 退出非零（不静默跳过）。
#
# ⚠️ **不覆盖已有的 git 浅克隆**：`upstream-medme/` 在开发机上是一个 git 仓库
#   （AGENTS.md 的同步配方 = `git fetch && git reset --hard FETCH_HEAD`）。
#   本脚本是**CI 专用**（CI 上是干净 checkout，没有这份 clone）⇒ 若检测到 `.git` 存在，
#   **拒绝覆盖并退出**（避免把开发者的 clone 弄成「文件与 HEAD 不一致」的脏状态）。
#
# 用法: ./ci/fetch-upstream.sh          # 幂等：已是同一 commit 则直接跳过
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# shellcheck disable=SC1091
. "$ROOT/upstream.pin"
: "${UPSTREAM_REPO:?upstream.pin 缺 UPSTREAM_REPO}"
: "${UPSTREAM_COMMIT:?upstream.pin 缺 UPSTREAM_COMMIT}"

DEST="${UPSTREAM_DEST:-$ROOT/upstream-medme}"
STAMP="$DEST.commit"   # 戳记放在 dest **外面** ⇒ 不弄脏 clone 的 git status

if [ -d "$DEST/.git" ]; then
  echo "[FAIL] $DEST 是一个 git 浅克隆 ⇒ 本脚本拒绝覆盖它（避免弄脏开发者的 clone）。"
  echo "       本机同步上游请按 AGENTS.md 的配方："
  echo "         git -C upstream-medme fetch --depth 1 origin main && git -C upstream-medme reset --hard FETCH_HEAD"
  exit 1
fi

# 幂等：已是同一 commit 且关键目录齐 ⇒ 跳过（CI 里重建缓存也能命中）
if [ -d "$DEST/packages" ] && [ -d "$DEST/examples/sample-records" ] \
   && [ -f "$STAMP" ] && [ "$(cat "$STAMP")" = "$UPSTREAM_COMMIT" ]; then
  echo "[证据] upstream 已是 $UPSTREAM_COMMIT，跳过下载（upstream-medme/ 齐备）"
  exit 0
fi

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

TARBALL="$TMP/upstream.tar.gz"
OK=0
for URL in \
  "https://codeload.github.com/$UPSTREAM_REPO/tar.gz/$UPSTREAM_COMMIT" \
  "https://ghfast.top/https://github.com/$UPSTREAM_REPO/archive/$UPSTREAM_COMMIT.tar.gz" ; do
  echo "try $URL"
  if curl -sSfL --retry 2 --max-time 900 -o "$TARBALL" "$URL"; then
    OK=1
    break
  fi
done
[ "$OK" = "1" ] || { echo "[FAIL] 上游下载失败（直连与 ghfast.top 都试过）"; exit 1; }
test -s "$TARBALL"

SRC="$TMP/src"
mkdir -p "$SRC"
tar xzf "$TARBALL" -C "$SRC"
TOP="$(find "$SRC" -maxdepth 1 -mindepth 1 -type d | head -1)"
[ -n "$TOP" ] || { echo "[FAIL] 解包后找不到顶层目录"; exit 1; }

# 只取最小集（不做全量拷贝 ⇒ 省时间与空间）
mkdir -p "$DEST"
cp "$TOP/Cargo.toml" "$TOP/Cargo.lock" "$DEST/"
rm -rf "$DEST/packages" "$DEST/examples"
cp -r "$TOP/packages" "$DEST/packages"
mkdir -p "$DEST/examples"
cp -r "$TOP/examples/sample-records" "$DEST/examples/sample-records"
echo "$UPSTREAM_COMMIT" > "$STAMP"

echo "[证据] upstream = $UPSTREAM_REPO@$UPSTREAM_COMMIT"
echo "[证据] upstream packages: $(ls "$DEST/packages" | tr '\n' ' ')"
echo "[证据] upstream examples: $(ls "$DEST/examples" | tr '\n' ' ')"
