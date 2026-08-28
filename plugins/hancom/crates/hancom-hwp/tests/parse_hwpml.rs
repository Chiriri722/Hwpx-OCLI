//! Legacy single-XML HWPML (`.hml`) reader contracts.
//!
//! Element and attribute names follow Hancom's public HWPML revision 1.2
//! specification. These tests intentionally cover both the documented `CHAR`
//! wrapper and direct `TEXT` character data seen in compatible producers.

use std::io::Cursor;

use officecli_hwpx::error::ErrorCode;
use officecli_hwpx::hwpml::read_document_from;
use officecli_hwpx::owpml::model::{Align, Block, Inline, VertAlign};

fn document(body: &str) -> String {
    format!(
        r##"<?xml version="1.0" encoding="UTF-8"?>
<HWPML Version="2.91" SubVersion="11.0.0.0" Style2="embed">
  <HEAD SecCnt="1">
    <DOCSUMMARY><TITLE>metadata must not leak</TITLE></DOCSUMMARY>
    <MAPPINGTABLE>
      <FACENAMELIST>
        <FONTFACE Lang="Hangul" Count="2">
          <FONT Id="0" Type="ttf" Name="함초롬바탕"/>
          <FONT Id="1" Type="ttf" Name="함초롬돋움"/>
        </FONTFACE>
      </FACENAMELIST>
      <CHARSHAPELIST Count="3">
        <CHARSHAPE Id="0" Height="1000" TextColor="#000000" ShadeColor="4294967295">
          <FONTID Hangul="0" Latin="0"/>
        </CHARSHAPE>
        <CHARSHAPE Id="1" Height="1400" TextColor="#FF0000" ShadeColor="4294967295">
          <FONTID Hangul="1" Latin="1"/>
          <BOLD/><ITALIC/><UNDERLINE Type="Bottom" Shape="Solid"/>
          <STRIKEOUT Type="Continuous" Shape="Solid"/><SUPERSCRIPT/>
        </CHARSHAPE>
        <CHARSHAPE Id="2" Height="900"><SUBSCRIPT/></CHARSHAPE>
      </CHARSHAPELIST>
      <PARASHAPELIST Count="2">
        <PARASHAPE Id="0" Align="Justify"><PARAMARGIN/></PARASHAPE>
        <PARASHAPE Id="1" Align="Center">
          <PARAMARGIN Indent="-500" Left="1000" Right="0" Prev="200" Next="300"
                       LineSpacingType="Percent" LineSpacing="160"/>
        </PARASHAPE>
      </PARASHAPELIST>
    </MAPPINGTABLE>
  </HEAD>
  <BODY><SECTION Id="0">{body}</SECTION></BODY>
  <TAIL><SCRIPTCODE><SCRIPTSOURCE>script must not leak</SCRIPTSOURCE></SCRIPTCODE></TAIL>
</HWPML>"##
    )
}

fn parse(xml: &str) -> officecli_hwpx::owpml::model::Document {
    read_document_from(Cursor::new(xml.as_bytes())).expect("HWPML document parses")
}

#[test]
fn reads_documented_char_wrappers_and_character_controls() {
    let xml = document(
        r#"<P ParaShape="0" Style="0">
             <TEXT CharShape="0"><CHAR>한글 &amp; English<TAB/>탭<LINEBREAK/>줄<HYPEN/><NBSPACE/><FWSPACE/></CHAR></TEXT>
           </P>
           <P ParaShape="0"><TEXT CharShape="0"><CHAR>둘째 문단</CHAR></TEXT></P>"#,
    );
    let doc = parse(&xml);
    let texts: Vec<String> = doc.paragraphs().map(|p| p.plain_text()).collect();
    assert_eq!(
        texts,
        vec!["한글 & English\t탭\n줄-\u{00A0}\u{3000}", "둘째 문단"]
    );
}

