//! 续写配置：事件字段优先，其次 story_config 的 `dynamic:` 段，最后默认值。

use serde_json::{Map, Value};

/// 默认最多续写多少段。作者可以用事件里的 `max_gen` 或 `story_config.yaml` 的
/// `dynamic.max_generated` 覆盖；写 0 表示关掉续写（直接走 fallback）。
pub const DEFAULT_MAX_GEN: u32 = 20;

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
    /// 本剧本累计最多生成多少段；`resolve` 总会给出值（缺省 `DEFAULT_MAX_GEN`），
    /// 作者写 `0` 即等于关掉续写。
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
            .or_else(|| dynamic.and_then(|d| d.get("max_generated")).and_then(as_u32))
            .or(Some(DEFAULT_MAX_GEN));

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_gen_defaults_to_twenty_and_event_wins() {
        let empty = Map::new();
        assert_eq!(
            DirectorConfig::resolve(&Value::Null, &empty).max_gen,
            Some(DEFAULT_MAX_GEN)
        );
        let ev = serde_json::json!({ "max_gen": 3 });
        assert_eq!(DirectorConfig::resolve(&ev, &empty).max_gen, Some(3));
        let zero = serde_json::json!({ "max_gen": 0 });
        assert_eq!(DirectorConfig::resolve(&zero, &empty).max_gen, Some(0));
    }

    #[test]
    fn mode_falls_back_to_agent() {
        let empty = Map::new();
        assert_eq!(
            DirectorConfig::resolve(&Value::Null, &empty).mode,
            DirectorMode::Agent
        );
        let ev = serde_json::json!({ "mode": "template" });
        assert_eq!(DirectorConfig::resolve(&ev, &empty).mode, DirectorMode::Template);
    }

    #[test]
    fn free_script_key_is_an_internal_var() {
        // PlayTrace::collect 会剔除 `__` 开头的键；标记必须遵守这个约定，
        // 否则它会被当成玩家变量写进轨迹和 state.json。
        assert!(crate::ai_service::game_system::script_engine::director::FREE_SCRIPT_KEY
            .starts_with("__"));
    }
}
