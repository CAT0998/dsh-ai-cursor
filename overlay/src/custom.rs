//! The reserved custom section, driven by a JSON file.
//!
//! The point of this module is that a user — or an AI acting for them — can add
//! panel controls without touching Rust. The file lives next to the artwork:
//!
//! ```json
//! {
//!   "title": "我的模块",
//!   "rows": [
//!     { "type": "label",  "text": "常用操作" },
//!     { "type": "button", "text": "回到矢量光标", "action": "art_prev0" },
//!     { "type": "button", "text": "停止 AI", "action": "stop" },
//!     { "type": "button", "text": "半透明", "set": { "opacity": 0.5 } }
//!   ]
//! }
//! ```
//!
//! `action` names an existing panel action (the same ids the built-in widgets
//! use). `set` is a small, explicit escape hatch for numeric settings. Nothing
//! here can execute code, so a file that arrives from elsewhere cannot do more
//! than press the panel's own buttons.

use std::fs;
use std::time::SystemTime;

use serde_json::Value;

#[derive(Clone, Debug)]
pub enum CustomRow {
    Label(String),
    Button { text: String, action: String, set: Option<Value> },
}

#[derive(Clone, Debug, Default)]
pub struct CustomSpec {
    pub title: Option<String>,
    pub rows: Vec<CustomRow>,
    /// Parse/IO problem, shown in the panel instead of silently doing nothing.
    pub error: Option<String>,
    pub mtime_ms: u128,
}

impl CustomSpec {
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn action_of(&self, index: usize) -> Option<(&str, Option<&Value>)> {
        match self.rows.get(index) {
            Some(CustomRow::Button { action, set, .. }) => Some((action.as_str(), set.as_ref())),
            _ => None,
        }
    }
}

pub fn file_path(asset_dir: &str) -> Option<std::path::PathBuf> {
    if asset_dir.trim().is_empty() {
        return None;
    }
    Some(std::path::Path::new(asset_dir).join("custom-section.json"))
}

fn mtime_ms(path: &std::path::Path) -> u128 {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// Load (or reload) the spec. A missing file is not an error — it is the normal
/// "slot not filled in yet" state, which the panel explains in words.
pub fn load(asset_dir: &str) -> CustomSpec {
    let Some(path) = file_path(asset_dir) else {
        return CustomSpec::default();
    };
    let mtime = mtime_ms(&path);
    if !path.exists() {
        return CustomSpec { mtime_ms: mtime, ..Default::default() };
    }
    let raw = match fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            return CustomSpec {
                error: Some(format!("读取失败：{e}")),
                mtime_ms: mtime,
                ..Default::default()
            }
        }
    };
    match parse(&raw) {
        Ok(mut spec) => {
            spec.mtime_ms = mtime;
            spec
        }
        Err(e) => CustomSpec { error: Some(e), mtime_ms: mtime, ..Default::default() },
    }
}

/// Pure parser: separated from IO so it is easy to reason about and test.
pub fn parse(raw: &str) -> Result<CustomSpec, String> {
    let v: Value = serde_json::from_str(raw).map_err(|e| format!("JSON 解析失败：{e}"))?;
    let Some(obj) = v.as_object() else {
        return Err("顶层必须是一个对象 { \"title\", \"rows\" }".to_string());
    };
    let title = obj.get("title").and_then(|t| t.as_str()).map(|s| s.to_string());
    let mut rows = Vec::new();
    if let Some(list) = obj.get("rows").and_then(|r| r.as_array()) {
        for (i, item) in list.iter().enumerate() {
            let kind = item.get("type").and_then(|t| t.as_str()).unwrap_or("label");
            let text = item
                .get("text")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string();
            match kind {
                "label" => rows.push(CustomRow::Label(text)),
                "button" => {
                    let action = item
                        .get("action")
                        .and_then(|a| a.as_str())
                        .unwrap_or("")
                        .to_string();
                    let set = item.get("set").cloned();
                    if action.is_empty() && set.is_none() {
                        return Err(format!("第 {i} 行是 button，但既没有 action 也没有 set"));
                    }
                    rows.push(CustomRow::Button { text, action, set });
                }
                other => return Err(format!("第 {i} 行的 type「{other}」不支持（label | button）")),
            }
        }
    } else if obj.get("rows").is_some() {
        return Err("rows 必须是数组".to_string());
    }
    if rows.len() > 12 {
        return Err(format!("最多 12 行，当前 {} 行", rows.len()));
    }
    Ok(CustomSpec { title, rows, error: None, mtime_ms: 0 })
}

/// What the panel shows when the slot has not been filled in yet.
pub fn hint_lines(asset_dir: &str) -> Vec<String> {
    let path = file_path(asset_dir)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "（未配置素材目录）".to_string());
    vec![
        "空位：还没有自定义内容".to_string(),
        format!("把 custom-section.json 放进：{path}"),
        r#"{"title":"我的模块","rows":[{"type":"button","text":"停止","action":"stop"}]}"#.to_string(),
        "支持 type: label / button（action 用面板已有动作；或 set 直接改数值）".to_string(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_labels_and_buttons() {
        let spec = parse(
            r#"{"title":"T","rows":[{"type":"label","text":"hi"},
                                   {"type":"button","text":"go","action":"stop"}]}"#,
        )
        .unwrap();
        assert_eq!(spec.title.as_deref(), Some("T"));
        assert_eq!(spec.rows.len(), 2);
        assert_eq!(spec.action_of(1).map(|(a, _)| a), Some("stop"));
        assert!(spec.action_of(0).is_none());
    }

    #[test]
    fn rejects_bad_shapes_with_a_reason() {
        assert!(parse("not json").unwrap_err().contains("JSON"));
        assert!(parse("[]").unwrap_err().contains("顶层"));
        assert!(parse(r#"{"rows":"x"}"#).unwrap_err().contains("数组"));
        assert!(parse(r#"{"rows":[{"type":"slider"}]}"#).unwrap_err().contains("不支持"));
        assert!(parse(r#"{"rows":[{"type":"button","text":"x"}]}"#).unwrap_err().contains("action"));
    }

    #[test]
    fn caps_the_row_count() {
        let rows = (0..13)
            .map(|i| format!(r#"{{"type":"label","text":"{i}"}}"#))
            .collect::<Vec<_>>()
            .join(",");
        assert!(parse(&format!(r#"{{"rows":[{rows}]}}"#)).unwrap_err().contains("最多"));
    }

    #[test]
    fn accepts_set_as_the_only_effect() {
        let spec = parse(r#"{"rows":[{"type":"button","text":"x","set":{"opacity":0.5}}]}"#).unwrap();
        assert_eq!(spec.action_of(0).map(|(a, _)| a), Some(""));
        assert!(spec.action_of(0).unwrap().1.is_some());
    }
}
