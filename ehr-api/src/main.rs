// medme-ehr · ehr-api bin —— 只负责启动：参数解析(未知即报错,D4-b)、[证据]行、dry-run/serve/rebuild。
// 路由与 OpenAPI 合成在 src/lib.rs（tests/contract.rs 交叉校验 CONTRACT↔router，防漂移）。
use anyhow::{Context, Result};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    // ⚠️ “参数传进去≠生效”是本项目 D4 三次血亏之处：未知 --flag 直接报错退出(D4-b 机制化)。
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut addr_arg: Option<String> = None;
    let mut print_openapi = false;
    let mut rebuild_pid: Option<String> = None;
    let mut rebuild_all_flag = false;
    let mut rebuild_dry = false;
    for a in &args {
        if let Some(v) = a.strip_prefix("--addr=") {
            addr_arg = Some(v.to_string());
        } else if a == "--print-openapi" {
            print_openapi = true;
        } else if a == "--rebuild-index" {
            rebuild_all_flag = true;
        } else if a == "--rebuild-index=dry-run" {
            rebuild_all_flag = true;
            rebuild_dry = true;
        } else if let Some(v) = a.strip_prefix("--rebuild-index=") {
            rebuild_pid = Some(v.to_string());
        } else {
            eprintln!(
                "[ERROR] unknown argument: {a}\nusage: ehr-api [--addr=HOST:PORT] [--print-openapi] [--rebuild-index | --rebuild-index=dry-run | --rebuild-index=PID]\n(env EHR_HOME, EHR_ADDR, EHR_API_TOKENS, EHR_API_TOKEN, RUST_LOG)"
            );
            std::process::exit(2);
        }
    }

    let home_in = std::path::PathBuf::from(std::env::var("EHR_HOME").unwrap_or_else(|_| "./ehr-home".into()));
    // dry-run 与 serve 都要求 EHR_HOME 已存在——只校验不创建(dry-run 不落任何状态；
    // serve 仅在其下建派生 vaults/ 子目录，registry/persons.jsonl 首写时才建)。
    // T4：鉴权表从 env 读（EHR_API_TOKENS / EHR_API_TOKEN）。格式错误 ⇒ 启动失败（D5：配置错误不静默退化）。
    let (st, token_set) = ehr_api::prepare_home_and_state_auth(&home_in, !print_openapi)?;
    // D1：生效地址单点计算一次，[证据]行与实际 bind 用同一个值。
    let addr = addr_arg.or_else(|| std::env::var("EHR_ADDR").ok()).unwrap_or_else(|| "127.0.0.1:8443".into());

    let mode = if print_openapi {
        "dry-run(print-openapi)"
    } else if rebuild_all_flag || rebuild_pid.is_some() {
        "rebuild"
    } else {
        "serve"
    };

    // ⚠️证据行走 **stderr**：dry-run 模式下 stdout 是机器数据(openapi.json),混入日志行会毒死管道消费方
    // (ehtest.sh A2/F1 首跑实锤——「Expecting value char 1」正是 [证据] 前缀字节)。POSIX 惯例:D4-b机制化。
    eprintln!(
        "[证据] ehr-api v{} | mode={} | EHR_HOME={} | ADDR={} | auth={}",
        env!("CARGO_PKG_VERSION"),
        mode,
        st.home.display(),
        addr,
        if token_set.is_open() {
            // open 模式必须显式可见：杜绝「以为开了鉴权其实没开」
            format!("open(tokens=0) ⚠️未配置 token，所有请求放行")
        } else {
            format!("bearer(tokens={})", token_set.len())
        },
    );

    if print_openapi {
        return serde_json::to_writer_pretty(std::io::stdout(), &ehr_api::openapi_json())
            .context("write openapi json")
            .map_err(anyhow::Error::from);
    }

    // rebuild 模式：离线动作（不需要 HTTP 服务），只操作 EHR_HOME 下的派生层。
    // 放在 serve 之前：重建是运维动作，不该要求 token、不该占端口。
    if rebuild_all_flag || rebuild_pid.is_some() {
        let home = st.home.clone();
        if rebuild_dry {
            let plan = ehr_api::rebuild::plan_rebuild(&home)?;
            println!("{}", serde_json::to_string_pretty(&plan)?);
            eprintln!(
                "[证据] ehr-api v{} | mode=rebuild(dry-run) | EHR_HOME={} | vaults={}",
                env!("CARGO_PKG_VERSION"),
                home.display(),
                plan.len()
            );
            return Ok(());
        }
        let results = match &rebuild_pid {
            Some(pid) => vec![ehr_api::rebuild::rebuild_person(&home, pid)?],
            None => ehr_api::rebuild::rebuild_all(&home)?,
        };
        println!("{}", serde_json::to_string_pretty(&results)?);
        let tl: usize = results.iter().map(|r| r.timeline_entries).sum();
        let pts: usize = results.iter().map(|r| r.lab_points).sum();
        eprintln!(
            "[证据] ehr-api v{} | mode=rebuild | EHR_HOME={} | vaults={} | timeline_entries={} | lab_points={}",
            env!("CARGO_PKG_VERSION"),
            home.display(),
            results.len(),
            tl,
            pts
        );
        return Ok(());
    }

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("bind {addr}"))?;
    tracing::info!("ehr-api serving http://{addr}/api/v1 · openapi at /openapi.json");
    axum::serve(listener, ehr_api::router(st)).await.context("axum serve")?;
    Ok(())
}
