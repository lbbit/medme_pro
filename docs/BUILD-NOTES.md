# medme-ehr 构建/通道笔记（阶段3 · Docker）——构建事实的单一来源(D1)

## 已验证配方(2026-10-20 实测，勿凭记忆改动数值)

| 项 | 值 | 证据 |
|---|---|---|
| crates sparse index | `sparse+https://mirrors.tuna.tsinghua.edu.cn/crates.io-index/` + `replace-with` | D-007;本机 ~/.cargo/config.toml 同源 |
| crates .crate 大包下载 | **static**: TUNA `<index>/static/cr/<aa>/<bb>/<name>-<ver>.crate`(路径是 `/cr/`!曾误拼成双写致 curl exit22)；回退 rsproxy `<root>/get/<name>/<ver>/download`,再回退官方源 | 冷缓存全量拉包实跑通;D-007b(TUNA可用)/rsproxy「大流量段不可用」历史结论**作废**(网络日异)。**先测单文件吞吐再开全量(D4)** |
| rustup dist(仅裸机装工具链时) | rsproxy env: RUSTUP_DIST_SERVER/SOURCE_UPDATE_SERVER=https://rsproxy.cn | D-007b 干净重装实证;**镜像内不需要**(FROM rust 自带,勿再加 ENV,见坑①)|
| apt | 默认 deb.debian.org(~1.8MB/s);TUNA mirror 实测更差 | Dockerfile ARG APT_MIRROR |
| GitHub pull/fetch | origin=ghfast.top 前缀地址 ~3MB/s | D-006 |

## Dockerfile 三个实战坑（本项目实录,D4-b 机制化条目）

0. **(D-016 后主路线)构建期联网走宿主 mihomo**:容器直连 static.crates.io TCP 不通(TCP_FAIL 实测),
   宿主 mihomo `allow-lan: true`(config.yaml 第3行,sudo 改后需重启 mihomo)后监听 *:7890;
   **关键坑:compose 的 `extra_hosts` 不会传给 buildx 的 RUN 层** → 必须用
   `docker buildx build --add-host host.docker.internal:host-gateway ...`(或 buildkit 配置)。
   另一坑:整目录 COPY ehr-api 会带回 .cargo/config.toml(vendored-sources 指向不存在的 vendor/)
   → build 层必须 `rm -f /src/ehr-api/.cargo/config.toml`(源码 COPY 之后)。两坑均已修入 Dockerfile。
   实证:/tmp/build9.log `[证据] deps fetched (network route)` + `dry-run exit=0`,e2e 冒烟 healthz/openapi.json 200。

1. **rust:1-slim-bookworm 已内置 toolchain**,任何 `RUSTUP_DIST_SERVER` 重指都会诱发构建期"补下载/校验"卡死→只 FROM+ENV PATH,绝不加 rustup env。(2026-10-20 e1-build.log 首发即中招)
2. **apt 私网 CA 证书**:deb.debian.org HTTPS cert 不在 debian:bookworm ca-certificates bundle → keyring 401。对策:`RUN sed -i 's|https://deb.debian.org|http://deb.debian.org|g' /etc/apt/sources.list.d/debian.sources`(换纯 http;本机 daemon.json 的 daocloud registry mirror **帮不到**容器内 apt)。若 TUNA 直连好可改 ARG APT_MIRROR 直指,一并免掉这条 sed。**待复测**。
3. (P7)`curl --retry-all-errors`:TUNA index config.json 偶发 502——必须让 curl 自重试而非 fail(曾把上游瞬时故障当 fatal)。脚本调 cargo 必带 `export PATH="$HOME/.cargo/bin:$PATH"`(D-010)。

## BuildKit 缓存卷(volume cache 不是共享 store!)

`--mount=type=cache,target=/usr/local/cargo|/build/target`,named=`medmeehr-cargocache/-buildtargetcache`(compose build.cache_from≠挂载卷;首次构建自建空量属预期)。⚠️多 project 并行 cron 会互踩同名卷(D-01x P8)→本部署独占后缀;**冷卷首轮全量拉包~数百MB正常**(≈15min@TUNA,见上表吞吐),别误判卡死。
增量证据:第二轮 rebuild `CANCELED/DONE` 层计数 vs 首轮对比记录于此↓

- e1-build.log(阶段3首建,Dockerfile v-skeleton):apt keyring 401 → STOP at apt layer(坑②首发)。已修入 Dockerfile。