#[test]
fn accepts_direct_text_children_without_collecting_head_or_tail_metadata() {
    let xml = document(r#"<P ParaShape="0"><TEXT CharShape="0">직접 텍스트</TEXT></P>"#);
    let doc = parse(&xml);
    assert_eq!(
        doc.paragraphs().next().expect("paragraph").plain_text(),
        "직접 텍스트"
    );
    let debug = format!("{doc:?}");
    assert!(!debug.contains("metadata must not leak"));
    assert!(!debug.contains("script must not leak"));
}

#[test]
fn applies_hwpml_character_and_paragraph_styles() {
    let xml = document(
        r#"<P ParaShape="1">
             <TEXT CharShape="0"><CHAR>보통</CHAR></TEXT>
             <TEXT CharShape="1"><CHAR>강조</CHAR></TEXT>
             <TEXT CharShape="2"><CHAR>아래</CHAR></TEXT>
           </P>"#,
    );
    let doc = parse(&xml);
    let paragraph = doc.paragraphs().next().expect("paragraph");
    assert_eq!(paragraph.style.align, Some(Align::Center));
    assert_eq!(paragraph.style.indent_hanging_twip, Some(100));
    assert_eq!(paragraph.style.indent_left_twip, Some(200));
    assert_eq!(paragraph.style.space_before_twip, Some(40));
    assert_eq!(paragraph.style.space_after_twip, Some(60));
    assert_eq!(paragraph.style.line_spacing_ratio, Some(1.6));

    let runs: Vec<_> = paragraph
        .inlines
        .iter()
        .filter_map(|inline| match inline {
            Inline::Text(run) => Some(run),
            _ => None,
        })
        .collect();
    assert_eq!(runs.len(), 3);
    assert_eq!(runs[0].style.font.as_deref(), Some("함초롬바탕"));
    assert_eq!(runs[0].style.size_pt, Some(10.0));
    assert!(runs[1].style.bold);
    assert!(runs[1].style.italic);
    assert!(runs[1].style.underline);
    assert!(runs[1].style.strike);
    assert_eq!(runs[1].style.color.as_deref(), Some("#FF0000"));
    assert_eq!(runs[1].style.font.as_deref(), Some("함초롬돋움"));
    assert_eq!(runs[1].style.size_pt, Some(14.0));
    assert_eq!(runs[1].style.vert_align, Some(VertAlign::Superscript));
    assert_eq!(runs[2].style.vert_align, Some(VertAlign::Subscript));
}

#[test]
fn skips_unsupported_control_subtrees_instead_of_leaking_their_metadata() {
    let xml = document(
        r#"<P ParaShape="0"><TEXT CharShape="0">
             <SECDEF><PARAMETERSET><STRING>control metadata</STRING></PARAMETERSET></SECDEF>
             <CHAR>보이는 본문</CHAR>
           </TEXT></P>"#,
    );
    let doc = parse(&xml);
    let paragraph = doc.paragraphs().next().expect("paragraph");
    assert_eq!(paragraph.plain_text().trim(), "보이는 본문");
    assert!(!paragraph.plain_text().contains("control metadata"));
}

#[test]
fn rejects_non_hwpml_xml_and_truncated_xml() {
    let non_hwpml = read_document_from(Cursor::new(b"<html><body>not hwpml</body></html>"))
        .expect_err("generic XML must not be accepted as HWPML");
    assert!(non_hwpml.message.contains("HWPML"), "got: {non_hwpml}");

    let truncated = read_document_from(Cursor::new(b"<HWPML><BODY><SECTION>"))
        .expect_err("truncated XML must fail");
    assert!(truncated.message.contains("xml"), "got: {truncated}");
}

#[test]
fn reads_utf16_little_and_big_endian_documents() {
    fn encoded(xml: &str, little_endian: bool) -> Vec<u8> {
        let mut bytes = if little_endian {
            vec![0xFF, 0xFE]
        } else {
            vec![0xFE, 0xFF]
        };
        for unit in xml.encode_utf16() {
            let pair = if little_endian {
                unit.to_le_bytes()
            } else {
                unit.to_be_bytes()
            };
            bytes.extend_from_slice(&pair);
        }
        bytes
    }

    let xml = r#"<?xml version="1.0" encoding="UTF-16"?>
<HWPML Version="2.91"><HEAD SecCnt="1"/><BODY><SECTION Id="0">
<P ParaShape="0"><TEXT CharShape="0"><CHAR>UTF-16 한글</CHAR></TEXT></P>
</SECTION></BODY></HWPML>"#;
    for little_endian in [true, false] {
        let doc = read_document_from(Cursor::new(encoded(xml, little_endian)))
            .expect("UTF-16 HWPML parses");
        assert_eq!(
            doc.paragraphs().next().expect("paragraph").plain_text(),
            "UTF-16 한글"
        );
    }
}

#[test]
fn maps_basic_hwpml_table_between_surrounding_text() {
    let xml = document(
        r#"<P ParaShape="0"><TEXT CharShape="0"><CHAR>표 앞</CHAR>
          <TABLE BorderFill="1" CellSpacing="0" ColCount="2" RowCount="1">
            <SHAPEOBJECT><SIZE Width="8000" Height="2000"/></SHAPEOBJECT>
            <ROW>
              <CELL BorderFill="1" ColAddr="0" ColSpan="1" RowAddr="0" RowSpan="1" Width="3000">
                <PARALIST><P ParaShape="0"><TEXT CharShape="0"><CHAR>A1</CHAR></TEXT></P></PARALIST>
              </CELL>
              <CELL BorderFill="1" ColAddr="1" ColSpan="1" RowAddr="0" RowSpan="1" Width="5000">
                <PARALIST><P ParaShape="0"><TEXT CharShape="0"><CHAR>B1</CHAR></TEXT></P></PARALIST>
              </CELL>
            </ROW>
          </TABLE><CHAR>표 뒤</CHAR></TEXT></P>"#,
    );
    let doc = parse(&xml);
    assert_eq!(doc.blocks.len(), 3, "paragraph/table/paragraph ordering");
    let Block::Paragraph(before) = &doc.blocks[0] else {
        panic!("first block must be a paragraph");
    };
    assert_eq!(before.plain_text().trim(), "표 앞");
    let Block::Table(table) = &doc.blocks[1] else {
        panic!("second block must be a table");
    };
    assert_eq!((table.rows, table.cols), (1, 2));
    assert_eq!(table.col_widths_twip, vec![600, 1000]);
    assert_eq!(table.cell_at(0, 0).expect("A1").plain_text(), "A1");
    assert_eq!(table.cell_at(0, 1).expect("B1").plain_text(), "B1");
    let Block::Paragraph(after) = &doc.blocks[2] else {
        panic!("third block must be a paragraph");
    };
    assert_eq!(after.plain_text().trim(), "표 뒤");
}

#[test]
fn content_bearing_controls_fail_instead_of_disappearing_silently() {
    for control in ["PICTURE", "EQUATION", "FOOTNOTE", "RECTANGLE"] {
        let xml = document(&format!(
            r#"<P ParaShape="0"><TEXT CharShape="0"><CHAR>앞</CHAR><{control}><STRING>중요 내용</STRING></{control}><CHAR>뒤</CHAR></TEXT></P>"#
        ));
        let error = read_document_from(Cursor::new(xml.as_bytes()))
            .expect_err("unsupported content must fail closed");
        assert_eq!(error.code, ErrorCode::UnsupportedFeature);
        assert!(error.message.contains(control), "got: {error}");
    }
}

#[test]
fn content_bearing_controls_beside_text_also_fail_closed() {
    let xml = document(
        r#"<P ParaShape="0">
             <TEXT CharShape="0"><CHAR>앞</CHAR></TEXT>
             <EQUATION><SCRIPT>중요한 수식</SCRIPT></EQUATION>
             <TEXT CharShape="0"><CHAR>뒤</CHAR></TEXT>
           </P>"#,
    );
    let error = read_document_from(Cursor::new(xml.as_bytes()))
        .expect_err("a sibling content control must not disappear");
    assert_eq!(error.code, ErrorCode::UnsupportedFeature);
    assert!(error.message.contains("EQUATION"), "got: {error}");
}

#[test]
fn preserves_empty_hwpml_paragraphs_as_blank_lines() {
    let xml = document(
        r#"<P ParaShape="0"><TEXT CharShape="0"><CHAR>위</CHAR></TEXT></P>
           <P ParaShape="0"><TEXT CharShape="0"><CHAR></CHAR></TEXT></P>
           <P ParaShape="0"><TEXT CharShape="0"><CHAR>아래</CHAR></TEXT></P>"#,
    );
    let doc = parse(&xml);
    let texts: Vec<String> = doc.paragraphs().map(|p| p.plain_text()).collect();
    assert_eq!(texts, vec!["위", "", "아래"]);
}

#[test]
fn accepts_documented_versions_and_rejects_unknown_ones() {
    for version in ["2.8", "2.9", "2.91"] {
        let xml = document("").replacen("Version=\"2.91\"", &format!("Version=\"{version}\""), 1);
        read_document_from(Cursor::new(xml.as_bytes())).expect("documented version parses");
    }

    let xml = document("").replacen("Version=\"2.91\"", "Version=\"3.0\"", 1);
    let error = read_document_from(Cursor::new(xml.as_bytes()))
        .expect_err("an unknown HWPML version must fail explicitly");
    assert_eq!(error.code, ErrorCode::UnsupportedFeature);
    assert!(error.message.contains("3.0"), "got: {error}");
}

#[test]
fn content_bearing_elements_outside_paragraphs_fail_closed() {
    let xml = r#"<HWPML Version="2.91"><BODY><SECTION>
        <DRAWINGOBJECT><DRAWTEXT>숨겨지면 안 되는 본문</DRAWTEXT></DRAWINGOBJECT>
    </SECTION></BODY></HWPML>"#;
    let error = read_document_from(Cursor::new(xml.as_bytes()))
        .expect_err("unknown BODY content must not disappear");
    assert_eq!(error.code, ErrorCode::UnsupportedFeature);
    assert!(error.message.contains("DRAWINGOBJECT"), "got: {error}");
}

#[test]
fn content_bearing_elements_directly_under_tables_fail_closed() {
    let xml = document(
        r#"<P ParaShape="0"><TEXT CharShape="0"><TABLE ColCount="1" RowCount="1">
          <CAPTION><P ParaShape="0"><TEXT CharShape="0"><CHAR>표 제목</CHAR></TEXT></P></CAPTION>
        </TABLE></TEXT></P>"#,
    );
    let error = read_document_from(Cursor::new(xml.as_bytes()))
        .expect_err("unknown table content must not disappear");
    assert_eq!(error.code, ErrorCode::UnsupportedFeature);
    assert!(error.message.contains("CAPTION"), "got: {error}");
}

#[test]
fn ignores_formatting_whitespace_between_text_children_but_keeps_char_spaces() {
    let xml = document(
        r#"<P ParaShape="0"><TEXT CharShape="0">
             <CHAR> </CHAR>
             <CHAR>앞</CHAR>
             <CHAR>뒤</CHAR>
           </TEXT></P>"#,
    );
    let doc = parse(&xml);
    assert_eq!(
        doc.paragraphs().next().expect("paragraph").plain_text(),
        " 앞뒤"
    );
}
