//! 골든파일 회귀 테스트.
//!
//! 서식·표·이미지·줄바꿈을 모두 담은 표준 문서 하나를 정해 두고, 전체
//! 파이프라인(ZIP → OWPML → 문서모델 → JSONL) 출력을 고정한다.
//! 리팩터링으로 출력이 의도치 않게 바뀌면 여기서 걸린다.
//!
//! 골든파일 갱신:
//! ```sh
//! UPDATE_GOLDEN=1 cargo test --test golden
//! ```
//! 갱신 후에는 diff를 눈으로 검토하고 커밋해야 한다.

mod common;

use std::io::Cursor;
use std::path::PathBuf;

use common::*;
use officecli_hwpx::emit::stream_document;
use officecli_hwpx::owpml::read_document_from;

fn golden_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
        .join(name)
}

/// 서식·표·이미지·줄바꿈을 한 문서에 모은 표준 픽스처.
fn canonical_document() -> HwpxBuilder {
    let mut b = HwpxBuilder::new();

    let plain = b.char_pr(CharPr::plain());
    let heading = b.char_pr(CharPr {
        height: Some(1600),
        bold: true,
        font_hangul: Some("함초롬돋움".into()),
        ..CharPr::plain()
    });
    let emphasis = b.char_pr(CharPr {
        height: Some(1000),
        text_color: Some("#C00000".into()),
        italic: true,
        underline: Some("BOTTOM".into()),
        ..Default::default()
    });

    // 주의: table()/picture()/CellSpec 헬퍼는 paraPr "0"을 참조한다.
    // 따라서 id 0을 먼저 무서식으로 등록해야 셀과 이미지 문단이 엉뚱한
    // 정렬을 물려받지 않는다.
    let default_pr = b.para_pr(ParaPr::default());
    let centered = b.para_pr(ParaPr::centered());
    let body_pr = b.para_pr(ParaPr {
        indent_first: Some(1000),
        line_spacing_percent: Some(160),
        space_after: Some(600),
        ..Default::default()
    });

    let mut body = String::new();

    // 1. 제목 — 단일 서식이므로 한 줄로 병합돼야 한다.
    body.push_str(&para(&heading, &centered, "2026년 분기 보고서"));

    // 2. 혼합 서식 문단 — 문단 + 런들로 쪼개져야 한다.
    body.push_str(&para_with_runs(
        &body_pr,
        &[
            (&plain, "이번 분기 매출은 "),
            (&emphasis, "전년 대비 12% 증가"),
            (&plain, "했습니다."),
        ],
    ));

    // 3. 문단 내 줄바꿈 — \n이 아니라 \v로 나가야 한다.
    body.push_str(&para_with_linebreak(
        &plain,
        &default_pr,
        "첫 번째 줄",
        "같은 문단의 둘째 줄",
    ));

    // 4. 표 — 병합 셀과 배경색 포함.
    body.push_str(&table(
        3,
        3,
        &[
            CellSpec::new(0, 0, "구분").span(1, 1).fill("#EDEDED"),
            CellSpec::new(0, 1, "1분기").fill("#EDEDED"),
            CellSpec::new(0, 2, "2분기").fill("#EDEDED"),
            CellSpec::new(1, 0, "매출"),
            CellSpec::new(1, 1, "1,200"),
            CellSpec::new(1, 2, "1,344"),
            CellSpec::new(2, 0, "비고").span(1, 3),
        ],
    ));

    // 5. 이미지.
    body.push_str(&picture("chart1", 7200, 3600, Some("분기 매출 추이")));

    // 6. 특수문자 — 엔티티 해제 확인.
    body.push_str(&para(&plain, &default_pr, "각주 & 참고 <자료> \"인용\""));

    b.section(body);
    b.bindata("chart1", "chart1.png", tiny_png(), "image/png");
    b
}

fn actual_jsonl() -> String {
    let bytes = canonical_document().build();
    let doc = read_document_from(Cursor::new(bytes)).expect("document parses");
    let mut out = Vec::new();
    stream_document(&doc, &mut out).expect("streams");
    String::from_utf8(out).expect("utf-8 output")
}

