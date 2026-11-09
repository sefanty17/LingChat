//! 续写配置：事件字段优先，其次 story_config 的 `dynamic:` 段，最后默认值。

use serde_json::{Map, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectorMode {
    /// 正式：走 Skill Agent 写章节
    Agent,
    /// 验证链路用：不调 LLM，写一个最小可运行章节
    Template,
}

#[derive(Debug, Clone)]
pub struct DirectorConfig {
    /// 失败去处：None = "end"，Some(id) = 跳到该章
    pub fallback: Option<String>,
    /// 本剧本累计最多生成多少章；None = 不限
    pub max_gen: Option<u32>,
    pub mode: DirectorMode,
}

impl DirectorConfig {
    /// 事件字段优先，其次 story_config 的 dynamic 段，最后默认值。
    pub fn resolve(event: &Value, script_settings: &Map<String, Value>) -> Self {
        let dynamic = script_settings.get("dynamic");

        let fallback = event
            .get("fallback")
            .and_then(|v| v.as_str())
            .or_else(|| dynamic.and_then(|d| d.get("fallback")).and_then(|v| v.as_str()))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        let max_gen = event
            .get("max_gen")
            .and_then(as_u32)
            .or_else(|| dynamic.and_then(|d| d.get("max_generated")).and_then(as_u32));

        let mode = event
            .get("mode")
            .and_then(|v| v.as_str())
            .or_else(|| dynamic.and_then(|d| d.get("mode")).and_then(|v| v.as_str()))
            .map(|s| {
                if s.trim().eq_ignore_ascii_case("template") {
                    DirectorMode::Template
                } else {
                    DirectorMode::Agent
                }
            })
            .unwrap_or(DirectorMode::Agent);

        Self {
            fallback,
            max_gen,
            mode,
        }
    }
}

fn as_u32(value: &Value) -> Option<u32> {
    match value {
        Value::Number(n) => n.as_u64().map(|v| v.min(u32::MAX as u64) as u32),
        Value::String(s) => s.trim().parse::<u32>().ok(),
        _ => None,
    }
}
