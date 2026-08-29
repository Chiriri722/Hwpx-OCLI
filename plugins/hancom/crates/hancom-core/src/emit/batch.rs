//! `BatchItem` — dump-reader가 stdout에 한 줄씩 쓰는 객체.
//!
//! 필드 스키마 근거: OfficeCLI wiki `command-batch.md` "Input Format" 표.
//! 우리는 `add` / `set`과 자동 seed 정리에 필요한 `remove`만 쓴다.

use serde::Serialize;
use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BatchItem {
    /// `add` / `set` 등.
    pub command: &'static str,
    /// add의 부모 경로.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// add의 요소 타입.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#type: Option<&'static str>,
    /// set/remove의 대상 경로.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// `preserve_order` 기능으로 삽입 순서가 유지된다. 골든파일 안정성에 필요.
    #[serde(skip_serializing_if = "Map::is_empty")]
    pub props: Map<String, Value>,
}

impl BatchItem {
    pub fn add(parent: impl Into<String>, ty: &'static str) -> Self {
        Self {
            command: "add",
            parent: Some(parent.into()),
            r#type: Some(ty),
            path: None,
            props: Map::new(),
        }
    }

    pub fn set(path: impl Into<String>) -> Self {
        Self {
            command: "set",
            parent: None,
            r#type: None,
            path: Some(path.into()),
            props: Map::new(),
        }
    }

    pub fn remove(path: impl Into<String>) -> Self {
        Self {
            command: "remove",
            parent: None,
            r#type: None,
            path: Some(path.into()),
            props: Map::new(),
        }
    }

    /// 문자열 prop을 넣는다. 빈 문자열도 유효한 값이므로 그대로 넣는다.
    pub fn prop(mut self, key: &str, value: impl Into<String>) -> Self {
        self.props
            .insert(key.to_string(), Value::String(value.into()));
        self
    }

    /// `Some`일 때만 넣는다.
    pub fn prop_opt(self, key: &str, value: Option<impl Into<String>>) -> Self {
        match value {
            Some(v) => self.prop(key, v),
            None => self,
        }
    }

    /// bool prop. OfficeCLI는 `--prop bold=true` 형태의 문자열을 받으므로
    /// 문자열로 넣는다. `false`는 기본값이라 생략한다.
    pub fn flag(self, key: &str, on: bool) -> Self {
        if on {
            self.prop(key, "true")
        } else {
            self
        }
    }

    pub fn has_props(&self) -> bool {
        !self.props.is_empty()
    }

    /// JSONL 한 줄. 개행은 포함하지 않는다.
    ///
    /// §5.5: BOM 없음, `\n`만. serde_json은 둘 다 만족한다.
    pub fn to_json_line(&self) -> String {
        serde_json::to_string(self).expect("BatchItem is always serializable")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_emits_command_parent_type() {
        let item = BatchItem::add("/body", "paragraph").prop("text", "안녕");
        let v: Value = serde_json::from_str(&item.to_json_line()).expect("valid json");
        assert_eq!(v["command"], "add");
        assert_eq!(v["parent"], "/body");
        assert_eq!(v["type"], "paragraph");
        assert_eq!(v["props"]["text"], "안녕");
        // set 전용 필드는 나오지 않아야 한다.
        assert!(v.get("path").is_none());
    }

    #[test]
    fn set_emits_path_without_parent_or_type() {
        let item = BatchItem::set("/body/p[1]/r[1]").flag("bold", true);
        let v: Value = serde_json::from_str(&item.to_json_line()).expect("valid json");
        assert_eq!(v["command"], "set");
        assert_eq!(v["path"], "/body/p[1]/r[1]");
        assert!(v.get("parent").is_none());
        assert!(v.get("type").is_none());
        assert_eq!(v["props"]["bold"], "true");
    }

    #[test]
    fn empty_props_are_omitted() {
        let item = BatchItem::add("/body", "paragraph");
        let v: Value = serde_json::from_str(&item.to_json_line()).expect("valid json");
        assert!(v.get("props").is_none(), "empty props must be omitted");
    }

    #[test]
    fn remove_emits_only_command_and_path() {
        let item = BatchItem::remove("/header[1]/p[1]");
        let v: Value = serde_json::from_str(&item.to_json_line()).expect("valid json");
        assert_eq!(v["command"], "remove");
        assert_eq!(v["path"], "/header[1]/p[1]");
        assert!(v.get("parent").is_none());
        assert!(v.get("type").is_none());
        assert!(v.get("props").is_none());
    }

    #[test]
    fn false_flags_are_omitted() {
        let item = BatchItem::add("/body", "paragraph").flag("bold", false);
        assert!(!item.has_props());
    }

    #[test]
    fn prop_order_is_preserved() {
        // 골든파일 비교가 안정적이려면 삽입 순서가 유지돼야 한다.
        let item = BatchItem::add("/body", "paragraph")
            .prop("text", "a")
            .prop("align", "center")
            .prop("size", "10pt");
        let line = item.to_json_line();
        let text_at = line.find("\"text\"").expect("text present");
        let align_at = line.find("\"align\"").expect("align present");
        let size_at = line.find("\"size\"").expect("size present");
        assert!(text_at < align_at && align_at < size_at, "got: {line}");
    }

    #[test]
    fn json_line_is_single_line_without_bom() {
        let item = BatchItem::add("/body", "paragraph").prop("text", "여러\n줄");
        let line = item.to_json_line();
        assert!(!line.starts_with('\u{feff}'));
        // 값 안의 개행은 \n으로 이스케이프되어 실제 개행 문자가 없어야 한다.
        assert!(!line.contains('\n'), "raw newline leaked: {line:?}");
        assert!(!line.contains('\r'));
        assert!(line.contains("\\n"));
    }

    #[test]
    fn korean_text_is_not_escaped_to_ascii() {
        // UTF-8 그대로 나가야 한다 (§5.5).
        let item = BatchItem::add("/body", "paragraph").prop("text", "한글");
        assert!(item.to_json_line().contains("한글"));
    }

    #[test]
    fn prop_opt_skips_none() {
        let item = BatchItem::add("/body", "paragraph")
            .prop_opt("align", None::<String>)
            .prop_opt("style", Some("Heading1"));
        assert!(!item.props.contains_key("align"));
        assert_eq!(item.props["style"], Value::String("Heading1".into()));
    }
}
