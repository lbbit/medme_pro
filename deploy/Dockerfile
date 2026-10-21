# medme-ehr · ehr-api 多阶段镜像（阶段3 server 最小版）
# 通道决策：本机实测 D-007 —— crates.io 走 TUNA(sparse+static)；GitHub 大文件被透明代理截胡，
# 基础镜像用官方 rust:1-slim-bookworm（docker hub 拉取历史验证可用）；apt 走 TUNA。
# D1：镜像源 URL 只在此拼装 compose 传入的 build args，不在别处重复写死。

ARG RUST_IMAGE=rust:1-slim-bookworm
ARG DEBIAN_IMAGE=debian:bookworm-slim

########## build 阶段 ##########
FROM ${RUST_IMAGE} AS build
ARG CARGO_SPARSE_URL
ARG CARGO_STATIC_ROOT
ARG APT_MIRROR
ARG HTTPS_PROXY
ARG HTTP_PROXY
# D-016: 构建期允许联网。代理指向宿主 mihomo(host.docker.internal→docker0 网关, allow-lan 已开);
# 置空 ARG 即全直连。cargo 走小写 env,CIO_* 是老版本 cargo 的等价名。
ENV https_proxy=${HTTPS_PROXY} http_proxy=${HTTP_PROXY} \
    CIO_PROXY=${HTTPS_PROXY} NO_PROXY=localhost,127.0.0.1,host.docker.internal

# cargo 通道(D-007)：sparse index + static 下载双段镜像；--locked 用已提交的 Cargo.lock 固定版本
RUN mkdir -p /usr/local/cargo && printf '[source.crates-io]\nreplace-with = "tuna"\n\n[source.tuna]\nregistry = "sparse+%s"\n\n[registries.tuna-static]\nregistry = "sparse+%s"\n' \
      "$CARGO_SPARSE_URL" "$CARGO_STATIC_ROOT" > /usr/local/cargo/config.toml

WORKDIR /src
# 依赖层缓存：先只拷 Cargo.toml/lock 拉依赖，再拷源码（改动源码不重拉 160+ crates）
COPY ehr-api/Cargo.toml ehr-api/Cargo.lock ./ehr-api/
COPY upstream-medme/Cargo.toml upstream-medme/Cargo.lock ./upstream-medme/
COPY upstream-medme/packages /src/upstream-medme/packages
# workspace 父 Cargo.toml 覆盖子 crate —— 需要整棵 workspace 才能编译 ehr-api 的 path 依赖
# 注意：ehr-api 不在 upstream workspace 内（独立 workspace），path 依赖按普通 crate 编译。
#
# 依赖获取(D-016): 构建期**允许联网**，主路线=在线拉包(镜像源由 compose build-args 注入上方 config.toml)。
# ehr-api/.cargo/config.toml(若存在,vendored-sources)会覆盖 source——在线路线必须先移除它;
# vendor/ 目录仅作网络不可达时的兜底(git-ignored,宿主 cargo vendor 可再生),在线成功则不进镜像。
COPY ehr-api/Cargo.toml ehr-api/Cargo.lock ./ehr-api/
RUN rm -f /src/ehr-api/.cargo/config.toml && cd ehr-api && cargo fetch && echo "[证据] deps fetched (network route)"

COPY ehr-api /src/ehr-api
# ↑ 整目录拷贝含 web/index.html（include_str! 内嵌，D-016）；Dockerfile 行尾注释会被当参数解析
# 整目录 COPY 把 .cargo/config.toml(vendored-sources→vendor/)也带了回来，而 vendor/ 不进 context
# (D-016 在线路线)——再次删除，让 cargo 用上一步已 fetch 的缓存。
RUN rm -f /src/ehr-api/.cargo/config.toml
# dry-run 探针: --print-openapi 仍校验 EHR_HOME(启动前置同一代码路径),给个临时目录即可;
# 用 mktemp -d 而非写死路径,避免污染层。
RUN cd ehr-api && cargo build --release --locked && \
    EHR_HOME=$(mktemp -d) ./target/release/ehr-api --print-openapi > /tmp/oa.json && \
    EHR_HOME=$(mktemp -d) ./target/release/ehr-api --print-rest-manifest > /tmp/rm.json && \
    N=$(grep -oE '"paths"[[:space:]]*:[[:space:]]*[0-9]+' /tmp/rm.json | grep -oE '[0-9]+' | head -1) && \
    M=$(grep -oE '^[[:space:]]+"/api[^"]*":' /tmp/oa.json | wc -l) && \
    echo "[证据] Dockerfile build: manifest_paths=$N openapi_paths=$M" && \
    { [ -n "$N" ] && [ "$N" = "$M" ] || { echo "[FAIL] manifest paths=$N != openapi paths=$M (T16: 派生清单须与 OpenAPI 同源)"; exit 1; }; } && \
    { [ "$M" -ge 16 ] || { echo "[FAIL] openapi_paths=$M < 16"; exit 1; }; } && \
    echo "[证据] Dockerfile build: ehr-api dry-run exit=0"

########## runtime 阶段 ##########
FROM ${DEBIAN_IMAGE}
ARG APT_MIRROR
# 只替换主机名段，保留 URN 路径(/debian)。上轮两坑：①整段替换→.../debian/debian 404；
# ②sed 嵌套进 $() 且 ARG 里含 / 时转义层叠出 http:/debian → 用 bash 算主机名最直白。
RUN SEDHOST=$(echo "$APT_MIRROR" | cut -d/ -f3) && sed -i "s|deb.debian.org|$SEDHOST|g" /etc/apt/sources.list.d/debian.sources; \
    apt-get update && apt-get install -y --no-install-recommends ca-certificates && \
    rm -rf /var/lib/apt/lists/*
COPY --from=build /src/ehr-api/target/release/ehr-api /usr/local/bin/ehr-api
# 非 root（EHR_HOME 由 boot 容器在 volume 内预建属主，见 docker-compose.yml 注释）
RUN useradd -r -u 10001 ehr && mkdir -p /home/ehr
USER 10001
ENV EHR_HOME=/home/ehr EHR_ADDR=0.0.0.0:8443 RUST_LOG=info
# [证据]行走 stderr（见 main.rs 注释，stdout 是机器数据）
ENTRYPOINT ["/usr/local/bin/ehr-api"]
