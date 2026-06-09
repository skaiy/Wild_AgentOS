use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "gliding", about = "Agent OS 编程控制台 - 基于 DeepSeek V4 的智能编程助手")]
struct Cli {
    #[arg(help = "单次 prompt（无则进入交互模式）")]
    prompt: Option<String>,

    #[arg(short = 'm', long = "model", default_value = "deepseek-v4-flash", help = "模型选择")]
    model: String,

    #[arg(short = 'w', long = "workspace", default_value = ".", help = "工作目录")]
    workspace: String,

    #[arg(long = "max-iterations", default_value = "50", help = "最大迭代次数")]
    max_iterations: u32,

    #[arg(long = "api-key", help = "DeepSeek API key（优先使用环境变量 DEEPSEEK_API_KEY）")]
    api_key: Option<String>,

    #[arg(long = "api-url", help = "API 地址（优先使用环境变量 DEEPSEEK_API_URL）")]
    api_url: Option<String>,

    #[arg(short = 'v', long = "verbose", help = "显示详细日志")]
    verbose: bool,

    #[arg(long = "debug", help = "显示调试日志（更详细）")]
    debug: bool,

    #[arg(long = "resume", help = "从 checkpoint 恢复任务（提供 task_iri）")]
    resume: Option<String>,

    #[arg(long = "list-checkpoints", help = "列出所有 checkpoint")]
    list_checkpoints: bool,

    #[arg(long = "workflow", help = "JSON-LD 工作流定义文件路径（可选，替代 LLM 生成的 plan）")]
    workflow: Option<String>,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let log_level = if cli.debug {
        "debug"
    } else if cli.verbose {
        "info"
    } else {
        "warn"
    };

    // Capture all tracing output into a shared buffer so the TUI can display it
    // in the log panel instead of sending it to stderr where it corrupts the display.
    let log_buffer = std::sync::Arc::new(code_cli::log_buffer::LogBuffer::new());
    let shared_log = code_cli::log_buffer::SharedLogBuffer(log_buffer.clone());

    // tui-markdown 0.3 spams "Could not find syntax for code block: ''" on
    // every render when encountering fenced ``` or indented (4-space) code blocks.
    // Suppress its warnings to keep the log panel clean.
    let filter_with_suppressions = |level: &str| {
        format!("{},tui_markdown=error", level)
    };

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(filter_with_suppressions(&log_level))),
        )
        .with_writer(shared_log)
        .with_target(false)
        .init();

    if let Some(key) = cli.api_key {
        std::env::set_var("DEEPSEEK_API_KEY", key);
    }
    if let Some(url) = cli.api_url {
        std::env::set_var("DEEPSEEK_API_URL", url);
    }

    let config = code_cli::config::CliConfig::from_env_and_args(
        cli.model,
        cli.workspace.clone(),
        cli.max_iterations,
        cli.workflow,
    );

    if cli.list_checkpoints {
        list_checkpoints(&config)?;
        return Ok(());
    }

    if let Some(ref task_iri) = cli.resume {
        resume_task(config, task_iri, log_buffer)?;
        return Ok(());
    }

    if let Some(prompt) = cli.prompt {
        run_single(config, &prompt)?;
    } else {
        code_cli::tui::App::new(config, log_buffer, None)?.run()?;
    }

    Ok(())
}

fn run_single(config: code_cli::config::CliConfig, prompt: &str) -> anyhow::Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    
    let mut engine = code_cli::engine::CodeCliEngine::new(config)?;
    println!("Code CLI - Agent OS");
    println!("Model: {} | Workspace: {}", engine.model(), engine.workspace());
    println!();

    let result = rt.block_on(engine.process_task(prompt));

    match result {
        Ok((_, tr)) => {
            let icon = match tr.status.as_str() { "success" => "✅", _ => "❌" };
            println!("{} {} | Turns: {} | Tools: {}", icon, tr.status.to_uppercase(), tr.turn_count, tr.tool_call_count);
            println!("📁 Output: {}", engine.workspace());
            println!();
            println!("{}", tr.summary);
        }
        Err(e) => {
            eprintln!("Error: {}", e);
        }
    }

    Ok(())
}

fn list_checkpoints(config: &code_cli::config::CliConfig) -> anyhow::Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    let engine = code_cli::engine::CodeCliEngine::new(config.clone())?;

    let checkpoints = rt.block_on(engine.list_checkpoints())?;
    if checkpoints.is_empty() {
        println!("没有找到 checkpoint。");
    } else {
        println!("Checkpoints:");
        for cp in &checkpoints {
            println!("  {}  {}  turns={}  {}", cp.created_at, cp.name, cp.node_count, cp.task_iri);
        }
        println!("\n使用 glidingcode --resume <task_iri> 恢复");
    }
    Ok(())
}

fn resume_task(
    config: code_cli::config::CliConfig,
    task_iri: &str,
    log_buffer: std::sync::Arc<code_cli::log_buffer::LogBuffer>,
) -> anyhow::Result<()> {
    println!("从 checkpoint 恢复任务: {}", task_iri);
    println!("正在打开控制台...\n");
    code_cli::tui::App::new(config, log_buffer, Some(task_iri.to_string()))?.run()?;
    Ok(())
}
