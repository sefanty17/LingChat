//! 玩家轨迹采集与落盘：`.agent/play-trace/` 下的总览、分章记录与状态快照。

use std::path::Path;

use serde_json::{Map, Value};

use crate::ai_service::game_system::game_status::GameStatus;
use crate::ai_service::types::{GameLine, ScriptStatus};
use crate::db::entities::line::LineAttribute;
use crate::utils::prompt::NARRATION_TAG;

pub const MARK_KEY: &str = "__chapter_marks";
pub const CHOICE_KEY: &str = "__choice_log";
pub const GEN_KEY: &str = "__gen_meta";
pub const GEN_DIR: &str = "__gen";
pub const TRACE_DIR: &str = ".agent/play-trace";

const MAX_CHAPTER_BYTES: usize = 150_000;

/// 章节在台词流里的起点。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChapterMark {
    pub id: String,
    pub line: usize,
}

/// 玩家做过的一次选择。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChoiceTrace {
    pub chapter: String,
    pub text: String,
    #[serde(default)]
    pub free: bool,
}

/// 已生成章节的元信息。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GeneratedMeta {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub ending: bool,
}

/// 轨迹里一条台词的类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceKind {
    Narration,
    Dialogue,
    Player,
}

/// 轨迹里的一条台词。
#[derive(Debug, Clone)]
pub struct TraceLine {
    pub kind: TraceKind,
    pub speaker: Option<String>,
    pub text: String,
    /// 这条台词在 `GameStatus.line_list` 里的下标。
    ///
    /// 章节标记记的是**原始**下标，而 `lines` 是筛过的（Plot/System 提示被丢掉），
    /// 所以切章时必须靠这个字段换算，不能直接拿原始下标当 `lines` 的下标用。
    pub raw: usize,
}

/// 一次续写前采集到的完整玩家轨迹。
#[derive(Debug, Clone)]
pub struct PlayTrace {
    pub script_key: String,
    pub script_name: String,
    pub chapters: Vec<ChapterMark>,
    pub choices: Vec<ChoiceTrace>,
    pub generated: Vec<GeneratedMeta>,
    /// 只含玩家/剧本变量，内部 `__` 键已剔除
    pub vars: Map<String, Value>,
    pub lines: Vec<TraceLine>,
}

impl PlayTrace {
    /// 从内存状态采集。调用方负责先锁好 game_status。
    pub fn collect(gs: &GameStatus) -> Self {
        let mut chapters: Vec<ChapterMark> = Vec::new();
        let mut choices = Vec::new();
        let mut generated = Vec::new();
        let mut vars = Map::new();
        let (script_key, script_name) = match gs.script_status.as_ref() {
            Some(ss) => {
                chapters = decode_list(ss.vars.get(MARK_KEY));
                choices = decode_list(ss.vars.get(CHOICE_KEY));
                generated = decode_list(ss.vars.get(GEN_KEY));
                vars = ss
                    .vars
                    .iter()
                    .filter(|(k, _)| !k.starts_with("__"))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                // 采集点尚未写入首个章节标记时，至少用当前章兜底，免得轨迹里一章都没有。
                if chapters.is_empty() && !ss.current_chapter_key.is_empty() {
                    chapters.push(ChapterMark {
                        id: ss.current_chapter_key.clone(),
                        line: 0,
                    });
                }
                (ss.path_key().replace('\\', "/"), ss.name.clone())
            },
            None => (String::new(), String::new()),
        };

        let lines = gs
            .line_list
            .iter()
            .enumerate()
            .filter_map(|(raw, line)| classify(raw, line))
            .collect();

        Self {
            script_key,
            script_name,
            chapters,
            choices,
            generated,
            vars,
            lines,
        }
    }

