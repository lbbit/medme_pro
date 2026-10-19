# 决策记录（ADR-lite，新决策在上）

## D-011 2026-10-20(下午) · **阶段2后半收口:documents三件套/timeline/search/labs四域实装获e2e全绿(fails=0)**；G3真凶=L相位缺env非溯源红线(D4『先怀疑测量方法』第三次应验)
背景:上轮(~03:xx,ehtest.sh扩至Phase G/H后)run-labs.log止于fails=2(G3,H2)。本轮复查证据链:
①H2缺口已在代码侧补上(lib.rs components(schemas(...LabPointOut,LabSeriesOut,LabsReply))+重建二进制--print-openapi现场回显3个schema齐全、paths=16)——但当时会话超时未复跑,D5补账。
②G3重跑仍FAIL→按D4先查测量法:**KeyError:'API' traceback钉死真凶=G3内嵌python的env前缀漏传`API=`**(只给了RAWJ/SHA_A/SHA_B)，urlopen拿不到base URL。机制化补修一行(`RAWJ="$labsj" API="$API" ...`)。旁证:同一算法在上轮两个独立DBG脚本(g3b/g3c 复刻fixture)checked=18 MISS=[]两次通过;本轮修后正式gate `G3 provenance verified points=6` PASS。**结论:labs数值溯源红线本身无恙，红线下的是探针**。教训升格:D-007/D-010同族——**断言脚本自己要被断言**;环境变量注入面应在heredoc首部`assert set(ENVIRON)>=requirement`或干脆缺key即traceback自曝(本次即traceback自曝，只是没人当场读)。
③新立Phase I守卫(I1~I7):timeline与search此前**已实装却零e2e覆盖**(D4『端点可验』欠账,contract.rs只锁挂载不锁语义)。词表经DEBUG_I相实跑现定(D-009③纪律:先无条件打印再定阈值):timeline=count==len+A件title/中文日期2025-10-15逐项对真值；search「白细胞」命中必含内核[高亮]、q逐字回显=D4参数生效回执、罕见词zzqqww count==0对照零假阳性、未知人404、缺q/纯空格q 400。
新坑(I4首跑FAIL破案,**FTS5 snippet()非AND完整性的证明面**):snippet仅截~20token窗口,jieba分词后多词查询的第二个匹配词可能落在窗外(实证:`检验报告`命中但snippet只见`[检验]`)——用「两token同现[高亮]」判AND是测量方法错误;等价可观测面=每条命中≥1个词的[高亮]+稀有词对照归零(I3/I5分工)，POC步骤06结论复核成立。**勿再把snippet窗当内容完整性**。
阶段判定:ehtest.sh G0+A3+B2+C1+D8+E3+F1+G4+H2+I7=34门逐名清点、全部PASS(fails=0;重跑port 8954,共享target二进制)+cargo test --all-targets契约/sha_gate全绿→PLAN表D-009/D-010既定验收项(multipart落地+timeline挂Vault+labs接aggregate)全部销账;**阶段2交付达成**(「curl版抽查对齐POC证据链」)。下一里程碑=阶段3 docker-compose一键部署，其中派生索引重建脚本属目标验收④硬项,MVP默认resilient open已覆盖并POC过(D-009删库重建)；「HTTP触发式删除medme.db重建」是否纳入API待定(见PLAN下一步)。
运维记账:本机git仓库初始化至今**0 commit**且无.gitignore(target合计~5G会被吞)→本轮补.gitignore(target//logs)+首提交checkpoint化以上状态。后台进程卫生(ehtest G0端口闸+/bin/kill按PID收尾)持续有效;本cron会话曾遗留一个调试ehr-api@18599孤儿,已收割——教训与D-010同源:**调起来就要记PID收干净**。

## D-010 2026-10-20 · **ehr-api 骨架验收 GREEN**（serve+curl+契约守卫全绿）；发现并机制化修复 cron 后台构建 PATH 坑
证据(CARGO_TARGET_DIR=/home/lbb/medme-shared-target,cargo test --all-targets): ①cargo check 通过(依赖161 crate,TUNA源);②cargo test: tests/contract.rs `contract_paths_are_routed_and_documented` PASS——CONTRACT表15条端点逐条验证 (a)router真实挂载(tower oneshot,非404;stub=501合法)且(b)/openapi.json含对应模板路径+operation——D1漂移保险已机制化(此前只有注释声称有该测试,实际缺失=D5补账)。③bin serve实测(EHR_HOME=$(mktemp -d),--addr=127.0.0.1:18543): [证据]行打出ADDR与bind同值(D1);healthz {"status":"ok"}→POST /persons 中文建档201返回UUIDv4→GET list折叠一致→timeline stub 501→openapi.json paths=16。落盘真相层核验: registry/persons.jsonl手工tail=append-only信封{op,ts,person_id,data},符合设计。④dry-run(--print-openapi)无副作用、exit=0(Broken pipe仅因管道head,无害);未知参数--pool-workers立即报错退出(D4-b首次实战拦截,正是当年烧算力的那个形状的坑)。
结构决策(bin/lib拆分): main.rs只做启动(args/证据行/bind)，路由+OpenAPI合成入src/lib.rs，tests经lib target可达(tests不能import bin——这是契约测试写不出来的根因)。AppState::from_home()为测试构造口。
新坑(D4第四变种·环境类):**setsid/nohup后台拉起构建时 PATH 不含 ~/.cargo/bin → exec cargo not found**（首轮 e2 后台build即死于此起点从未编译;轮询方还误以为"完成")。机制化(D4-b):**所有脚本/任务里调cargo一律显式 `export PATH="$HOME/.cargo/bin:$PATH"`**(本条为准绳),或后续在ehr-api同级加Makefile封装。另教训:*"[ -s xxx.done ]"哨兵若由失败进程负责touch，失败即永挂* —— done标志要在成功路径touch或由watcher自己判log错误行(本次640353空转循环47min由此来)。
下一步(stage-2后半,按D-009既定验收清单剩余项): Vault per-person挂载(`/persons/{pid}/timeline`换真Vault::open+timeline)、每人串行ingest队列(multipart POST /documents落地=POC九步http化)、labs接parser::aggregate。

