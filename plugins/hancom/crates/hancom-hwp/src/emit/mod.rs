//! BatchItem 생성과 JSONL 스트리밍.

pub mod batch;
pub mod word;

use std::io::Write;

use crate::error::Result;
use crate::owpml::model::Document;

/// 문서를 JSONL로 스트리밍한다.
///
/// §2.1 "dump-reader plugins MUST emit one batch item per line, flushed
/// individually." — 행마다 flush해야 호스트 감시견이 활동 신호를 받는다.
/// §5.5 개행은 `\n`. `writeln!`은 플랫폼 무관하게 `\n`만 쓴다.
pub fn stream_document<W: Write>(doc: &Document, out: &mut W) -> Result<usize> {
    word::try_emit_document(doc, |item| -> Result<()> {
        writeln!(out, "{}", item.to_json_line())?;
        out.flush()?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::owpml::model::{Block, CharStyle, Inline, Paragraph, ParaStyle, TextRun};

    fn sample() -> Document {
        Document {
            blocks: vec![
                Block::Paragraph(Paragraph {
                    style: ParaStyle::default(),
                    inlines: vec![Inline::Text(TextRun {
                        text: "첫 문단".into(),
                        style: CharStyle::default(),
                    })],
                }),
                Block::Paragraph(Paragraph {
                    style: ParaStyle::default(),
                    inlines: vec![Inline::Text(TextRun {
                        text: "둘째 문단".into(),
                        style: CharStyle::default(),
                    })],
                }),
            ],
        }
    }

    #[test]
    fn writes_one_json_object_per_line() {
        let mut buf = Vec::new();
        let n = stream_document(&sample(), &mut buf).expect("streams");
        assert_eq!(n, 2);
        let s = String::from_utf8(buf).expect("utf-8");
        let lines: Vec<&str> = s.lines().collect();
        assert_eq!(lines.len(), 2);
        for line in lines {
            let v: serde_json::Value = serde_json::from_str(line).expect("each line is json");
            assert!(v.is_object(), "each line must be an object, not an array");
        }
    }

    #[test]
    fn output_is_not_a_top_level_array() {
        // §5.1: 최상위 배열은 corrupt_batch로 거부된다.
        let mut buf = Vec::new();
        stream_document(&sample(), &mut buf).expect("streams");
        let s = String::from_utf8(buf).expect("utf-8");
        assert!(!s.trim_start().starts_with('['), "must not be an array");
    }

    #[test]
    fn uses_lf_only_and_no_bom() {
        // §5.5
        let mut buf = Vec::new();
        stream_document(&sample(), &mut buf).expect("streams");
        assert!(!buf.starts_with(&[0xEF, 0xBB, 0xBF]), "must not emit BOM");
        assert!(!buf.contains(&b'\r'), "must not emit CR");
        assert_eq!(buf.last(), Some(&b'\n'), "last line must be terminated");
    }

    #[test]
    fn empty_document_emits_nothing() {
        let mut buf = Vec::new();
        let n = stream_document(&Document::default(), &mut buf).expect("streams");
        assert_eq!(n, 0);
        assert!(buf.is_empty());
    }

    #[test]
    fn block_sink_stops_before_emitting_later_items_after_an_error() {
        let mut calls = 0usize;
        let error = word::try_emit_document(&sample(), |_item| {
            calls += 1;
            Err("stop")
        })
        .expect_err("sink failure must stop emission");

        assert_eq!(error, "stop");
        assert_eq!(calls, 1, "later blocks must not be materialized after failure");
    }
}
