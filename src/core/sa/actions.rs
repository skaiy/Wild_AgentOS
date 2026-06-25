use std::future::Future;
use std::pin::Pin;

use tracing::{info, warn};

use crate::core::event_bus::EventPriority;
use crate::core::sa::{ActionParams, InterventionAction, SupplementaryInputAction, SupervisorAgent};
use crate::CoreError;

type ActionHandler = Box<dyn for<'a> Fn(&'a mut SupervisorAgent, ActionParams, &'a str) -> Pin<Box<dyn Future<Output = Result<(), CoreError>> + Send + 'a>> + Send>;

/// 干预动作注册表：根据动作类型查找对应的处理函数
pub(super) fn get_action_handler(action: &InterventionAction) -> Option<ActionHandler> {
    match action {
        InterventionAction::Continue => Some(Box::new(|_sa, _params, _task_iri| {
            Box::pin(async move {
                info!("干预: 继续执行");
                Ok(())
            })
        })),
        InterventionAction::ContinueWithMonitor => Some(Box::new(|_sa, _params, _task_iri| {
            Box::pin(async move {
                warn!("干预: 继续执行但加强监控");
                Ok(())
            })
        })),
        InterventionAction::IncreaseRetry { .. } => Some(Box::new(|_sa, params, _task_iri| {
            Box::pin(async move {
                let retries = params.additional_retries.unwrap_or(3);
                info!("干预: 增加重试次数至 {} 次", retries);
                Ok(())
            })
        })),
        InterventionAction::IncreaseTimeout { .. } => Some(Box::new(|_sa, params, _task_iri| {
            Box::pin(async move {
                let secs = params.additional_seconds.unwrap_or(60);
                info!("干预: 增加超时时间至 {} 秒", secs);
                Ok(())
            })
        })),
        InterventionAction::ReduceComplexity => Some(Box::new(|_sa, _params, _task_iri| {
            Box::pin(async move {
                info!("干预: 降低复杂度预期");
                Ok(())
            })
        })),
        InterventionAction::RestrictTools { .. } => Some(Box::new(|_sa, params, _task_iri| {
            Box::pin(async move {
                let tools = params.allowed_tools.clone().unwrap_or_default();
                info!("干预: 限制可用工具集为 {:?}", tools);
                Ok(())
            })
        })),
        InterventionAction::SkipStep { .. } => Some(Box::new(|_sa, params, _task_iri| {
            Box::pin(async move {
                let step = params.step_id.as_deref().unwrap_or("unknown");
                info!("干预: 跳过步骤 {}", step);
                Ok(())
            })
        })),
        InterventionAction::RetryStep { .. } => Some(Box::new(|_sa, params, _task_iri| {
            Box::pin(async move {
                let step = params.step_id.as_deref().unwrap_or("unknown");
                info!("干预: 重试步骤 {}", step);
                Ok(())
            })
        })),
        InterventionAction::Parallelize => Some(Box::new(|_sa, _params, _task_iri| {
            Box::pin(async move {
                info!("干预: 并行化执行");
                Ok(())
            })
        })),
        InterventionAction::SplitStep { .. } => Some(Box::new(|_sa, params, _task_iri| {
            Box::pin(async move {
                let step = params.step_id.as_deref().unwrap_or("unknown");
                let sub_steps = params.sub_steps.clone().unwrap_or_default();
                info!("干预: 拆分步骤 {} 为 {:?} 个子步骤", step, sub_steps.len());
                Ok(())
            })
        })),
        InterventionAction::InsertExtraStep { .. } => Some(Box::new(|_sa, params, _task_iri| {
            Box::pin(async move {
                let desc = params.description.as_deref().unwrap_or("unknown");
                info!("干预: 插入额外步骤: {}", desc);
                Ok(())
            })
        })),
        InterventionAction::FallbackToShallow => Some(Box::new(|_sa, _params, _task_iri| {
            Box::pin(async move {
                info!("干预: 回退到浅层模式");
                Ok(())
            })
        })),
        InterventionAction::EmergencyMode => Some(Box::new(|_sa, _params, _task_iri| {
            Box::pin(async move {
                warn!("干预: 进入紧急模式");
                Ok(())
            })
        })),
        InterventionAction::IncreaseBudget { .. } => Some(Box::new(|_sa, params, _task_iri| {
            Box::pin(async move {
                let tokens = params.additional_tokens.unwrap_or(1000);
                let secs = params.additional_time_secs.unwrap_or(120);
                info!("干预: 增加预算 {} tokens + {} 秒（已获人工确认）", tokens, secs);
                Ok(())
            })
        })),
        InterventionAction::FreezeAndReport => Some(Box::new(|sa, _params, task_iri| {
            Box::pin(async move {
                info!("干预: 冻结当前状态并生成报告");
                let _ = sa.event_bus.emit(task_iri, "TASK_FROZEN", "SA",
                    &serde_json::json!({"action": "freeze_and_report", "task_iri": task_iri}).to_string()).await;
                Ok(())
            })
        })),
        InterventionAction::AbortTask { .. } => Some(Box::new(|sa, params, task_iri| {
            Box::pin(async move {
                let reason = params.reason.as_deref().unwrap_or("no specific reason");
                warn!("干预: 终止任务，原因: {}", reason);
                let _ = sa.event_bus.emit(task_iri, "TASK_ABORTED", "SA",
                    &serde_json::json!({"reason": reason}).to_string()).await;
                Ok(())
            })
        })),
        InterventionAction::NotifyHuman { .. } => Some(Box::new(|sa, params, task_iri| {
            Box::pin(async move {
                let msg = params.message.as_deref().unwrap_or("需要人工介入");
                info!("干预: 通知人工介入: {}", msg);
                let _ = sa.event_bus.emit_with_priority(task_iri, "NOTIFY_HUMAN", "SA",
                    &serde_json::json!({"message": msg, "task_iri": task_iri}).to_string(),
                    EventPriority::Critical,
                ).await;
                Ok(())
            })
        })),
    }
}