## D-009 2026-10-19 · **阶段1 POC GREEN**：底座路线实锤(方案A+一人一vault)，进入阶段2(ehr-api 骨架)
证据(poc/src/import_demo.rs 全绿,cargo run exit=0): 打开裸目录成箱→导入 upstream examples/sample-records/*.txt×10
(未开 ocr/dicom feature) → sha256 CAS逐字节回验 → dedup幂等(timeline计数不变) → timeline含中文日期样本2025年解析、无日期样本保留 → FTS+jieba「血常规」命中带高亮 + 罕见词对照组零假阳性 →
parser::aggregate出 series=21 points=21,**点值溯源token级核验21/21通过**,异常flag在,LAB点.date与原件timeline日期一致(唯一缺口来自刻意「未标日期」样例) → **删 medme.db(+wal/shm)后 resilient open: timeline 10/10 +FTS热重建,零损失**。另 cargo test -p core-model -p parser =346 passed on Linux。
新坑(D4第三次同类,方向性变种):自检断言写反——"至少N个点有date应为下限契约",我误写成上限导致把**正确行为当失败**;DBG输出直接证伪(ptdate全部=titledate)。教训补条:**断言前先手算一遍正/反向语义,或先跑DBG无条件打印再定阈值**(本轮即用此法修正)。
下一步:D-阶段2 ehr-api(axum+utoipa OpenAPI+cron每人串行 ingest队列),骨架验收=dry-run curl /openapi.json+/healthz+单箱端到端http化POC等价9步的curl版抽查(/healthz,/persons POST,/documents multipart上传)。

## D-008 2026-10-19 · 每小时 cron `medme-ehr-hourly-push`（silent）自动推进本项目
规则已写入任务文本:读 PLAN →检查后台构建死活动作→小步前进且每步带生效证据→回写文档;仅里程碑需方向确认、或真实阻塞时才发消息打扰用户。timeout 1800s < 3600s 间隔满足防重叠。

## D-007b 2026-10-19 · **更正 D-007**：cargo“卡死”元凶是残破 rustup 工具链，不是网络
真因:首轮 rustup-init 被 kill -15 打断,留下无 manifest 的 stable 工具链(rustlib rlib n=0)——任何 crate 编译都报
"can't find crate for core/std",表面看似卡在下载。干净重装(uninstall+install minimal via rsproxy,全程顺畅)后
rlib=27,cargo check 正常开始编译。**D-007 里"TUNA/rsproxy 大包通道不可用"判断作废**(OSS透明代理403只干扰 GitHub raw/codeload,D-004/D-006仍有效);crates 镜像三家均可,TUNA 保留为默认(本文件为准)。
教训(D4-b):被打断的安装器必须显式验装(`ls .../rustlib/*/lib/*.rlib | wc -l` 应≈百级而非 0),一行命令的习惯。

## D-007 2026-10-19 · crates.io 通道: TUNA sparse index(大文件走 static);rsproxy 大流量段不可用
症状:D-阶段1首次 cargo check -p core-model 挂 3h+,只落 66 个小包(sparse config.json/rsproxy下载索引 json 都通),
target 里 0 rmeta —— **小元数据通、~MB级 .crate(linux-raw-sys/syn)卡死**。对照试验: rsproxy.cn
crates-index/config→404不存在但被阿里云OSS透明代理回签; tuna config.json→200直连可达。
处置:cargo source replace=tuna sparse(static.crates.tsinghua拉包)。机制化(D4-b):配置写死在
~/.cargo/config.toml,不靠记性。**教训**:任何长时间网络型构建,先单独测一个 MB 级文件的真实吞吐再开全量。

## D-006 2026-10-19 · 上游仓库通道：origin=https 直连(读免授权)，慢时走 ghfast.top 镜像克隆；推送需 SSH key
实测: 本机 GitHub **元数据操作飞快**(ls-remote 1.3s)、但 https tarball/pack 大流量通道被限速到KB/s级、
反复超时中断(codeload/浅clone均阵亡);代理7890也被同机 HF GGUF 下载占满。**ghfast.top 前缀 ~3MB/s**
最终成功交付 `--depth 1` 完整浅克隆(HEAD=9c9251d @2026-10-16)。
现状:`upstream-medme/.git` remote origin = ghfast 地址(后续 pull 若GitHub直连带宽恢复可改回;
只 fetch 不改写,无分叉风险)。SSH 测试:port22通、握手快,但本机 RSA 公钥
(SHA256:pi+1FkTOAqAOnu4UrgUUAjtKOM4hFsBAJeoFsAgxAKE, lbb@lbb-B450M-DS3H-V2)未绑 GitHub→publickey拒绝。
拉取永远不需要授权;**向 PR/推分支前需用户把该公钥加进 GitHub 账号**(或另发 deploy key)。

## D-005 2026-10-19 · 阶段0销账：方案 A 证实可行；不复用上游 services/api
证据(upstream-medme@main): workspace packages/* 零 Tauri 依赖(core-model 仅 rusqlite-bundled 等)；
`Vault::open(root)` 纯目录对象、Event 枚举无 person 字段→单人假设按设计，多人靠外层一人一 vault；
apps/cli 仅是 pipeline::ingest/Vault::search/timeline 的 ~102 行薄壳→不必走"调 CLI"路线。
上游 services/api = 阿里云 FC Flask「账号+密文同步」骨架(服务端只见密文)，与本项目的明文查询服务正交，不复用。
待验证余量: cargo check on Linux(本机无 rustc)、vault SQLite 并发写行为、ocr crate feature 重量。

## D-004 2026-10-19 · 网络路线：GitHub 拉取一律走本机 7890 代理
状态：**已验证必要**。直连 codeload/raw 被限速到 KB/s 级且与 HF GGUF 抢带宽；
本机其他任务均用 `-x http://127.0.0.1:7890`。后续 git clone / cargo 依赖同理考虑。

## D-003 2026-10-19 · 许可证策略：全盘跟随 AGPL-3.0
新增代码 AGPL 开源、保持与上游可双向合并。理由：AGPL 衍生品对外服务须开源，
既然我们要"提供服务"，最省事的路径就是本来就用 AGPL，同时便于反哺上游(PR)。

## D-002 2026-10-19 · 多人档案建模倾向：一人一 vault（待 POC 证实）
事件日志按人分目录各自追加，天然隔离/免冲突;跨人查询靠派生索引层聚合。
备选是单一日志加 subject 字段(改动上游更深,不倾向)。判据=少改上游。

## D-001 2026-10-19 · 底座路线暂定方案 A：复用 Rust 内核 + axum API 壳
A=Rust 服务端直接 link medme crates(axum+OpenAPI);B=HTTP 外壳调 medme-cli 子进程;
C=标准 FHIR Server 另起炉灶只借鉴理念。
选 A 的理由:core-model/parser/pipeline 全部资产可用,单一语言,Docker 编译产物清晰。
风险:crate 可能耦合 Tauri —— **阶段 0 勘察后若有硬耦合则降级为 B**。此决策待销账。
