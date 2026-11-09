//! 用 Skill Agent 写下一章：新建会话 → 固定指令 → 强制两个队列项 → 确认落盘。

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use anyhow::{Context, Result, anyhow};

use crate::ai_service::game_system::script_engine::events::ScriptContext;
use crate::ai_service::game_system::script_engine::responses::{
    GeneratingPayload, SCRIPT_GENERATING,
};
use crate::ai_service::skill_agent::config::{
    SkillAgentConfig, resolve_context_window, resolve_skill_agent_provider,
};
use crate::ai_service::skill_agent::core::{SkillAgentRunContext, run_chat_with_plan};
use crate::ai_service::skill_agent::events::SkillAgentEvent;
use crate::ai_service::skill_agent::{ApprovalMap, CancelFlag, db, role, router, stage};
use crate::ai_service::types::{LlmMessage, ScriptStatus};

use super::trace::TRACE_DIR;

pub async fn write_chapter_by_agent(
    ctx: &mut ScriptContext<'_>,
    script: &ScriptStatus,
    chapter_id: &str,
) -> Result<()> {
    let script_key = script.path_key().replace('\\', "/");
    let script_dir = script.script_path.clone();

    let channel = build_progress_channel(ctx.app.clone(), chapter_id);

    let title = format!("剧本续写·{}·{}", script.name, chapter_id);
    let conversation_id = db::create_conversation(ctx.db, Some(title), Some(script_key.clone()))
        .await
        .map_err(anyhow::Error::msg)?;

    let llm = resolve_skill_agent_provider(ctx.app, Some(true))
        .ok_or_else(|| anyhow!("未配置可用的 LLM provider，无法续写章节"))?;
    let context_window = resolve_context_window(&llm).await;

    let config = SkillAgentConfig::load(ctx.app);
    let sandbox_dir = config.resolve_sandbox_dir();
    let skills_dir = config.resolve_skills_dir();

    let history = vec![LlmMessage::user(format!(
        "玩家已经玩到本剧本的末尾，接下来这一章由你续写。请先读 {TRACE_DIR}/00-index.md 了解玩家走过的全部剧情，再读最近 3 章的完整记录，然后按运行时指令写出 {chapter_id} 这一章。"
    ))];

    let forced_plan = Some(router::RoutePlan {
        items: vec![
            router::QueueItem {
                kind: role::TaskKind::DraftChapter,
                target: chapter_id.to_string(),
            },
            router::QueueItem {
                kind: role::TaskKind::WriteChapter,
                target: chapter_id.to_string(),
            },
        ],
        boundary: None,
        reason: Some("运行时续写".into()),
    });

    let agent_ctx = SkillAgentRunContext {
        conversation_id,
        channel,
        approvals: ApprovalMap::default(),
        db: ctx.db.clone(),
        llm: Arc::new(llm),
        app: ctx.app.clone(),
        config,
        sandbox_dir,
        skills_dir,
        data_dir: crate::data_dir::get_data_dir().clone(),
        script_key: Some(script_key.clone()),
        existing_script_keys: crate::utils::script_paths::enumerate_script_keys(),
        stage_snapshot: stage::derive(Some(script_key.as_str())),
        bound_script_key: std::sync::Mutex::new(None),
        context_window,
        runtime_rewrite: true,
    };

    let cancelled: CancelFlag = Arc::new(AtomicBool::new(false));
    run_chat_with_plan(agent_ctx, history, cancelled, forced_plan)
        .await
        .map_err(anyhow::Error::msg)?;

    crate::utils::script_paths::resolve_chapter_file(&script_dir, chapter_id, true)
        .map_err(anyhow::Error::msg)
        .with_context(|| format!("Agent 未写出章节: {}", chapter_id))?;
    Ok(())
}

/// 后端自建通道：只把 Status / ItemStart 转成一条进度（该事件只有 Serialize，故按 JSON 判型）。
fn build_progress_channel(
    app: tauri::AppHandle,
    chapter_id: &str,
) -> tauri::ipc::Channel<SkillAgentEvent> {
    let chapter = chapter_id.to_string();
    tauri::ipc::Channel::new(move |body| {
        let Ok(value) = body.deserialize::<serde_json::Value>() else {
            return Ok(());
        };
        let kind = value.get("type").and_then(|v| v.as_str()).unwrap_or_default();
        let text = match kind {
            "status" => value.get("content").and_then(|v| v.as_str()),
            "item_start" => value.get("title").and_then(|v| v.as_str()),
            _ => None,
        };
        let Some(text) = text else {
            return Ok(());
        };
        tracing::info!("[Director] 续写进度: {}", text);
        let payload = GeneratingPayload {
            on: true,
            chapter: chapter.clone(),
        };
        let emitted =
            crate::ai_service::message_system::events::emit(&app, SCRIPT_GENERATING, &payload);
        if let Err(e) = emitted {
            tracing::warn!("[Director] 发送续写进度失败: {}", e);
        }
        Ok(())
    })
}