    /// 扫 Chapters/__gen 下已有 cNNN.yaml，返回下一个序号（从 1 起）。
    pub fn next_seq(script_dir: &Path) -> u32 {
        let dir = script_dir.join("Chapters").join(GEN_DIR);
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return 1;
        };
        let mut max = 0u32;
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let Some(stem) = name.strip_suffix(".yaml") else {
                continue;
            };
            let Some(num) = stem.strip_prefix('c') else {
                continue;
            };
            if let Ok(n) = num.parse::<u32>() {
                max = max.max(n);
            }
        }
        max + 1
    }

    /// "__gen/c001"
    pub fn next_chapter_id(script_dir: &Path) -> String {
        format!("{}/c{:03}", GEN_DIR, Self::next_seq(script_dir))
    }

    /// 写 <script_dir>/.agent/play-trace/：00-index.md、chNN.md、state.json
    pub fn write_to(&self, script_dir: &Path) -> anyhow::Result<()> {
        let dir = script_dir.join(TRACE_DIR);
        std::fs::create_dir_all(&dir)?;
        remove_chapter_files(&dir);

        let ranges = self.chapter_ranges();
        let mut rows: Vec<TraceRow> = Vec::new();
        for (idx, mark) in self.chapters.iter().enumerate() {
            let (start, end) = ranges[idx];
            let title = read_chapter_title(script_dir, &mark.id)
                .unwrap_or_else(|| mark.id.clone());
            let header = format!("# 第 {} 章 · {}\n\n", idx + 1, title);
            let body: Vec<String> = self.lines[start..end].iter().map(render_line).collect();
            let parts = split_parts(&header, &body);
            let mut files = Vec::with_capacity(parts.len());
            for (k, part) in parts.iter().enumerate() {
                let name = if parts.len() == 1 {
                    format!("ch{:02}.md", idx + 1)
                } else {
                    format!("ch{:02}-{}.md", idx + 1, part_suffix(k))
                };
                write_text(&dir.join(&name), part)?;
                files.push(name);
            }
            rows.push(TraceRow {
                id: mark.id.clone(),
                title,
                files,
            });
        }

        write_text(&dir.join("00-index.md"), &self.render_index(&rows))?;
        write_text(&dir.join("state.json"), &self.render_state()?)?;
        Ok(())
    }

    fn chapter_ranges(&self) -> Vec<(usize, usize)> {
        let total = self.lines.len();
        let n = self.chapters.len();
        // 标记记的是原始台词下标，`lines` 是筛过的，先换算成 `lines` 里的位置。
        let at = |raw: usize| self.lines.partition_point(|l| l.raw < raw).min(total);
        (0..n)
            .map(|i| {
                let start = at(self.chapters[i].line);
                let end = if i + 1 < n {
                    at(self.chapters[i + 1].line)
                } else {
                    total
                };
                (start, end.max(start))
            })
            .collect()
    }

    fn render_index(&self, rows: &[TraceRow]) -> String {
        let mut out = String::new();
        out.push_str("# 剧情轨迹总览\n");
        out.push_str(&format!(
            "剧本：{}（{}）\n",
            self.script_name, self.script_key
        ));
        out.push_str(&format!(
            "已走章节：{}　已生成：{}\n\n",
            self.chapters.len(),
            self.generated.len()
        ));

        out.push_str("## 章节\n");
        out.push_str("| # | 章节 id | 标题 | 记录文件 |\n");
        out.push_str("|---|---------|------|----------|\n");
        for (idx, row) in rows.iter().enumerate() {
            out.push_str(&format!(
                "| {} | {} | {} | {} |\n",
                idx + 1,
                row.id,
                row.title,
                row.files.join("、")
            ));
        }

        out.push_str("\n## 关键选择\n");
        if self.choices.is_empty() {
            out.push_str("（无）\n");
        } else {
            for choice in &self.choices {
                out.push_str(&format!("- [{}] {}\n", choice.chapter, choice.text));
            }
        }

        out.push_str("\n## 已生成章节\n");
        if self.generated.is_empty() {
            out.push_str("（无）\n");
        } else {
            for meta in &self.generated {
                if meta.summary.trim().is_empty() {
                    out.push_str(&format!("- {} {}\n", meta.id, meta.title));
                } else {
                    out.push_str(&format!("- {} {}：{}\n", meta.id, meta.title, meta.summary));
                }
            }
        }

        out.push_str("\n## 下一步\n");
        let n = self.chapters.len();
        if n == 0 {
            out.push_str("（暂无章节记录）\n");
        } else {
            let from = if n > 3 { n - 2 } else { 1 };
            out.push_str(&format!(
                "读最近 3 章完整记录（ch{from:02}.md ~ ch{n:02}.md），更早的只看本页摘要。\n"
            ));
        }
        out
    }

    fn render_state(&self) -> anyhow::Result<String> {
        let value = serde_json::json!({
            "script_key": &self.script_key,
            "script_name": &self.script_name,
            "chapters": &self.chapters,
            "choices": &self.choices,
            "generated": &self.generated,
            "vars": &self.vars,
        });
        Ok(serde_json::to_string(&value)?)
    }
}

struct TraceRow {
    id: String,
    title: String,
    files: Vec<String>,
}

