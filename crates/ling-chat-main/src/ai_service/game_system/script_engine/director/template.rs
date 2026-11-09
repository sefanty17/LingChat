//! 模板续写：不调 LLM，写一个最小可运行章节，验证「写盘即被执行」。

use std::path::Path;

use anyhow::{Context, Result};

pub fn write_template_chapter(script_dir: &Path, chapter_id: &str, seq: u32) -> Result<()> {
    let path = crate::utils::script_paths::resolve_chapter_file(script_dir, chapter_id, false)
        .map_err(anyhow::Error::msg)?;

    let text = format!(
        "name: 自动续写 {seq}\nevents:\n  - type: narration\n    text: |\n      （这是自动续写的第 {seq} 段剧情。）\n  - type: chapter_end\n    end_type: linear\n    next_chapter: end\n"
    );

    crate::utils::yaml_file::ensure_parent_dir(&path).map_err(anyhow::Error::msg)?;

    let mut tmp = path.as_os_str().to_os_string();
    tmp.push(".tmp");
    let tmp = std::path::PathBuf::from(tmp);
    std::fs::write(&tmp, text.as_bytes()).with_context(|| format!("写入模板章节失败: {:?}", tmp))?;
    std::fs::rename(&tmp, &path).with_context(|| format!("章节落盘失败: {:?}", path))?;
    Ok(())
}
