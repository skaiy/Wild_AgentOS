use std::io::{self, Write};

use crossterm::cursor::{MoveToColumn, RestorePosition, SavePosition};
use crossterm::style::{Color, Print, ResetColor, SetForegroundColor};
use crossterm::terminal::{Clear, ClearType};
use crossterm::{execute, queue};

use wild_agent_os_core::core::event_bus::Event;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorTheme {
    pub heading: Color,
    pub emphasis: Color,
    pub strong: Color,
    pub inline_code: Color,
    pub link: Color,
    pub quote: Color,
    pub spinner_active: Color,
    pub spinner_done: Color,
    pub spinner_failed: Color,
    pub tool_name: Color,
    pub file_path: Color,
    pub pa: Color,
    pub da: Color,
    pub ca: Color,
    pub aa: Color,
    pub sa: Color,
}

impl Default for ColorTheme {
    fn default() -> Self {
        Self {
            heading: Color::Cyan,
            emphasis: Color::Magenta,
            strong: Color::Yellow,
            inline_code: Color::Green,
            link: Color::Blue,
            quote: Color::DarkGrey,
            spinner_active: Color::Blue,
            spinner_done: Color::Green,
            spinner_failed: Color::Red,
            tool_name: Color::Yellow,
            file_path: Color::Green,
            pa: Color::Cyan,
            da: Color::Magenta,
            ca: Color::Yellow,
            aa: Color::Green,
            sa: Color::Blue,
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Spinner {
    frame_index: usize,
}

impl Spinner {
    const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

    pub fn new() -> Self {
        Self::default()
    }

    pub fn tick(&mut self, label: &str, theme: &ColorTheme, out: &mut impl Write) -> io::Result<()> {
        let frame = Self::FRAMES[self.frame_index % Self::FRAMES.len()];
        self.frame_index += 1;
        queue!(
            out,
            SavePosition,
            MoveToColumn(0),
            Clear(ClearType::CurrentLine),
            SetForegroundColor(theme.spinner_active),
            Print(format!("{frame} {label}")),
            ResetColor,
            RestorePosition
        )?;
        out.flush()
    }

    pub fn finish(&mut self, label: &str, theme: &ColorTheme, out: &mut impl Write) -> io::Result<()> {
        self.frame_index = 0;
        execute!(
            out,
            MoveToColumn(0),
            Clear(ClearType::CurrentLine),
            SetForegroundColor(theme.spinner_done),
            Print(format!("✔ {label}\n")),
            ResetColor
        )
    }

    pub fn fail(&mut self, label: &str, theme: &ColorTheme, out: &mut impl Write) -> io::Result<()> {
        self.frame_index = 0;
        execute!(
            out,
            MoveToColumn(0),
            Clear(ClearType::CurrentLine),
            SetForegroundColor(theme.spinner_failed),
            Print(format!("✘ {label}\n")),
            ResetColor
        )
    }
}

pub struct StreamRenderer {
    theme: ColorTheme,
    spinner: Spinner,
    current_phase: String,
}

impl Default for StreamRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamRenderer {
    pub fn new() -> Self {
        Self {
            theme: ColorTheme::default(),
            spinner: Spinner::new(),
            current_phase: String::new(),
        }
    }

    pub fn handle_event(&mut self, event: &Event) {
        let stdout = &mut io::stdout();
        match event.event_type.as_str() {
            "CYCLE_STARTED" => {
                self.current_phase = "SA".to_string();
                let _ = execute!(
                    stdout,
                    SetForegroundColor(self.theme.sa),
                    Print("\n┌─────────────────────────────────────\n"),
                    Print("│ 🎯 SupervisorAgent 调度中...\n"),
                    Print("└─────────────────────────────────────\n"),
                    ResetColor
                );
            }

            "PARALLEL_START" => {
                let _ = execute!(
                    stdout,
                    SetForegroundColor(Color::DarkGrey),
                    Print("  ├─ 并行执行开始\n"),
                    ResetColor
                );
            }

            "AGENT_BLOCKED" => {
                let _ = execute!(
                    stdout,
                    SetForegroundColor(Color::Yellow),
                    Print("  ├─ ⚠️ Agent 阻塞: "),
                    ResetColor,
                    Print(&event.payload),
                    Print("\n")
                );
            }

            "AGENT_ERROR" => {
                let _ = self.spinner.fail("Agent 错误", &self.theme, stdout);
                let _ = execute!(
                    stdout,
                    SetForegroundColor(Color::Red),
                    Print("  ├─ ❌ "),
                    Print(&event.payload),
                    Print("\n"),
                    ResetColor
                );
            }

            "STEP_PRIORITIZED" => {
                let _ = execute!(
                    stdout,
                    SetForegroundColor(Color::DarkGrey),
                    Print("  ├─ 步骤优先级调整: "),
                    ResetColor,
                    Print(&event.payload),
                    Print("\n")
                );
            }

            "STEP_SKIPPED" => {
                let _ = execute!(
                    stdout,
                    SetForegroundColor(Color::DarkGrey),
                    Print("  ├─ ⏭️ 跳过步骤: "),
                    ResetColor,
                    Print(&event.payload),
                    Print("\n")
                );
            }

            "STEP_ABORTED" => {
                let _ = execute!(
                    stdout,
                    SetForegroundColor(Color::Red),
                    Print("  ├─ 🛑 步骤中止: "),
                    ResetColor,
                    Print(&event.payload),
                    Print("\n")
                );
            }

            "TASK_FROZEN" => {
                let _ = execute!(
                    stdout,
                    SetForegroundColor(Color::Yellow),
                    Print("  ├─ ❄️ 任务冻结\n"),
                    ResetColor
                );
            }

            "TASK_ABORTED" => {
                let _ = execute!(
                    stdout,
                    SetForegroundColor(Color::Red),
                    Print("  ├─ 🛑 任务中止\n"),
                    ResetColor
                );
            }

            "INTERVENTION_EXECUTED" => {
                let _ = execute!(
                    stdout,
                    SetForegroundColor(Color::Cyan),
                    Print("  ├─ 🔧 干预执行: "),
                    ResetColor,
                    Print(&event.payload),
                    Print("\n")
                );
            }

            "OBJECTIVE_REFINED" => {
                let _ = execute!(
                    stdout,
                    SetForegroundColor(Color::Cyan),
                    Print("  ├─ 📝 目标优化: "),
                    ResetColor,
                    Print(&event.payload),
                    Print("\n")
                );
            }

            "CONSTRAINT_ADDED" => {
                let _ = execute!(
                    stdout,
                    SetForegroundColor(Color::Yellow),
                    Print("  ├─ 📋 添加约束: "),
                    ResetColor,
                    Print(&event.payload),
                    Print("\n")
                );
            }

            "SUPPLEMENTARY_CONTEXT" => {
                let _ = execute!(
                    stdout,
                    SetForegroundColor(Color::DarkGrey),
                    Print("  ├─ 📎 补充上下文\n"),
                    ResetColor
                );
            }

            "NOTIFY_HUMAN" => {
                let _ = execute!(
                    stdout,
                    SetForegroundColor(Color::Yellow),
                    Print("  ├─ 🔔 通知: "),
                    ResetColor,
                    Print(&event.payload),
                    Print("\n")
                );
            }

            _ => {
                // 显示未知事件类型（调试用）
                if !event.event_type.is_empty() {
                    let _ = execute!(
                        stdout,
                        SetForegroundColor(Color::DarkGrey),
                        Print("  ├─ "),
                        Print(&event.event_type),
                        Print(": "),
                        Print(&event.payload.chars().take(50).collect::<String>()),
                        Print("\n"),
                        ResetColor
                    );
                }
            }
        }
    }

    pub fn show_task_start(&mut self, workspace: &str) {
        let stdout = &mut io::stdout();
        let _ = execute!(
            stdout,
            SetForegroundColor(Color::Cyan),
            Print("\n╔══════════════════════════════════════╗\n"),
            Print("║          🚀 任务开始                  ║\n"),
            Print("╚══════════════════════════════════════╝\n"),
            ResetColor,
            SetForegroundColor(Color::DarkGrey),
            Print("  工作目录: "),
            SetForegroundColor(Color::Green),
            Print(workspace),
            Print("\n\n"),
            ResetColor
        );
    }

    pub fn show_task_result(&mut self, status: &str, summary: &str, turn_count: u32, tool_call_count: u32, workspace: &str) {
        let stdout = &mut io::stdout();
        let status_color = match status {
            "success" => Color::Green,
            "partial" => Color::Yellow,
            _ => Color::Red,
        };
        let status_icon = match status {
            "success" => "✅",
            "partial" => "⚠️",
            _ => "❌",
        };

        let _ = execute!(
            stdout,
            SetForegroundColor(Color::Cyan),
            Print("\n╔══════════════════════════════════════╗\n"),
            Print("║          📋 任务完成                  ║\n"),
            Print("╚══════════════════════════════════════╝\n"),
            ResetColor,
            SetForegroundColor(status_color),
            Print("  状态: "),
            Print(status_icon),
            Print(" "),
            Print(status),
            Print("\n"),
            ResetColor,
            SetForegroundColor(Color::DarkGrey),
            Print("  轮次: "),
            Print(&turn_count.to_string()),
            Print("  |  工具调用: "),
            Print(&tool_call_count.to_string()),
            Print("\n"),
            ResetColor,
            SetForegroundColor(Color::Green),
            Print("  📁 输出目录: "),
            Print(workspace),
            Print("\n"),
            ResetColor
        );

        if !summary.is_empty() {
            let _ = execute!(
                stdout,
                Print("\n"),
                SetForegroundColor(Color::White),
                Print("  "),
                Print(summary),
                Print("\n"),
                ResetColor
            );
        }
    }

    pub fn finish(&mut self) {
        let stdout = &mut io::stdout();
        let _ = execute!(stdout, ResetColor);
        let _ = stdout.flush();
    }
}

pub fn banner() {
    let stdout = &mut io::stdout();
    let _ = execute!(
        stdout,
        SetForegroundColor(Color::Cyan),
        Print("\n"),
        Print("╔══════════════════════════════════════╗\n"),
        Print("║        Code CLI - Agent OS           ║\n"),
        Print("║     编程控制台 (DeepSeek V4)          ║\n"),
        Print("╚══════════════════════════════════════╝\n"),
        ResetColor,
        SetForegroundColor(Color::DarkGrey),
        Print("  输入 /help 查看帮助，/exit 退出\n\n"),
        ResetColor
    );
}

pub fn prompt() {
    let stdout = &mut io::stdout();
    let _ = execute!(
        stdout,
        SetForegroundColor(Color::Green),
        Print("❯ "),
        ResetColor
    );
    let _ = stdout.flush();
}

pub fn user_input(text: &str) {
    let stdout = &mut io::stdout();
    let _ = execute!(
        stdout,
        SetForegroundColor(Color::DarkGrey),
        Print(format!("{}\n", text)),
        ResetColor
    );
}

pub fn info(msg: &str) {
    let stdout = &mut io::stdout();
    let _ = execute!(
        stdout,
        SetForegroundColor(Color::DarkGrey),
        Print(format!("{}\n", msg)),
        ResetColor
    );
}

pub fn success(msg: &str) {
    let stdout = &mut io::stdout();
    let _ = execute!(
        stdout,
        SetForegroundColor(Color::Green),
        Print(format!("{}\n", msg)),
        ResetColor
    );
}

pub fn warn(msg: &str) {
    let stdout = &mut io::stdout();
    let _ = execute!(
        stdout,
        SetForegroundColor(Color::Yellow),
        Print(format!("{}\n", msg)),
        ResetColor
    );
}

pub fn error(msg: &str) {
    let stdout = &mut io::stderr();
    let _ = execute!(
        stdout,
        SetForegroundColor(Color::Red),
        Print(format!("{}\n", msg)),
        ResetColor
    );
}

pub fn help_message() {
    let stdout = &mut io::stdout();
    let _ = execute!(
        stdout,
        SetForegroundColor(Color::Cyan),
        Print("\n╔══════════════════════════════════════╗\n"),
        Print("║           Code CLI 帮助              ║\n"),
        Print("╚══════════════════════════════════════╝\n"),
        ResetColor
    );
    let _ = execute!(
        stdout,
        Print("\n"),
        SetForegroundColor(Color::Yellow),
        Print("命令:\n"),
        ResetColor,
        Print("  /model <name>    切换模型 (deepseek-v4-flash / deepseek-v4-pro)\n"),
        Print("  /clear           清空对话历史\n"),
        Print("  /help            显示此帮助\n"),
        Print("  /exit            退出\n"),
        Print("\n"),
        SetForegroundColor(Color::Yellow),
        Print("多行输入: 以 \\ 结尾续行\n"),
        ResetColor,
        Print("\n"),
        SetForegroundColor(Color::DarkGrey),
        Print("说明: CLI 是 Agent OS 的终端界面，所有智能逻辑\n"),
        Print("      (工具调用、MCP、Hook、Agent调度、记忆管理)\n"),
        Print("      都由 Agent OS 内核处理。\n"),
        ResetColor,
        Print("\n")
    );
}