/// 记下一个章节起点；`line` 是进入该章时的台词条数，用来把台词按章切开。
pub fn push_chapter_mark(ss: &mut ScriptStatus, id: &str, line: usize) {
    if id.trim().is_empty() {
        return;
    }
    let mut marks: Vec<ChapterMark> = decode_list(ss.vars.get(MARK_KEY));
    if marks.last().map(|m| m.id == id).unwrap_or(false) {
        return;
    }
    marks.push(ChapterMark {
        id: id.to_string(),
        line,
    });
    if let Ok(value) = serde_json::to_value(&marks) {
        ss.vars.insert(MARK_KEY.to_string(), value);
    }
}

/// 记下玩家的一次选择。
pub fn push_choice(ss: &mut ScriptStatus, text: &str, free: bool) {
    let mut choices: Vec<ChoiceTrace> = decode_list(ss.vars.get(CHOICE_KEY));
    choices.push(ChoiceTrace {
        chapter: ss.current_chapter_key.clone(),
        text: text.to_string(),
        free,
    });
    if let Ok(value) = serde_json::to_value(&choices) {
        ss.vars.insert(CHOICE_KEY.to_string(), value);
    }
}

pub fn read_chapter_title(script_dir: &Path, chapter_id: &str) -> Option<String> {
    let path = crate::utils::script_paths::resolve_chapter_file(script_dir, chapter_id, true).ok()?;
    let text = std::fs::read_to_string(path).ok()?;
    let value: Value = serde_yaml::from_str(&text).ok()?;
    value
        .get("name")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn decode_list<T: serde::de::DeserializeOwned>(value: Option<&Value>) -> Vec<T> {
    value
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}

/// 一条台词 → 轨迹行；不属于剧情的一律丢掉。
///
/// 旁白信封（`{旁白:…}`）优先于 attribute 判定：引擎写旁白时走
/// `PromptLine::new(PromptRole::Narrator, ..)`，落表的 attribute 仍是 User，
/// 只靠 attribute 会把旁白错记成玩家台词。
fn classify(raw: usize, line: &GameLine) -> Option<TraceLine> {
    let content = line.base.content.as_str();
    if content.trim().is_empty() {
        return None;
    }
    if content.contains("（接下来的剧情演绎提示：") || content.contains("（系统提示：") {
        return None;
    }
    if let Some(rest) = content.trim().strip_prefix(NARRATION_TAG) {
        let inner = rest.strip_suffix('}').unwrap_or(rest).trim();
        if inner.is_empty() {
            return None;
        }
        return Some(TraceLine {
            kind: TraceKind::Narration,
            speaker: None,
            text: inner.to_string(),
            raw,
        });
    }
    match line.attribute() {
        LineAttribute::User => Some(TraceLine {
            kind: TraceKind::Player,
            speaker: line.base.display_name.clone(),
            text: content.to_string(),
            raw,
        }),
        LineAttribute::Assistant => Some(TraceLine {
            kind: TraceKind::Dialogue,
            speaker: line.base.display_name.clone(),
            text: content.to_string(),
            raw,
        }),
        LineAttribute::System | LineAttribute::Tool => None,
    }
}

fn render_line(line: &TraceLine) -> String {
    let name = match line.kind {
        TraceKind::Narration => "旁白".to_string(),
        TraceKind::Player => line
            .speaker
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "玩家".to_string()),
        TraceKind::Dialogue => line
            .speaker
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "角色".to_string()),
    };
    format!("【{}】{}", name, line.text)
}

fn split_parts(header: &str, lines: &[String]) -> Vec<String> {
    let mut parts: Vec<String> = Vec::new();
    let mut cur = String::from(header);
    for line in lines {
        if cur.len() > header.len() && cur.len() + line.len() + 1 > MAX_CHAPTER_BYTES {
            parts.push(std::mem::take(&mut cur));
            cur.push_str(header);
        }
        cur.push_str(line);
        cur.push('\n');
    }
    parts.push(cur);
    parts
}

fn part_suffix(index: usize) -> String {
    let mut n = index;
    let mut out = String::new();
    loop {
        out.insert(0, (b'a' + (n % 26) as u8) as char);
        if n < 26 {
            break;
        }
        n = n / 26 - 1;
    }
    out
}

