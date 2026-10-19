# poc — 阶段 1 单人内核证明（不带任何 Web/API）

目标：用 examples/sample-records/*.txt，纯库路径(非 CLI、不开 ocr feature)完成
建档→导入→时间线→search→labs 聚合→CAS原件回读，一条命令跑通并打印证据。

## 运行前置

- rustup stable (~/.cargo)，`~/.cargo/config.toml` 走 TUNA(DECISIONS D-007)
- `cd ../upstream-medme && cargo check -p parser` 先过 —— **这是本 POC 的开锁条件**；不过则停，不写 POC 代码(D4:失败即汇报)

## 计划脚本

`import_demo.rs`(bin crate `poc`, path 依赖 upstream packages/*):

    let vault = Vault::open(&dir)?;                       // 一人一箱: /tmp/poc-vault/"张三"
    for f in sample-records { pipeline-free import: read_text → add_document(+DocRef hash) }
    print timeline(); search("血常规"); aggregate().series(labs with flag+evidence);
    cas.read(source_sha256) == original bytes            // CAS 完整性断言
    drop(vault); delete db file; reopen                 // 「删派生库可重建」证明

验收输出=每步一行 [PASS] + labs 表带 H/L flag 与 evidence docref 锚点。
