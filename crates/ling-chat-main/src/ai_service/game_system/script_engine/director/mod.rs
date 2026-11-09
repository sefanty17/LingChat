//! 目录员：剧本跑到末尾时，按玩家轨迹续写下一章。

pub mod agent_run;
pub mod config;
pub mod template;
pub mod trace;

pub use config::{DirectorConfig, DirectorMode};
pub use trace::PlayTrace;

use anyhow::{Context, Result, anyhow};

use crate::ai_service::game_system::script_engine::events::ScriptContext;
use crate::ai_service::game_system::script_engine::responses::{
    GeneratingPayload, SCRIPT_GENERATING,
};

use trace::GeneratedMeta;

/// 生成下一章。`Ok(None)` 表示按配置 fallback 处理，`Err` 原样上抛。
pub async fn generate_next_chapter(
    ctx: &mut ScriptContext<'_>,
    cfg: &DirectorConfig,
) -> Result<Option<String>> {
    let script = {
        let gs = ctx.game_status.lock().await;
        gs.script_status.clone()
    }
    .ok_or_else(|| anyhow!("ScriptStatus 未设置"))?;
    let dir = script.script_path.clone();

    let seq = PlayTrace::next_seq(&dir);
    if let Some(max) = cfg.max_gen {
        if seq > max {
            tracing::info!("[Director] 已到生成上限 {}（下一次是第 {} 章），不再续写", max, seq);
            return Ok(None);
        }
    }

    // Agent 写 `Chapters/__gen/<id>.yaml`，父目录得先存在（write_file 会建，但不依赖它）。
    let gen_dir = dir.join("Chapters").join(trace::GEN_DIR);
    std::fs::create_dir_all(&gen_dir)
        .with_context(|| format!("无法创建续写章节目录: {:?}", gen_dir))?;

    let trace = {
        let gs = ctx.game_status.lock().await;
        PlayTrace::collect(&gs)
    };
    trace.write_to(&dir)?;

    let id = PlayTrace::next_chapter_id(&dir);
    emit_generating(ctx.app, true, &id, seq);

    let result = match cfg.mode {
        DirectorMode::Template => template::write_template_chapter(&dir, &id, seq),
        DirectorMode::Agent => {
            let first = agent_run::write_chapter_by_agent(ctx, &script, &id, seq).await;
            match first {
                Ok(()) => Ok(()),
                Err(e) => {
                    tracing::warn!("[Director] 首次续写失败，重试一次: {:#}", e);
                    agent_run::write_chapter_by_agent(ctx, &script, &id, seq).await
                },
            }
        },
    };

    emit_generating(ctx.app, false, &id, seq);

    match result {
        Ok(()) => {
            let title = trace::read_chapter_title(&dir, &id).unwrap_or_else(|| id.clone());
            push_gen_meta(
                ctx,
                GeneratedMeta {
                    id: id.clone(),
                    title,
                    summary: String::new(),
                    ending: false,
                },
            )
            .await;
            tracing::info!("[Director] 续写完成: {}", id);
            Ok(Some(id))
        },
        Err(e) => {
            tracing::error!("[Director] 续写失败: {:#}", e);
            Ok(None)
        },
    }
}

fn emit_generating(app: &tauri::AppHandle, on: bool, chapter: &str, seq: u32) {
    let payload = GeneratingPayload {
        on,
        chapter: chapter.to_string(),
        seq,
    };
    let emitted = crate::ai_service::message_system::events::emit(app, SCRIPT_GENERATING, &payload);
    if let Err(e) = emitted {
        tracing::warn!("[Director] 发送续写状态失败: {}", e);
    }
}

async fn push_gen_meta(ctx: &mut ScriptContext<'_>, meta: GeneratedMeta) {
    let mut gs = ctx.game_status.lock().await;
    let Some(ss) = gs.script_status.as_mut() else {
        return;
    };
    let mut list: Vec<GeneratedMeta> = ss
        .vars
        .get(trace::GEN_KEY)
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    list.push(meta);
    match serde_json::to_value(&list) {
        Ok(value) => {
            ss.vars.insert(trace::GEN_KEY.to_string(), value);
        },
        Err(e) => tracing::warn!("[Director] 记录生成章节失败: {}", e),
    }
}
