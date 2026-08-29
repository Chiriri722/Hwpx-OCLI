//! Legacy single-XML HWPML (`.hml`) reader contracts.
//!
//! Element and attribute names follow Hancom's public HWPML revision 1.2
//! specification. Only character data inside the documented `CHAR` wrapper is
//! projected into the shared document model.

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
fn reads_documented_char_wrappers_and_supported_character_controls() {
    let xml = document(
        r#"<P ParaShape="0" Style="0">
             <TEXT CharShape="0"><CHAR>한글 &amp; English<TAB/>탭<LINEBREAK/>줄<NBSPACE/>끝</CHAR></TEXT>
           </P>
           <P ParaShape="0"><TEXT CharShape="0"><CHAR>둘째 문단</CHAR></TEXT></P>"#,
    );
    let doc = parse(&xml);
    let texts: Vec<String> = doc.paragraphs().map(|p| p.plain_text()).collect();
    assert_eq!(texts, vec!["한글 & English\t탭\n줄\u{00A0}끝", "둘째 문단"]);
}

#[test]
fn accepts_xml_character_references_but_rejects_undeclared_entities() {
    let xml = document(r#"<P><TEXT><CHAR>&lt;기본&gt; &#x1F600; &#44032;</CHAR></TEXT></P>"#);
    let doc = parse(&xml);
    assert_eq!(
        doc.paragraphs().next().expect("paragraph").plain_text(),
        "<기본> 😀 가"
    );

    for entity in ["&undeclared;", "&#x110000;", "&#0;"] {
        let xml = document(&format!("<P><TEXT><CHAR>{entity}</CHAR></TEXT></P>"));
        let error = read_document_from(Cursor::new(xml.as_bytes()))
            .expect_err("undeclared or invalid XML entities must not become visible text");
        assert_eq!(error.code, ErrorCode::CorruptInput);
        assert!(error.message.contains("reference") || error.message.contains("entity"));
    }

    for entity in ["&undeclared;", "&#0;"] {
        let xml = format!("<HWPML Version=\"2.91\" Payload=\"{entity}\"><BODY/></HWPML>");
        let error = read_document_from(Cursor::new(xml.as_bytes()))
            .expect_err("invalid references in otherwise unused attributes still break XML");
        assert_eq!(error.code, ErrorCode::CorruptInput);
    }
}

#[test]
fn ambiguous_hyphen_and_fixed_width_space_fail_closed() {
    for control in ["HYPEN", "FWSPACE"] {
        let xml = document(&format!(
            r#"<P ParaShape="0"><TEXT CharShape="0"><CHAR>앞<{control}/>뒤</CHAR></TEXT></P>"#
        ));
        let error = read_document_from(Cursor::new(xml.as_bytes()))
            .expect_err("a control without a defined Unicode projection must fail");
        assert_eq!(error.code, ErrorCode::UnsupportedFeature);
        assert!(error.message.contains(control), "got: {error}");
    }
}

#[test]
fn rejects_direct_text_children_outside_documented_char_wrapper() {
    let xml = document(r#"<P ParaShape="0"><TEXT CharShape="0">직접 텍스트</TEXT></P>"#);
    let error = read_document_from(Cursor::new(xml.as_bytes()))
        .expect_err("undocumented direct TEXT content must not be guessed");
    assert_eq!(error.code, ErrorCode::UnsupportedFeature);
    assert!(error.message.contains("CHAR"), "got: {error}");
}

#[test]
fn rejects_text_and_paragraphs_outside_the_documented_body_path() {
    for xml in [
        "<HWPML Version=\"2.91\"><BODY><SECTION>본문</SECTION></BODY></HWPML>",
        "<HWPML Version=\"2.91\"><BODY><SECTION><P>본문</P></SECTION></BODY></HWPML>",
        "<HWPML Version=\"2.91\"><BODY><P><TEXT><CHAR>본문</CHAR></TEXT></P></BODY></HWPML>",
    ] {
        let error = read_document_from(Cursor::new(xml.as_bytes()))
            .expect_err("only BODY/SECTION/P/TEXT/CHAR may contribute text");
        assert_eq!(error.code, ErrorCode::UnsupportedFeature);
    }
}

#[test]
fn unknown_root_children_fail_instead_of_hiding_document_content() {
    let xml = "<HWPML Version=\"2.91\"><PAYLOAD><P><TEXT><CHAR>숨은 본문</CHAR></TEXT></P></PAYLOAD><BODY><SECTION/></BODY></HWPML>";
    let error = read_document_from(Cursor::new(xml.as_bytes()))
        .expect_err("unknown root content must not be discarded");
    assert_eq!(error.code, ErrorCode::UnsupportedFeature);
    assert!(error.message.contains("PAYLOAD"), "got: {error}");
}

#[test]
fn rejects_duplicate_head_body_and_tail_root_children() {
    for (name, children) in [
        ("HEAD", "<HEAD/><HEAD/><BODY><SECTION/></BODY>"),
        ("BODY", "<BODY><SECTION/></BODY><BODY><SECTION/></BODY>"),
        ("TAIL", "<BODY><SECTION/></BODY><TAIL/><TAIL/>"),
    ] {
        let xml = format!("<HWPML Version=\"2.91\">{children}</HWPML>");
        let error = read_document_from(Cursor::new(xml.as_bytes()))
            .expect_err("root structural elements may occur only once");
        assert_eq!(error.code, ErrorCode::CorruptInput);
        assert!(error.message.contains(name), "got: {error}");
    }
}

#[test]
fn rejects_root_children_whose_order_would_lose_style_information() {
    for xml in [
        "<HWPML Version=\"2.91\"><BODY><SECTION/></BODY><HEAD/></HWPML>",
        "<HWPML Version=\"2.91\"><TAIL/><BODY><SECTION/></BODY></HWPML>",
    ] {
        let error = read_document_from(Cursor::new(xml.as_bytes()))
            .expect_err("HEAD/BODY/TAIL order is semantically significant");
        assert_eq!(error.code, ErrorCode::CorruptInput);
        assert!(error.message.contains("order"), "got: {error}");
    }
}

#[test]
fn preserves_cdata_inside_char_without_collecting_head_or_tail_metadata() {
    let xml = document(
        r#"<P ParaShape="0"><TEXT CharShape="0"><CHAR><![CDATA[<본문 & 그대로>]]></CHAR></TEXT></P>"#,
    );
    let doc = parse(&xml);
    assert_eq!(
        doc.paragraphs().next().expect("paragraph").plain_text(),
        "<본문 & 그대로>"
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

    let truncated = read_document_from(Cursor::new(b"<HWPML Version=\"2.91\"><BODY><SECTION>"))
        .expect_err("truncated XML must fail");
    assert!(truncated.message.contains("xml"), "got: {truncated}");

    for xml in [
        "prefix<HWPML Version=\"2.91\"><BODY><SECTION/></BODY></HWPML>",
        "<HWPML Version=\"2.91\"><BODY><SECTION/></BODY></HWPML>suffix",
    ] {
        let error = read_document_from(Cursor::new(xml.as_bytes()))
            .expect_err("non-whitespace outside the XML root must fail");
        assert_eq!(error.code, ErrorCode::CorruptInput);
    }
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
<P><TEXT><CHAR>UTF-16 한글</CHAR></TEXT></P>
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
fn rejects_xml_declarations_that_conflict_with_the_actual_byte_encoding() {
    let utf8_declares_utf16 = [
        b"\xEF\xBB\xBF".as_slice(),
        document("")
            .replacen("encoding=\"UTF-8\"", "encoding=\"UTF-16\"", 1)
            .as_bytes(),
    ]
    .concat();
    let error = read_document_from(Cursor::new(utf8_declares_utf16))
        .expect_err("UTF-8 bytes must not claim UTF-16");
    assert_eq!(error.code, ErrorCode::CorruptInput);
    assert!(error.message.contains("encoding"), "got: {error}");

    let xml = document("");
    let mut utf16_declares_utf8 = vec![0xFF, 0xFE];
    for unit in xml.encode_utf16() {
        utf16_declares_utf8.extend_from_slice(&unit.to_le_bytes());
    }
    let error = read_document_from(Cursor::new(utf16_declares_utf8))
        .expect_err("UTF-16 bytes must not claim UTF-8");
    assert_eq!(error.code, ErrorCode::CorruptInput);
    assert!(error.message.contains("encoding"), "got: {error}");
}

#[test]
fn rejects_xml_versions_outside_the_supported_xml_1_0_grammar() {
    let xml = document("").replacen("version=\"1.0\"", "version=\"1.1\"", 1);
    let error = read_document_from(Cursor::new(xml.as_bytes()))
        .expect_err("the parser normalizes attributes under XML 1.0 rules");
    assert_eq!(error.code, ErrorCode::UnsupportedFeature);
    assert!(error.message.contains("XML version"), "got: {error}");
}

#[test]
fn rejects_xml_declarations_outside_the_document_start() {
    for xml in [
        " <!-- leading comment --><?xml version=\"1.0\"?><HWPML Version=\"2.91\"><BODY/></HWPML>",
        "<?xml version=\"1.0\"?><?xml version=\"1.0\"?><HWPML Version=\"2.91\"><BODY/></HWPML>",
        "<HWPML Version=\"2.91\"><?xml version=\"1.0\"?><BODY/></HWPML>",
    ] {
        let error = read_document_from(Cursor::new(xml.as_bytes()))
            .expect_err("an XML declaration is only valid as the first document token");
        assert_eq!(error.code, ErrorCode::CorruptInput);
        assert!(error.message.contains("declaration"), "got: {error}");
    }
}

#[test]
fn rejects_malformed_xml_declaration_pseudo_attributes() {
    for declaration in [
        "<?xml version=\"1.0\" version=\"1.0\"?>",
        "<?xml version=\"1.0\" standalone=\"maybe\"?>",
        "<?xml version=\"1.0\" standalone=\"yes\" encoding=\"UTF-8\"?>",
        "<?xml version=\"1.0\" vendor=\"Hancom\"?>",
    ] {
        let xml = format!("{declaration}<HWPML Version=\"2.91\"><BODY/></HWPML>");
        let error = read_document_from(Cursor::new(xml.as_bytes()))
            .expect_err("XML declaration pseudo-attributes must follow the XML 1.0 grammar");
        assert_eq!(error.code, ErrorCode::CorruptInput);
        assert!(error.message.contains("declaration"), "got: {error}");
    }
}

#[test]
fn accepts_well_formed_xml_declaration_variants() {
    for declaration in [
        "<?xml version=\"1.0\"?>",
        "<?xml version=\"1.0\" standalone=\"yes\"?>",
        "<?xml version=\"1.0\" encoding=\"utf8\" standalone=\"no\"?>",
    ] {
        let xml = format!("{declaration}<HWPML Version=\"2.91\"><BODY/></HWPML>");
        read_document_from(Cursor::new(xml.as_bytes()))
            .expect("well-formed XML 1.0 declaration must remain compatible");
    }
}

#[test]
fn accepts_legal_misc_before_root_when_declaration_is_absent() {
    let xml = " \n<!-- producer note --><?hancom preview?><HWPML Version=\"2.91\"><BODY/></HWPML>";
    read_document_from(Cursor::new(xml.as_bytes()))
        .expect("XML whitespace, comments, and processing instructions may precede the root");
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
    let blocks = &doc.sections[0].blocks;
    assert_eq!(blocks.len(), 3, "paragraph/table/paragraph ordering");
    let Block::Paragraph(before) = &blocks[0] else {
        panic!("first block must be a paragraph");
    };
    assert_eq!(before.plain_text().trim(), "표 앞");
    let Block::Table(table) = &blocks[1] else {
        panic!("second block must be a table");
    };
    assert_eq!((table.rows, table.cols), (1, 2));
    assert_eq!(table.col_widths_twip, vec![600, 1000]);
    assert_eq!(table.cell_at(0, 0).expect("A1").plain_text(), "A1");
    assert_eq!(table.cell_at(0, 1).expect("B1").plain_text(), "B1");
    let Block::Paragraph(after) = &blocks[2] else {
        panic!("third block must be a paragraph");
    };
    assert_eq!(after.plain_text().trim(), "표 뒤");
}

#[test]
fn preserves_significant_space_only_runs_around_tables() {
    let xml = document(
        r#"<P><TEXT><CHAR><NBSPACE/></CHAR><TABLE ColCount="1" RowCount="1">
          <ROW><CELL ColAddr="0" ColSpan="1" RowAddr="0" RowSpan="1">
            <PARALIST><P/></PARALIST>
          </CELL></ROW>
        </TABLE><CHAR> </CHAR></TEXT></P>"#,
    );
    let doc = parse(&xml);
    let blocks = &doc.sections[0].blocks;
    assert_eq!(blocks.len(), 3, "space-only runs are document content");
    let Block::Paragraph(before) = &blocks[0] else {
        panic!("first block must be a paragraph");
    };
    assert_eq!(before.plain_text(), "\u{00A0}");
    let Block::Paragraph(after) = &blocks[2] else {
        panic!("third block must be a paragraph");
    };
    assert_eq!(after.plain_text(), " ");
}

#[test]
fn rejects_table_structure_elements_under_the_wrong_parent() {
    for table_body in [
        r#"<CELL ColAddr="0" RowAddr="0"><PARALIST><P/></PARALIST></CELL>"#,
        r#"<SHAPEOBJECT><ROW><CELL ColAddr="0" RowAddr="0"><PARALIST><P/></PARALIST></CELL></ROW></SHAPEOBJECT>"#,
        r#"<SIZE Width="100" Height="100"/>"#,
    ] {
        let xml = document(&format!(
            "<P><TEXT><TABLE ColCount=\"1\" RowCount=\"1\">{table_body}</TABLE></TEXT></P>"
        ));
        let error = read_document_from(Cursor::new(xml.as_bytes()))
            .expect_err("table structure must follow the documented parent chain");
        assert_eq!(error.code, ErrorCode::CorruptInput);
        assert!(error.message.contains("parent"), "got: {error}");
    }
}

#[test]
fn rejects_malformed_or_out_of_grid_table_coordinates() {
    for table in [
        r#"<TABLE ColCount="1" RowCount="not-a-number"/>"#,
        r#"<TABLE ColCount="1" RowCount="1"><ROW><CELL ColAddr="x" RowAddr="0"><PARALIST><P/></PARALIST></CELL></ROW></TABLE>"#,
        r#"<TABLE ColCount="1" RowCount="1"><ROW><CELL ColAddr="0" ColSpan="0" RowAddr="0"><PARALIST><P/></PARALIST></CELL></ROW></TABLE>"#,
        r#"<TABLE ColCount="1" RowCount="1"><ROW><CELL ColAddr="0" RowAddr="1"><PARALIST><P/></PARALIST></CELL></ROW></TABLE>"#,
    ] {
        let xml = document(&format!("<P><TEXT>{table}</TEXT></P>"));
        let error = read_document_from(Cursor::new(xml.as_bytes()))
            .expect_err("malformed table coordinates must not be repaired silently");
        assert_eq!(error.code, ErrorCode::CorruptInput);
    }
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
fn accepts_common_subset_version_allowlist_and_rejects_unknown_ones() {
    for version in ["2.1", "2.8", "2.9", "2.91"] {
        let xml = document("").replacen("Version=\"2.91\"", &format!("Version=\"{version}\""), 1);
        read_document_from(Cursor::new(xml.as_bytes())).expect("allowlisted version parses");
    }

    let xml = document("").replacen("Version=\"2.91\"", "Version=\"3.0\"", 1);
    let error = read_document_from(Cursor::new(xml.as_bytes()))
        .expect_err("an unknown HWPML version must fail explicitly");
    assert_eq!(error.code, ErrorCode::UnsupportedFeature);
    assert!(error.message.contains("3.0"), "got: {error}");
}

#[test]
fn requires_an_exact_unprefixed_root_and_version_attribute() {
    for xml in [
        document("")
            .replacen("<HWPML", "<hwpml", 1)
            .replacen("</HWPML>", "</hwpml>", 1),
        document("")
            .replacen("<HWPML", "<h:HWPML xmlns:h=\"urn:not-hwpml\"", 1)
            .replacen("</HWPML>", "</h:HWPML>", 1),
        document("").replacen("<HWPML", "<HWPML xmlns=\"urn:not-hwpml\"", 1),
        document("").replacen("Version=\"2.91\"", "version=\"2.91\"", 1),
        document("").replacen(" Version=\"2.91\"", "", 1),
    ] {
        let error = read_document_from(Cursor::new(xml.as_bytes()))
            .expect_err("HWPML names are case-sensitive and Version is required");
        assert_eq!(error.code, ErrorCode::CorruptInput);
    }
}

#[test]
fn rejects_default_namespace_confusion_below_the_root() {
    for xml in [
        "<HWPML Version=\"2.91\"><BODY xmlns=\"urn:not-hwpml\"><SECTION/></BODY></HWPML>",
        "<HWPML Version=\"2.91\"><BODY><SECTION xmlns=\"urn:not-hwpml\"/></BODY></HWPML>",
        "<HWPML Version=\"2.91\"><BODY><SECTION><P><TEXT><CHAR xmlns=\"urn:not-hwpml\">본문</CHAR></TEXT></P></SECTION></BODY></HWPML>",
    ] {
        let error = read_document_from(Cursor::new(xml.as_bytes()))
            .expect_err("all legacy HWPML grammar elements must remain in no namespace");
        assert_eq!(error.code, ErrorCode::CorruptInput);
        assert!(error.message.contains("namespace"), "got: {error}");
    }
}

#[test]
fn recognizes_hwpml_but_rejects_document_type_declarations_as_unsupported() {
    let xml = document("").replacen("<HWPML", "<!DOCTYPE HWPML [<!ELEMENT HWPML ANY>]><HWPML", 1);
    let error = read_document_from(Cursor::new(xml.as_bytes()))
        .expect_err("DTD processing is outside the supported security boundary");
    assert_eq!(error.code, ErrorCode::UnsupportedFeature);
    assert!(error.message.contains("document type"), "got: {error}");
}

#[test]
fn distinguishes_legal_from_misplaced_or_duplicate_doctypes() {
    for xml in [
        "<HWPML Version=\"2.91\"><!DOCTYPE X><BODY/></HWPML>",
        "<!DOCTYPE HWPML><!DOCTYPE HWPML><HWPML Version=\"2.91\"><BODY/></HWPML>",
    ] {
        let error = read_document_from(Cursor::new(xml.as_bytes()))
            .expect_err("DOCTYPE is legal only once in the prolog");
        assert_eq!(error.code, ErrorCode::CorruptInput);
        assert!(error.message.contains("DOCTYPE"), "got: {error}");
    }

    let external = "<!DOCTYPE HWPML SYSTEM \"file:///definitely-not-read.dtd\"><HWPML Version=\"2.91\"><BODY/></HWPML>";
    let error = read_document_from(Cursor::new(external.as_bytes()))
        .expect_err("a legal external DTD is recognized but never resolved");
    assert_eq!(error.code, ErrorCode::UnsupportedFeature);
}

#[test]
fn a_bom_conflicting_with_an_unsupported_declared_encoding_is_corrupt() {
    let declaration =
        "<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?><HWPML Version=\"2.91\"><BODY/></HWPML>";
    let utf8 = [b"\xEF\xBB\xBF".as_slice(), declaration.as_bytes()].concat();
    let error = read_document_from(Cursor::new(utf8))
        .expect_err("a UTF-8 BOM is decisive encoding evidence");
    assert_eq!(error.code, ErrorCode::CorruptInput);
    assert!(error.message.contains("encoding"), "got: {error}");

    let mut utf16 = vec![0xFF, 0xFE];
    for unit in declaration.encode_utf16() {
        utf16.extend_from_slice(&unit.to_le_bytes());
    }
    let error = read_document_from(Cursor::new(utf16))
        .expect_err("a UTF-16 BOM is decisive encoding evidence");
    assert_eq!(error.code, ErrorCode::CorruptInput);

    let error = read_document_from(Cursor::new(declaration.as_bytes()))
        .expect_err("without a BOM the unsupported declaration remains a policy error");
    assert_eq!(error.code, ErrorCode::UnsupportedFeature);
}

#[test]
fn rejects_duplicate_style_ids_and_conflicting_vertical_alignment() {
    for duplicate in [
        "<CHARSHAPELIST><CHARSHAPE Id=\"7\"/><CHARSHAPE Id=\"7\"/></CHARSHAPELIST>",
        "<PARASHAPELIST><PARASHAPE Id=\"7\"/><PARASHAPE Id=\"7\"/></PARASHAPELIST>",
    ] {
        let xml = format!(
            "<HWPML Version=\"2.91\"><HEAD><MAPPINGTABLE>{duplicate}</MAPPINGTABLE></HEAD><BODY><SECTION/></BODY></HWPML>"
        );
        let error = read_document_from(Cursor::new(xml.as_bytes()))
            .expect_err("duplicate style IDs must not silently overwrite data");
        assert_eq!(error.code, ErrorCode::CorruptInput);
        assert!(error.message.contains("duplicate"), "got: {error}");
    }

    let xml = "<HWPML Version=\"2.91\"><HEAD><MAPPINGTABLE><CHARSHAPELIST><CHARSHAPE Id=\"7\"><SUPERSCRIPT/><SUBSCRIPT/></CHARSHAPE></CHARSHAPELIST></MAPPINGTABLE></HEAD><BODY><SECTION/></BODY></HWPML>";
    let error = read_document_from(Cursor::new(xml.as_bytes()))
        .expect_err("one style cannot be both superscript and subscript");
    assert_eq!(error.code, ErrorCode::CorruptInput);
    assert!(error.message.contains("SUPERSCRIPT"), "got: {error}");
    assert!(error.message.contains("SUBSCRIPT"), "got: {error}");
}

#[test]
fn resolves_font_ids_within_their_declared_language_group() {
    let xml = r#"<HWPML Version="2.91"><HEAD><MAPPINGTABLE>
      <FACENAMELIST>
        <FONTFACE Lang="Hangul"><FONT Id="0" Name="한글 글꼴"/></FONTFACE>
        <FONTFACE Lang="Latin"><FONT Id="0" Name="Latin Font"/></FONTFACE>
      </FACENAMELIST>
      <CHARSHAPELIST><CHARSHAPE Id="0"><FONTID Latin="0"/></CHARSHAPE></CHARSHAPELIST>
    </MAPPINGTABLE></HEAD><BODY><SECTION><P><TEXT CharShape="0"><CHAR>Latin</CHAR></TEXT></P></SECTION></BODY></HWPML>"#;
    let doc = parse(xml);
    let paragraph = doc.paragraphs().next().expect("paragraph");
    let Inline::Text(run) = &paragraph.inlines[0] else {
        panic!("text run expected");
    };
    assert_eq!(run.style.font.as_deref(), Some("Latin Font"));
}

#[test]
fn rejects_invalid_duplicate_or_misplaced_mapping_ids() {
    for declaration in [
        "<CHARSHAPELIST><CHARSHAPE/></CHARSHAPELIST>",
        "<CHARSHAPELIST><CHARSHAPE Id=\"not-a-number\"/></CHARSHAPELIST>",
        "<PARASHAPELIST><PARASHAPE/></PARASHAPELIST>",
        "<PARASHAPELIST><PARASHAPE Id=\"1000001\"/></PARASHAPELIST>",
        "<CHARSHAPELIST><CHARSHAPE Id=\"1\"/><CHARSHAPE Id=\"01\"/></CHARSHAPELIST>",
        "<FACENAMELIST><FONTFACE Lang=\"Hangul\"><FONT Name=\"Missing ID\"/></FONTFACE></FACENAMELIST>",
        "<FACENAMELIST><FONTFACE Lang=\"Latin\"><FONT Id=\"bad\" Name=\"Bad ID\"/></FONTFACE></FACENAMELIST>",
    ] {
        let xml = format!(
            "<HWPML Version=\"2.91\"><HEAD><MAPPINGTABLE>{declaration}</MAPPINGTABLE></HEAD><BODY/></HWPML>"
        );
        let error = read_document_from(Cursor::new(xml.as_bytes()))
            .expect_err("mapping declarations require bounded numeric IDs");
        assert_eq!(error.code, ErrorCode::CorruptInput);
        assert!(error.message.contains("Id"), "got: {error}");
    }

    let duplicate_font = r#"<HWPML Version="2.91"><HEAD><MAPPINGTABLE><FACENAMELIST>
      <FONTFACE Lang="Hangul"><FONT Id="0" Name="A"/><FONT Id="0" Name="B"/></FONTFACE>
    </FACENAMELIST></MAPPINGTABLE></HEAD><BODY/></HWPML>"#;
    let error = read_document_from(Cursor::new(duplicate_font.as_bytes()))
        .expect_err("font IDs are unique within one language group");
    assert_eq!(error.code, ErrorCode::CorruptInput);
    assert!(error.message.contains("duplicate"), "got: {error}");

    let misplaced = r#"<HWPML Version="2.91"><HEAD><DOCSUMMARY><CHARSHAPE Id="0"/></DOCSUMMARY></HEAD><BODY/></HWPML>"#;
    let error = read_document_from(Cursor::new(misplaced.as_bytes()))
        .expect_err("style declarations must stay inside their documented lists");
    assert_eq!(error.code, ErrorCode::CorruptInput);
    assert!(error.message.contains("parent"), "got: {error}");
}

#[test]
fn rejects_mapping_wrappers_outside_the_documented_parent_chain() {
    for head in [
        r#"<DOCSUMMARY><MAPPINGTABLE><CHARSHAPELIST><CHARSHAPE Id="0"/></CHARSHAPELIST></MAPPINGTABLE></DOCSUMMARY>"#,
        r#"<MAPPINGTABLE><DOCSUMMARY><FACENAMELIST><FONTFACE Lang="Hangul"><FONT Id="0" Name="A"/></FONTFACE></FACENAMELIST></DOCSUMMARY></MAPPINGTABLE>"#,
    ] {
        let xml = format!("<HWPML Version=\"2.91\"><HEAD>{head}</HEAD><BODY/></HWPML>");
        let error = read_document_from(Cursor::new(xml.as_bytes()))
            .expect_err("mapping wrappers must stay on the documented HEAD/MAPPINGTABLE path");
        assert_eq!(error.code, ErrorCode::CorruptInput);
        assert!(error.message.contains("parent"), "got: {error}");
    }
}

#[test]
fn rejects_explicit_dangling_style_references() {
    for body in [
        "<SECTION><P ParaShape=\"7\"><TEXT><CHAR>본문</CHAR></TEXT></P></SECTION>",
        "<SECTION><P><TEXT CharShape=\"7\"><CHAR>본문</CHAR></TEXT></P></SECTION>",
    ] {
        let xml = format!(
            "<HWPML Version=\"2.91\"><HEAD><MAPPINGTABLE><CHARSHAPELIST><CHARSHAPE Id=\"0\"/></CHARSHAPELIST><PARASHAPELIST><PARASHAPE Id=\"0\"/></PARASHAPELIST></MAPPINGTABLE></HEAD><BODY>{body}</BODY></HWPML>"
        );
        let error = read_document_from(Cursor::new(xml.as_bytes()))
            .expect_err("an explicit style reference must resolve");
        assert_eq!(error.code, ErrorCode::CorruptInput);
        assert!(error.message.contains("reference"), "got: {error}");
    }
}

#[test]
fn limits_aggregate_attribute_bytes_even_below_the_attribute_count_cap() {
    let oversized = "x".repeat(256 * 1024 + 1);
    let xml =
        format!("<HWPML Version=\"2.91\" Payload=\"{oversized}\"><BODY><SECTION/></BODY></HWPML>");
    let error = read_document_from(Cursor::new(xml.as_bytes()))
        .expect_err("large attributes must be bounded independently of count");
    assert_eq!(error.code, ErrorCode::CorruptInput);
    assert!(error.message.contains("attribute bytes"), "got: {error}");
}

#[test]
fn rejects_payload_inside_empty_character_controls() {
    for control in [
        "<TAB>숨은 문자열</TAB>",
        "<LINEBREAK><X/></LINEBREAK>",
        "<NBSPACE>&amp;</NBSPACE>",
        "<TITLEMARK>숨은 문자열</TITLEMARK>",
    ] {
        let xml = document(&format!("<P><TEXT><CHAR>앞{control}뒤</CHAR></TEXT></P>"));
        let error = read_document_from(Cursor::new(xml.as_bytes()))
            .expect_err("documented empty controls must reject child payloads");
        assert_eq!(error.code, ErrorCode::CorruptInput);
        assert!(error.message.contains("empty"), "got: {error}");
    }
}

#[test]
fn rejects_known_controls_under_the_wrong_parent() {
    for xml in [
        document("<P><TEXT><TITLEMARK/></TEXT></P>"),
        document("<P><TEXT><CHAR><SECDEF/></CHAR></TEXT></P>"),
        document("<P><TEXT><CHAR><COLDEF/></CHAR></TEXT></P>"),
    ] {
        let error = read_document_from(Cursor::new(xml.as_bytes()))
            .expect_err("known controls must appear under their documented parent");
        assert_eq!(error.code, ErrorCode::CorruptInput);
        assert!(error.message.contains("parent"), "got: {error}");
    }
}

#[test]
fn requires_table_cell_paragraphs_to_follow_paralist() {
    let xml = document(
        "<P><TEXT><TABLE RowCount=\"1\" ColCount=\"1\"><ROW><CELL><P><TEXT><CHAR>직접</CHAR></TEXT></P></CELL></ROW></TABLE></TEXT></P>",
    );
    let error = read_document_from(Cursor::new(xml.as_bytes()))
        .expect_err("CELL paragraphs must be direct PARALIST children");
    assert_eq!(error.code, ErrorCode::CorruptInput);
    assert!(error.message.contains("PARALIST"), "got: {error}");
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