fn remove_chapter_files(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with("ch") || !name.ends_with(".md") {
            continue;
        }
        let stem = &name[2..name.len() - 3];
        if stem.chars().all(|c| c.is_ascii_digit() || c == '-') {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

fn write_text(path: &Path, text: &str) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut tmp = path.as_os_str().to_os_string();
    tmp.push(".tmp");
    let tmp = std::path::PathBuf::from(tmp);
    std::fs::write(&tmp, text.as_bytes())?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("lc_trace_{}_{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn next_seq_starts_at_one_and_skips_existing() {
        let dir = tmp_dir("seq");
        assert_eq!(PlayTrace::next_seq(&dir), 1);
        assert_eq!(PlayTrace::next_chapter_id(&dir), "__gen/c001");

        let gen_dir = dir.join("Chapters").join(GEN_DIR);
        std::fs::create_dir_all(&gen_dir).unwrap();
        std::fs::write(gen_dir.join("c001.yaml"), "name: x\n").unwrap();
        std::fs::write(gen_dir.join("c003.yaml"), "name: y\n").unwrap();
        assert_eq!(PlayTrace::next_seq(&dir), 4);
    }

    #[test]
    fn template_chapter_is_runnable_yaml() {
        let dir = tmp_dir("tpl");
        let id = "__gen/c001";
        crate::ai_service::game_system::script_engine::director::template::write_template_chapter(
            &dir, id, 1,
        )
        .unwrap();
        let path = crate::utils::script_paths::resolve_chapter_file(&dir, id, true).unwrap();
        let text = std::fs::read_to_string(path).unwrap();
        let value: Value = serde_yaml::from_str(&text).unwrap();
        let events = value.get("events").and_then(|v| v.as_array()).unwrap();
        assert!(
            events
                .iter()
                .any(|e| e.get("type").and_then(|t| t.as_str()) == Some("chapter_end")),
            "模板章节必须自带 chapter_end，否则引擎不知道下一章: {text}"
        );
    }

    #[test]
    fn trace_splits_lines_by_chapter_mark() {
        let dir = tmp_dir("split");
        let trace = PlayTrace {
            script_key: "standalone/t".into(),
            script_name: "测试剧本".into(),
            chapters: vec![
                ChapterMark { id: "main".into(), line: 0 },
                ChapterMark { id: "main2".into(), line: 2 },
            ],
            choices: vec![ChoiceTrace { chapter: "main2".into(), text: "去公园".into(), free: false }],
            generated: vec![],
            vars: Map::new(),
            lines: vec![
                TraceLine { kind: TraceKind::Narration, speaker: None, text: "开场".into(), raw: 0 },
                TraceLine { kind: TraceKind::Player, speaker: Some("我".into()), text: "第一句".into(), raw: 1 },
                TraceLine { kind: TraceKind::Dialogue, speaker: Some("钦灵".into()), text: "第二句".into(), raw: 2 },
                TraceLine { kind: TraceKind::Player, speaker: Some("我".into()), text: "第三句".into(), raw: 3 },
            ],
        };
        trace.write_to(&dir).unwrap();

        let idx = std::fs::read_to_string(dir.join(TRACE_DIR).join("00-index.md")).unwrap();
        assert!(idx.contains("已走章节：2"), "index 应列出两章: {idx}");
        assert!(idx.contains("去公园"), "index 应带关键选择");

        let ch1 = std::fs::read_to_string(dir.join(TRACE_DIR).join("ch01.md")).unwrap();
        let ch2 = std::fs::read_to_string(dir.join(TRACE_DIR).join("ch02.md")).unwrap();
        assert!(ch1.contains("开场") && ch1.contains("第一句"), "第一章应含前两条台词: {ch1}");
        assert!(!ch1.contains("第二句"), "第一章不该串到第二章台词: {ch1}");
        assert!(ch2.contains("第二句") && ch2.contains("第三句"), "第二章应含后两条: {ch2}");
    }

    /// 系统提示被 classify 丢掉后，`ChapterMark.line`（原始下标）不能直接当
    /// `lines` 的下标用 —— 这个测试钉住那次换算，别再退回按原始下标切章。
    #[test]
    fn chapter_ranges_convert_raw_mark_to_filtered_index() {
        let trace = PlayTrace {
            script_key: String::new(),
            script_name: String::new(),
            chapters: vec![
                ChapterMark { id: "main".into(), line: 0 },
                ChapterMark { id: "main2".into(), line: 4 },
            ],
            choices: vec![],
            generated: vec![],
            vars: Map::new(),
            // raw=2 的那条（Plot/System 提示）已被筛掉
            lines: vec![0, 1, 3, 4, 5]
                .into_iter()
                .map(|raw| TraceLine {
                    kind: TraceKind::Player,
                    speaker: None,
                    text: format!("line{raw}"),
                    raw,
                })
                .collect(),
        };
        assert_eq!(trace.chapter_ranges(), vec![(0, 3), (3, 5)]);
    }
}