#[test]
fn canonical_document_matches_golden_output() {
    let actual = actual_jsonl();
    let path = golden_path("canonical.jsonl");

    if std::env::var("UPDATE_GOLDEN").is_ok() {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(&path, &actual).expect("write golden");
        eprintln!("golden updated: {}", path.display());
        return;
    }

    let expected = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "golden file missing ({e}). regenerate with:\n  \
             UPDATE_GOLDEN=1 cargo test --test golden\npath: {}",
            path.display()
        )
    });
    // Git may leave an existing Windows worktree copy as CRLF even after the
    // fixture is pinned to LF in .gitattributes. Output EOLs are checked below;
    // this comparison is only about the JSONL contents.
    let expected = expected.replace("\r\n", "\n");

    if actual != expected {
        // 어느 줄이 달라졌는지 바로 보이게 한다.
        let a: Vec<&str> = actual.lines().collect();
        let e: Vec<&str> = expected.lines().collect();
        let mut report = String::new();
        for i in 0..a.len().max(e.len()) {
            let av = a.get(i).copied().unwrap_or("<missing>");
            let ev = e.get(i).copied().unwrap_or("<missing>");
            if av != ev {
                report.push_str(&format!(
                    "line {}:\n  expected: {ev}\n  actual:   {av}\n",
                    i + 1
                ));
            }
        }
        panic!(
            "golden mismatch ({} expected lines vs {} actual)\n{report}",
            e.len(),
            a.len()
        );
    }
}

#[test]
fn golden_output_satisfies_protocol_invariants() {
    // 골든파일이 갱신되더라도 계약은 깨질 수 없다.
    let actual = actual_jsonl();
    assert!(!actual.is_empty(), "expected output");
    assert!(!actual.contains('\r'), "CR is forbidden (§5.5)");
    assert!(
        !actual.trim_start().starts_with('['),
        "no top-level array (§5.1)"
    );
    assert_eq!(actual.chars().last(), Some('\n'), "last line terminated");

    for (i, line) in actual.lines().enumerate() {
        let v: serde_json::Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("line {} is not valid JSON: {e}\n{line}", i + 1));
        assert!(v.is_object(), "line {} must be an object", i + 1);

        // 표면별 줄바꿈 불변식 (officecli v1.0.143 실측):
        //   문단·런 `add` → raw `\n` 금지 (문단이 쪼개짐)
        //   셀 `set`      → `\v` 금지 (XML 불법 문자로 거부)
        let is_cell = v.get("command").and_then(|c| c.as_str()) == Some("set");
        if let Some(props) = v.get("props").and_then(|p| p.as_object()) {
            for (k, val) in props {
                let Some(s) = val.as_str() else { continue };
                if is_cell {
                    assert!(
                        !s.contains('\u{000B}'),
                        "line {} cell prop {k} has XML-illegal \\v",
                        i + 1
                    );
                } else {
                    assert!(!s.contains('\n'), "line {} prop {k} has raw newline", i + 1);
                }
                assert!(!s.contains('\r'), "line {} prop {k} has CR", i + 1);
            }
        }
    }
}

#[test]
fn golden_document_covers_intended_features() {
    // 골든 픽스처가 실제로 모든 기능을 담고 있는지 확인한다.
    // 픽스처가 조용히 빈약해지면 회귀 테스트가 의미를 잃는다.
    let actual = actual_jsonl();

    assert!(
        actual.contains(r#""type":"paragraph""#),
        "paragraphs missing"
    );
    assert!(actual.contains(r#""type":"run""#), "runs missing");
    assert!(actual.contains(r#""type":"table""#), "table missing");
    assert!(actual.contains(r#""type":"picture""#), "picture missing");
    assert!(actual.contains(r#""command":"set""#), "cell sets missing");
    assert!(actual.contains(r#""bold":"true""#), "bold missing");
    assert!(actual.contains(r#""italic":"true""#), "italic missing");
    assert!(actual.contains(r#""align":"center""#), "alignment missing");
    assert!(actual.contains(r#""colspan""#), "cell merge missing");
    assert!(actual.contains(r#""fill""#), "cell fill missing");
    assert!(
        actual.contains("data:image/png;base64,"),
        "data URI missing"
    );
    assert!(actual.contains("\\u000b"), "soft line break missing");
    assert!(actual.contains("각주 & 참고"), "entity unescaping missing");
    assert!(actual.contains("함초롬돋움"), "font name missing");
}
