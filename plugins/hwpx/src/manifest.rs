//! 프로토콜 §4 매니페스트. `<plugin> --info`가 출력하는 JSON 객체.

use serde::Serialize;

pub const PLUGIN_NAME: &str = "officecli-hwpx";
pub const PLUGIN_VERSION: &str = env!("CARGO_PKG_VERSION");
/// §4.1: v1 플러그인은 반드시 1. 불일치 시 메인이 exit 5로 거부한다.
pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Serialize)]
pub struct IdleTimeout {
    /// §4.2: 필수, 양의 정수. 0은 매니페스트에서 금지.
    pub default: u32,
    pub verbs: VerbTimeouts,
}

#[derive(Debug, Serialize)]
pub struct VerbTimeouts {
    /// §4.2 권장값: dump-reader.dump = 30초 (스트리밍 emit이 idle을 낮게 유지).
    pub dump: u32,
}

#[derive(Debug, Serialize)]
pub struct Manifest {
    pub name: &'static str,
    pub version: &'static str,
    pub protocol: u32,
    pub kinds: Vec<&'static str>,
    pub extensions: Vec<&'static str>,
    /// §4.1/§4.3: dump-reader는 필수. docx/xlsx/pptx 중 하나. ADR-3 참고.
    pub target: &'static str,
    /// §4.1: 진단용 태그. 호스트는 이 값으로 분기하지 않는다.
    pub runtime: &'static str,
    pub idle_timeout_seconds: IdleTimeout,
    pub description: &'static str,
    pub license: &'static str,
    /// §4.3 선택 필드. 현재 커버하는 기능 태그.
    pub supports: Vec<&'static str>,
}

impl Default for Manifest {
    fn default() -> Self {
        Self {
            name: PLUGIN_NAME,
            version: PLUGIN_VERSION,
            protocol: PROTOCOL_VERSION,
            kinds: vec!["dump-reader"],
            extensions: vec![".hwpx"],
            target: "docx",
            runtime: "rust",
            idle_timeout_seconds: IdleTimeout {
                default: 60,
                verbs: VerbTimeouts { dump: 30 },
            },
            description: "HWPX (OWPML) dump-reader — converts .hwpx into officecli docx commands",
            license: "MIT",
            supports: vec![
                "paragraphs",
                "runs",
                "tables",
                "images",
                "alignment",
                "cell-merge",
            ],
        }
    }
}

impl Manifest {
    /// §5.5: BOM 없는 UTF-8, 개행은 `\n`. 한 줄짜리 JSON 객체를 돌려준다.
    pub fn to_json_line(&self) -> String {
        // serde_json은 BOM을 붙이지 않고 ASCII 개행만 쓴다.
        serde_json::to_string(self).expect("manifest is statically serializable")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn manifest_json() -> Value {
        serde_json::from_str(&Manifest::default().to_json_line()).expect("valid json")
    }

    #[test]
    fn declares_all_required_fields() {
        // §4.1 필수 필드 표
        let m = manifest_json();
        for key in [
            "name",
            "version",
            "protocol",
            "kinds",
            "extensions",
            "idle_timeout_seconds",
            "runtime",
        ] {
            assert!(m.get(key).is_some(), "missing required field: {key}");
        }
    }

    #[test]
    fn dump_reader_declares_target() {
        // §4.1: target은 dump-reader에 필수이며 docx/xlsx/pptx 중 하나
        let m = manifest_json();
        let target = m["target"].as_str().expect("target must be a string");
        assert!(
            ["docx", "xlsx", "pptx"].contains(&target),
            "invalid target: {target}"
        );
    }

    #[test]
    fn protocol_version_is_one() {
        assert_eq!(manifest_json()["protocol"], 1);
    }

    #[test]
    fn extensions_keep_leading_dot() {
        // §4.1: extensions는 leading dot 포함 (`[".doc"]`)
        let m = manifest_json();
        for ext in m["extensions"].as_array().expect("array") {
            let s = ext.as_str().expect("string");
            assert!(s.starts_with('.'), "extension must start with dot: {s}");
        }
    }

    #[test]
    fn name_is_kebab_case() {
        // §4.1: kebab-case 안정 식별자
        let name = PLUGIN_NAME;
        assert!(
            name.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
            "not kebab-case: {name}"
        );
    }

    #[test]
    fn idle_timeout_default_is_positive() {
        // §4.2: 0은 매니페스트에서 금지
        let m = manifest_json();
        let d = m["idle_timeout_seconds"]["default"]
            .as_u64()
            .expect("default must be an integer");
        assert!(d > 0, "idle timeout default must be positive, got {d}");
    }

    #[test]
    fn all_json_keys_are_snake_case() {
        // §5.5: 모든 JSON 키는 snake_case
        fn check(v: &Value) {
            if let Value::Object(map) = v {
                for (k, val) in map {
                    assert!(
                        k.chars()
                            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
                        "key not snake_case: {k}"
                    );
                    check(val);
                }
            }
        }
        check(&manifest_json());
    }

    #[test]
    fn version_is_semver_triple() {
        let v = PLUGIN_VERSION;
        let parts: Vec<&str> = v.split('.').collect();
        assert_eq!(parts.len(), 3, "not semver: {v}");
        for p in parts {
            assert!(
                p.chars().next().is_some_and(|c| c.is_ascii_digit()),
                "not semver: {v}"
            );
        }
    }

    #[test]
    fn kinds_declares_dump_reader_kebab_case() {
        let m = manifest_json();
        let kinds: Vec<&str> = m["kinds"]
            .as_array()
            .expect("array")
            .iter()
            .map(|k| k.as_str().expect("string"))
            .collect();
        assert_eq!(kinds, vec!["dump-reader"]);
    }

    #[test]
    fn does_not_declare_reserved_kinds() {
        // §2.4: engine / transformer는 v1에서 선언 금지
        let m = manifest_json();
        for k in m["kinds"].as_array().expect("array") {
            let s = k.as_str().expect("string");
            assert!(
                !["engine", "transformer"].contains(&s),
                "reserved kind declared: {s}"
            );
        }
    }

    #[test]
    fn json_line_has_no_bom_and_no_crlf() {
        // §5.5
        let s = Manifest::default().to_json_line();
        assert!(!s.starts_with('\u{feff}'), "must not emit BOM");
        assert!(!s.contains('\r'), "must not emit CR");
        assert!(!s.contains('\n'), "manifest must be a single line");
    }
}