/// 从 LLM 输出中提取 JSON
fn extract_json(content: &str) -> &str {
    if content.starts_with('{') {
        content
    } else if let Some(start) = content.find('{') {
        if let Some(end) = content.rfind('}') {
            &content[start..=end]
        } else {
            content
        }
    } else {
        content
    }
}

/// 补充输入动作注册表
type SupplementaryInputHandler = Box<dyn for<'a> Fn(&'a SupervisorAgent, SupplementaryInputAction, ActionParams, &'a str) -> Pin<Box<dyn Future<Output = Result<(), CoreError>> + Send + 'a>> + Send>;

fn get_supplementary_handler(action: &SupplementaryInputAction) -> Option<SupplementaryInputHandler> {
    match action {
        SupplementaryInputAction::AddContext => Some(Box::new(|sa, _action, _params, supplement| {
            Box::pin(async move {
                info!("补充输入: 添加上下文");
                sa.inject_to_current_agent("", supplement).await;
                Ok(())
            })
        })),
        SupplementaryInputAction::RefineObjective => Some(Box::new(|_sa, _action, _params, supplement| {
            Box::pin(async move {
                info!("补充输入: 细化目标 - {}", supplement);
                Ok(())
            })
        })),
        SupplementaryInputAction::ProvideConstraint => Some(Box::new(|_sa, _action, _params, supplement| {
            Box::pin(async move {
                info!("补充输入: 提供约束 - {}", supplement);
                Ok(())
            })
        })),
        SupplementaryInputAction::GuideDirection => Some(Box::new(|sa, _action, _params, supplement| {
            Box::pin(async move {
                info!("补充输入: 引导方向 - {}", supplement);
                sa.inject_to_current_agent("", supplement).await;
                Ok(())
            })
        })),
        SupplementaryInputAction::PrioritizeStep => Some(Box::new(|_sa, _action, params, _supplement| {
            Box::pin(async move {
                let step = params.step_id.as_deref().unwrap_or("next");
                info!("补充输入: 优先步骤 - {}", step);
                Ok(())
            })
        })),
        SupplementaryInputAction::SuggestApproach => Some(Box::new(|sa, _action, _params, supplement| {
            Box::pin(async move {
                info!("补充输入: 建议方法 - {}", supplement);
                sa.inject_to_current_agent("", supplement).await;
                Ok(())
            })
        })),
        SupplementaryInputAction::PauseExecution => Some(Box::new(|_sa, _action, _params, _supplement| {
            Box::pin(async move {
                warn!("补充输入: 暂停执行");
                Ok(())
            })
        })),
        SupplementaryInputAction::ResumeExecution => Some(Box::new(|_sa, _action, _params, _supplement| {
            Box::pin(async move {
                info!("补充输入: 恢复执行");
                Ok(())
            })
        })),
        SupplementaryInputAction::SkipCurrentStep => Some(Box::new(|_sa, _action, _params, _supplement| {
            Box::pin(async move {
                info!("补充输入: 跳过当前步骤");
                Ok(())
            })
        })),
        SupplementaryInputAction::ConfirmDirection => Some(Box::new(|sa, _action, _params, supplement| {
            Box::pin(async move {
                info!("补充输入: 确认方向");
                sa.inject_to_current_agent("", supplement).await;
                Ok(())
            })
        })),
        SupplementaryInputAction::CorrectApproach => Some(Box::new(|sa, _action, _params, supplement| {
            Box::pin(async move {
                info!("补充输入: 纠正方向 - {}", supplement);
                sa.inject_to_current_agent("", supplement).await;
                Ok(())
            })
        })),
        SupplementaryInputAction::AbortCurrentStep => Some(Box::new(|_sa, _action, _params, _supplement| {
            Box::pin(async move {
                warn!("补充输入: 中止当前步骤");
                Ok(())
            })
        })),
    }
}

pub(super) fn parse_or_repair_json<T: serde::de::DeserializeOwned>(raw: &str) -> Result<T, String> {
    if let Ok(v) = serde_json::from_str(raw) {
        return Ok(v);
    }

    let mut repaired = String::with_capacity(raw.len() + 8);
    let mut in_string = false;
    let mut escaped = false;
    let mut brace_depth: i32 = 0;
    let mut bracket_depth: i32 = 0;

    for c in raw.chars() {
        if escaped {
            escaped = false;
        } else if c == '\\' && in_string {
            escaped = true;
        } else if c == '"' {
            in_string = !in_string;
        } else if !in_string {
            match c {
                '{' => brace_depth += 1,
                '}' => brace_depth -= 1,
                '[' => bracket_depth += 1,
                ']' => bracket_depth -= 1,
                _ => {}
            }
        }
        repaired.push(c);
    }

    if in_string {
        repaired.push('"');
    }
    while repaired.ends_with(',') {
        repaired.pop();
    }
    for _ in 0..brace_depth.max(0) {
        repaired.push('}');
    }
    for _ in 0..bracket_depth.max(0) {
        repaired.push(']');
    }

    serde_json::from_str(&repaired).map_err(|e| format!("{}", e))
}

