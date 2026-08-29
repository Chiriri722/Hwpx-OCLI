//! 실제 ZIP+OWPML 파일을 대상으로 한 파서 검증.

mod common;

use std::io::Cursor;

use common::*;
use officecli_hwpx::emit::word::emit_document;
use officecli_hwpx::owpml::model::{
    Align, Block, HeaderFooterPage, Inline, NoteKind, NoteLineType, NoteLineWidth,
    NoteNumberFormat, NoteNumberRestart, NotePosition, VertAlign,
};
use officecli_hwpx::owpml::read_document_from;

fn parse(b: &HwpxBuilder) -> officecli_hwpx::owpml::model::Document {
    read_document_from(Cursor::new(b.build())).expect("document parses")
}

fn document_with_named_styles(styles: &str, active_style: &str) -> HwpxBuilder {
    let mut builder = HwpxBuilder::new();
    let char_pr = builder.char_pr(CharPr::plain());
    let para_pr = builder.para_pr(ParaPr::default());
    let item_count = styles.matches("<hh:style ").count();
    builder.ref_list_xml(format!(
        r#"<hh:styles itemCnt="{item_count}">{styles}</hh:styles>"#
    ));
    builder.section(format!(
        r#"<hp:p paraPrIDRef="{para_pr}" styleIDRef="{active_style}"><hp:run charPrIDRef="{char_pr}"><hp:t>스타일 본문</hp:t></hp:run></hp:p>"#
    ));
    builder
}

#[test]
fn reads_paragraph_text_in_order() {
    let doc = parse(&simple_doc(&["첫 번째 문단", "두 번째 문단", "세 번째"]));
    let texts: Vec<String> = doc.paragraphs().map(|p| p.plain_text()).collect();
    assert_eq!(texts, vec!["첫 번째 문단", "두 번째 문단", "세 번째"]);
}

#[test]
fn preserves_section_boundaries_and_header_footer_stories() {
    let mut builder = HwpxBuilder::new();
    let char_pr = builder.char_pr(CharPr::plain());
    let para_pr = builder.para_pr(ParaPr::default());

    builder.section(format!(
        concat!(
            r#"<hp:p paraPrIDRef="{pp}"><hp:run charPrIDRef="{cp}">"#,
            r#"<hp:secPr><hp:visibility hideFirstHeader="1" hideFirstFooter="0"/>"#,
            r#"</hp:secPr>"#,
            r#"<hp:ctrl><hp:header id="11" applyPageType="BOTH"><hp:subList>"#,
            r#"<hp:p paraPrIDRef="{pp}"><hp:run charPrIDRef="{cp}"><hp:t>머리말 1</hp:t></hp:run></hp:p>"#,
            r#"<hp:p paraPrIDRef="{pp}"><hp:run charPrIDRef="{cp}"><hp:t>머리말 2</hp:t></hp:run></hp:p>"#,
            r#"</hp:subList></hp:header></hp:ctrl>"#,
            r#"<hp:ctrl><hp:footer id="12" applyPageType="BOTH"><hp:subList>"#,
            r#"<hp:p paraPrIDRef="{pp}"><hp:run charPrIDRef="{cp}"><hp:t>공통 꼬리말</hp:t></hp:run></hp:p>"#,
            r#"</hp:subList></hp:footer></hp:ctrl>"#,
            r#"<hp:t>첫 구역 본문</hp:t></hp:run></hp:p>"#,
        ),
        pp = para_pr,
        cp = char_pr,
    ));
    builder.section(para(&char_pr, &para_pr, "둘째 구역 본문"));

    let document = parse(&builder);
    assert_eq!(
        document.sections.len(),
        2,
        "spine sections must remain distinct"
    );

    let first = &document.sections[0];
    assert_eq!(first.blocks.len(), 1);
    let Block::Paragraph(body) = &first.blocks[0] else {
        panic!("first section body must remain a paragraph");
    };
    assert_eq!(body.plain_text(), "첫 구역 본문");
    assert!(first.hide_first_header);
    assert!(!first.hide_first_footer);

    assert_eq!(first.headers.len(), 1);
    assert_eq!(first.headers[0].page, HeaderFooterPage::Both);
    assert_eq!(
        first.headers[0]
            .blocks
            .iter()
            .filter_map(|block| match block {
                Block::Paragraph(paragraph) => Some(paragraph.plain_text()),
                Block::Table(_) => None,
            })
            .collect::<Vec<_>>(),
        vec!["머리말 1", "머리말 2"]
    );
    assert_eq!(first.footers.len(), 1);
    assert_eq!(first.footers[0].page, HeaderFooterPage::Both);

    assert_eq!(
        document.sections[1]
            .blocks
            .iter()
            .filter_map(|block| match block {
                Block::Paragraph(paragraph) => Some(paragraph.plain_text()),
                Block::Table(_) => None,
            })
            .collect::<Vec<_>>(),
        vec!["둘째 구역 본문"]
    );
    assert_eq!(
        document
            .paragraphs()
            .map(|p| p.plain_text())
            .collect::<Vec<_>>(),
        vec!["첫 구역 본문", "둘째 구역 본문"],
        "header/footer paragraphs must never leak into body iteration"
    );
}

#[test]
fn converts_header_footer_page_counters_to_dynamic_docx_fields() {
    let mut builder = HwpxBuilder::new();
    let char_pr = builder.char_pr(CharPr {
        bold: true,
        ..CharPr::plain()
    });
    let para_pr = builder.para_pr(ParaPr::default());
    builder.section(format!(
        concat!(
            r#"<hp:p paraPrIDRef="{pp}"><hp:run charPrIDRef="{cp}"><hp:ctrl>"#,
            r#"<hp:footer id="12" applyPageType="BOTH"><hp:subList>"#,
            r#"<hp:p paraPrIDRef="{pp}"><hp:run charPrIDRef="{cp}">"#,
            r#"<hp:ctrl><hp:autoNum num="1" numType="PAGE"><hp:autoNumFormat type="DIGIT" userChar="" prefixChar="" suffixChar="" supscript="0"/></hp:autoNum></hp:ctrl>"#,
            r#"<hp:t> / </hp:t>"#,
            r#"<hp:ctrl><hp:autoNum num="3" numType="TOTAL_PAGE"><hp:autoNumFormat type="DIGIT" userChar="" prefixChar="" suffixChar="" supscript="0"/></hp:autoNum></hp:ctrl>"#,
            r#"</hp:run></hp:p></hp:subList></hp:footer></hp:ctrl>"#,
            r#"<hp:t>본문</hp:t></hp:run></hp:p>"#,
        ),
        pp = para_pr,
        cp = char_pr,
    ));

    let document = parse(&builder);
    let fields: Vec<_> = emit_document(&document)
        .into_iter()
        .filter(|item| item.r#type == Some("field"))
        .collect();
    assert_eq!(fields.len(), 2, "PAGE and TOTAL_PAGE must not disappear");
    assert_eq!(fields[0].parent.as_deref(), Some("/footer[1]/p[1]"));
    assert_eq!(fields[0].props["fieldType"], "page");
    assert_eq!(fields[1].props["fieldType"], "numpages");
    assert_eq!(fields[0].props["bold"], "true");
    assert_eq!(fields[1].props["bold"], "true");
}

#[test]
fn rejects_unverified_page_counter_kinds_and_number_formats() {
    let cases = [
        r#"<hp:autoNum num="1" numType="TABLE"><hp:autoNumFormat type="DIGIT"/></hp:autoNum>"#,
        r#"<hp:autoNum num="1" numType="FOOTNOTE"><hp:autoNumFormat type="DIGIT"/></hp:autoNum>"#,
        r#"<hp:autoNum num="1" numType="PAGE"><hp:autoNumFormat type="ROMAN_SMALL"/></hp:autoNum>"#,
        r#"<hp:autoNum num="1" numType="PAGE"><hp:autoNumFormat type="DIGIT" suffixChar=")"/></hp:autoNum>"#,
        r#"<hp:autoNum num="1" numType="PAGE"><hp:autoNumFormat type="DIGIT" supscript="1"/></hp:autoNum>"#,
    ];

    for auto_num in cases {
        let mut builder = HwpxBuilder::new();
        let char_pr = builder.char_pr(CharPr::plain());
        let para_pr = builder.para_pr(ParaPr::default());
        builder.section(format!(
            r#"<hp:p paraPrIDRef="{para_pr}"><hp:run charPrIDRef="{char_pr}"><hp:ctrl>{auto_num}</hp:ctrl><hp:t>본문</hp:t></hp:run></hp:p>"#
        ));

        let error = read_document_from(Cursor::new(builder.build()))
            .expect_err("unsupported automatic numbering must fail closed");
        assert_eq!(
            error.code,
            officecli_hwpx::error::ErrorCode::UnsupportedFeature,
            "case {auto_num}"
        );
        assert!(error.message.contains("autoNum"), "got {error:?}");
    }
}

#[test]
fn rejects_malformed_or_duplicate_header_footer_stories() {
    let cases = [
        (
            r#"<hp:header id="1" applyPageType="BOTH"/>"#,
            "required subList",
        ),
        (
            r#"<hp:header id="1" applyPageType="BOTH"><hp:subList/><hp:subList/></hp:header>"#,
            "more than one subList",
        ),
        (
            r#"<hp:footer id="1" applyPageType="BOTH"><hp:p/><hp:subList/></hp:footer>"#,
            "outside subList",
        ),
        (
            r#"<hp:header id="1" applyPageType="LAST"><hp:subList/></hp:header>"#,
            "invalid applyPageType",
        ),
        (
            r#"<hp:header id="1" applyPageType="BOTH"><hp:subList><hp:unexpected><hp:p/></hp:unexpected></hp:subList></hp:header>"#,
            "unexpected direct child",
        ),
    ];

    for (stories, message) in cases {
        let mut builder = HwpxBuilder::new();
        let char_pr = builder.char_pr(CharPr::plain());
        let para_pr = builder.para_pr(ParaPr::default());
        builder.section(format!(
            r#"<hp:p paraPrIDRef="{para_pr}"><hp:run charPrIDRef="{char_pr}"><hp:ctrl>{stories}</hp:ctrl><hp:t>본문</hp:t></hp:run></hp:p>"#
        ));

        let error = read_document_from(Cursor::new(builder.build()))
            .expect_err("malformed header/footer input must fail closed");
        assert_eq!(
            error.code,
            officecli_hwpx::error::ErrorCode::CorruptInput,
            "case {stories}"
        );
        assert!(error.message.contains(message), "got {error:?}");
    }
}

#[test]
fn accepts_paired_odd_even_header_footer_stories() {
    let mut builder = HwpxBuilder::new();
    let char_pr = builder.char_pr(CharPr::plain());
    let para_pr = builder.para_pr(ParaPr::default());
    builder.section(format!(
        concat!(
            r#"<hp:p paraPrIDRef="{pp}"><hp:run charPrIDRef="{cp}"><hp:ctrl>"#,
            r#"<hp:footer id="1" applyPageType="ODD"><hp:subList><hp:p paraPrIDRef="{pp}"><hp:run charPrIDRef="{cp}"><hp:t>홀수</hp:t></hp:run></hp:p></hp:subList></hp:footer>"#,
            r#"<hp:footer id="2" applyPageType="EVEN"><hp:subList><hp:p paraPrIDRef="{pp}"><hp:run charPrIDRef="{cp}"><hp:t>짝수</hp:t></hp:run></hp:p></hp:subList></hp:footer>"#,
            r#"</hp:ctrl><hp:t>본문</hp:t></hp:run></hp:p>"#,
        ),
        pp = para_pr,
        cp = char_pr,
    ));

    let document = parse(&builder);
    assert_eq!(document.sections[0].footers.len(), 2);
    assert_eq!(document.sections[0].footers[0].page, HeaderFooterPage::Odd);
    assert_eq!(document.sections[0].footers[1].page, HeaderFooterPage::Even);
}

#[test]
fn accepts_single_parity_header_footer_story() {
    let mut builder = HwpxBuilder::new();
    let char_pr = builder.char_pr(CharPr::plain());
    let para_pr = builder.para_pr(ParaPr::default());
    builder.section(format!(
        concat!(
            r#"<hp:p paraPrIDRef="{pp}"><hp:run charPrIDRef="{cp}"><hp:ctrl>"#,
            r#"<hp:footer id="1" applyPageType="EVEN"><hp:subList><hp:p paraPrIDRef="{pp}"><hp:run charPrIDRef="{cp}"><hp:t>짝수 전용</hp:t></hp:run></hp:p></hp:subList></hp:footer>"#,
            r#"</hp:ctrl><hp:t>본문</hp:t></hp:run></hp:p>"#,
        ),
        pp = para_pr,
        cp = char_pr,
    ));

    let document = parse(&builder);
    assert_eq!(document.sections[0].footers.len(), 1);
    assert_eq!(document.sections[0].footers[0].page, HeaderFooterPage::Even);

    let items = emit_document(&document);
    let footer_slots: Vec<_> = items
        .iter()
        .filter(|item| item.r#type == Some("footer"))
        .map(|item| item.props["type"].as_str().expect("slot type"))
        .collect();
    assert_eq!(footer_slots, vec!["default", "even"]);
    assert!(items.iter().any(|item| {
        item.path.as_deref() == Some("/footer[2]/p[1]")
            && item.props.get("text").and_then(|value| value.as_str()) == Some("짝수 전용")
    }));
}

#[test]
fn rejects_unverified_header_footer_timelines_and_overlaps() {
    let cases = [
        concat!(
            r#"<hp:header id="1" applyPageType="ODD"><hp:subList/></hp:header>"#,
            r#"<hp:header id="2" applyPageType="ODD"><hp:subList/></hp:header>"#,
        ),
        concat!(
            r#"<hp:header id="1" applyPageType="BOTH"><hp:subList/></hp:header>"#,
            r#"<hp:header id="2" applyPageType="ODD"><hp:subList/></hp:header>"#,
        ),
    ];

    for stories in cases {
        let mut builder = HwpxBuilder::new();
        let char_pr = builder.char_pr(CharPr::plain());
        let para_pr = builder.para_pr(ParaPr::default());
        builder.section(format!(
            r#"<hp:p paraPrIDRef="{para_pr}"><hp:run charPrIDRef="{char_pr}"><hp:ctrl>{stories}</hp:ctrl><hp:t>본문</hp:t></hp:run></hp:p>"#
        ));

        let error = read_document_from(Cursor::new(builder.build()))
            .expect_err("unverified overlap must fail closed");
        assert_eq!(
            error.code,
            officecli_hwpx::error::ErrorCode::UnsupportedFeature,
            "case {stories}"
        );
        assert!(error.message.contains("header"), "got {error:?}");
    }

    let mut builder = HwpxBuilder::new();
    let char_pr = builder.char_pr(CharPr::plain());
    let para_pr = builder.para_pr(ParaPr::default());
    builder.section(format!(
        concat!(
            r#"<hp:p paraPrIDRef="{pp}"><hp:run charPrIDRef="{cp}"><hp:t>앞 본문</hp:t></hp:run></hp:p>"#,
            r#"<hp:p paraPrIDRef="{pp}"><hp:run charPrIDRef="{cp}"><hp:ctrl>"#,
            r#"<hp:header id="3" applyPageType="BOTH"><hp:subList/></hp:header>"#,
            r#"</hp:ctrl><hp:t>뒤 본문</hp:t></hp:run></hp:p>"#,
        ),
        pp = para_pr,
        cp = char_pr,
    ));
    let error = read_document_from(Cursor::new(builder.build()))
        .expect_err("mid-section activation must not be widened to the whole section");
    assert_eq!(
        error.code,
        officecli_hwpx::error::ErrorCode::UnsupportedFeature
    );
    assert!(error.message.contains("after body content"));

    let mut builder = HwpxBuilder::new();
    let char_pr = builder.char_pr(CharPr::plain());
    let para_pr = builder.para_pr(ParaPr::default());
    builder.section(format!(
        concat!(
            r#"<hp:p paraPrIDRef="{pp}"/>"#,
            r#"<hp:p paraPrIDRef="{pp}"><hp:run charPrIDRef="{cp}"><hp:ctrl>"#,
            r#"<hp:footer id="4" applyPageType="BOTH"><hp:subList/></hp:footer>"#,
            r#"</hp:ctrl></hp:run></hp:p>"#,
        ),
        pp = para_pr,
        cp = char_pr,
    ));
    let error = read_document_from(Cursor::new(builder.build()))
        .expect_err("activation after an empty body paragraph is still mid-section");
    assert_eq!(
        error.code,
        officecli_hwpx::error::ErrorCode::UnsupportedFeature
    );
    assert!(error.message.contains("after body content"));

    let mut builder = HwpxBuilder::new();
    let char_pr = builder.char_pr(CharPr::plain());
    let para_pr = builder.para_pr(ParaPr::default());
    builder.section(format!(
        concat!(
            r#"<hp:p paraPrIDRef="{pp}"><hp:run charPrIDRef="{cp}"><hp:tab/><hp:ctrl>"#,
            r#"<hp:header id="5" applyPageType="BOTH"><hp:subList/></hp:header>"#,
            r#"</hp:ctrl></hp:run></hp:p>"#,
        ),
        pp = para_pr,
        cp = char_pr,
    ));
    let error = read_document_from(Cursor::new(builder.build()))
        .expect_err("activation after visible inline content is still mid-section");
    assert_eq!(
        error.code,
        officecli_hwpx::error::ErrorCode::UnsupportedFeature
    );
    assert!(error.message.contains("after body content"));
}

#[test]
fn rejects_notes_inside_header_footer_stories() {
    let mut builder = HwpxBuilder::new();
    let char_pr = builder.char_pr(CharPr::plain());
    let para_pr = builder.para_pr(ParaPr::default());
    builder.section(format!(
        concat!(
            r#"<hp:p paraPrIDRef="{pp}"><hp:run charPrIDRef="{cp}"><hp:ctrl>"#,
            r#"<hp:header id="1" applyPageType="BOTH"><hp:subList>"#,
            r#"<hp:p paraPrIDRef="{pp}"><hp:run charPrIDRef="{cp}"><hp:ctrl>"#,
            r#"<hp:footNote number="1"><hp:subList><hp:p paraPrIDRef="{pp}"><hp:run charPrIDRef="{cp}"><hp:t>주석</hp:t></hp:run></hp:p></hp:subList></hp:footNote>"#,
            r#"</hp:ctrl></hp:run></hp:p></hp:subList></hp:header>"#,
            r#"</hp:ctrl><hp:t>본문</hp:t></hp:run></hp:p>"#,
        ),
        pp = para_pr,
        cp = char_pr,
    ));

    let error = read_document_from(Cursor::new(builder.build()))
        .expect_err("DOCX cannot carry notes from a header/footer story");
    assert_eq!(
        error.code,
        officecli_hwpx::error::ErrorCode::UnsupportedFeature
    );
    assert!(error.message.contains("headers and footers cannot contain"));
}

#[test]
fn rejects_invalid_first_page_visibility_flags() {
    let mut builder = HwpxBuilder::new();
    let char_pr = builder.char_pr(CharPr::plain());
    let para_pr = builder.para_pr(ParaPr::default());
    builder.section(format!(
        r#"<hp:p paraPrIDRef="{para_pr}"><hp:run charPrIDRef="{char_pr}"><hp:secPr><hp:visibility hideFirstHeader="sometimes"/></hp:secPr><hp:t>본문</hp:t></hp:run></hp:p>"#
    ));

    let error = read_document_from(Cursor::new(builder.build()))
        .expect_err("invalid boolean flags must not silently become false");
    assert_eq!(error.code, officecli_hwpx::error::ErrorCode::CorruptInput);
    assert!(error.message.contains("hideFirstHeader"));
}

#[test]
fn preserves_section_footnote_and_endnote_policies() {
    let mut builder = HwpxBuilder::new();
    let char_pr = builder.char_pr(CharPr::plain());
    let para_pr = builder.para_pr(ParaPr::default());
    builder.section(format!(
        concat!(
            r#"<hp:p paraPrIDRef="{pp}"><hp:run charPrIDRef="{cp}"><hp:secPr>"#,
            r#"<hp:footNotePr>"#,
            r#"<hp:autoNumFormat type="ROMAN_SMALL" userChar="" prefixChar="[" suffixChar="]" supscript="1"/>"#,
            r#"<hp:numbering type="ON_PAGE" newNum="3"/>"#,
            r#"<hp:placement place="EACH_COLUMN" beneathText="1"/>"#,
            r#"</hp:footNotePr>"#,
            r#"<hp:endNotePr>"#,
            r#"<hp:autoNumFormat type="LATIN_CAPITAL" userChar="" prefixChar="" suffixChar="." supscript="0"/>"#,
            r#"<hp:numbering type="ON_SECTION" newNum="5"/>"#,
            r#"<hp:placement place="END_OF_SECTION" beneathText="0"/>"#,
            r#"</hp:endNotePr></hp:secPr><hp:t>본문</hp:t></hp:run></hp:p>"#,
        ),
        pp = para_pr,
        cp = char_pr,
    ));

    let document = parse(&builder);
    let section = &document.sections[0];
    let footnote = section
        .footnote_properties
        .as_ref()
        .expect("footnote policy");
    assert_eq!(footnote.number_format, NoteNumberFormat::LowerRoman);
    assert_eq!(footnote.restart, NoteNumberRestart::EachPage);
    assert_eq!(footnote.start, 3);
    assert_eq!(footnote.position, NotePosition::BeneathText);
    assert_eq!(footnote.prefix, "[");
    assert_eq!(footnote.suffix, "]");
    assert!(footnote.superscript);

    let endnote = section.endnote_properties.as_ref().expect("endnote policy");
    assert_eq!(endnote.number_format, NoteNumberFormat::UpperLetter);
    assert_eq!(endnote.restart, NoteNumberRestart::EachSection);
    assert_eq!(endnote.start, 5);
    assert_eq!(endnote.position, NotePosition::SectionEnd);
    assert_eq!(endnote.suffix, ".");
    assert!(!endnote.superscript);
}

#[test]
fn preserves_exact_section_note_line_and_spacing_profiles() {
    let mut builder = HwpxBuilder::new();
    let char_pr = builder.char_pr(CharPr::plain());
    let para_pr = builder.para_pr(ParaPr::default());
    builder.section(format!(
        concat!(
            r#"<hp:p paraPrIDRef="{pp}"><hp:run charPrIDRef="{cp}"><hp:secPr>"#,
            r#"<hp:footNotePr>"#,
            r#"<hp:autoNumFormat type="DIGIT"/><hp:numbering type="CONTINUOUS" newNum="1"/>"#,
            r#"<hp:placement place="EACH_COLUMN" beneathText="0"/>"#,
            r##"<hp:noteLine length="-1" type="SOLID" width="0.12 mm" color="#102030"/>"##,
            r#"<hp:noteSpacing betweenNotes="283" belowLine="567" aboveLine="850"/>"#,
            r#"</hp:footNotePr><hp:endNotePr>"#,
            r#"<hp:autoNumFormat type="ROMAN_CAPITAL"/><hp:numbering type="ON_SECTION" newNum="2"/>"#,
            r#"<hp:placement place="END_OF_DOCUMENT" beneathText="0"/>"#,
            r##"<hp:noteLine length="14692344" type="DOUBLEWAVE" width="5.0 mm" color="#A0B0C0"/>"##,
            r#"<hp:noteSpacing betweenNotes="0" belowLine="1" aboveLine="4294967295"/>"#,
            r#"</hp:endNotePr></hp:secPr><hp:t>본문만 있음</hp:t></hp:run></hp:p>"#,
        ),
        pp = para_pr,
        cp = char_pr,
    ));

    let document = parse(&builder);
    let footnote = document.sections[0]
        .footnote_properties
        .as_ref()
        .expect("footnote policy");
    let footnote_line = footnote.note_line.as_ref().expect("footnote line");
    assert_eq!(footnote_line.length, -1);
    assert_eq!(footnote_line.line_type, NoteLineType::Solid);
    assert_eq!(footnote_line.width, NoteLineWidth::Mm0_12);
    assert_eq!(footnote_line.color, "#102030");
    let footnote_spacing = footnote.note_spacing.as_ref().expect("footnote spacing");
    assert_eq!(footnote_spacing.between_notes, 283);
    assert_eq!(footnote_spacing.below_line, 567);
    assert_eq!(footnote_spacing.above_line, 850);

    let endnote = document.sections[0]
        .endnote_properties
        .as_ref()
        .expect("endnote policy");
    let endnote_line = endnote.note_line.as_ref().expect("endnote line");
    assert_eq!(endnote_line.length, 14_692_344);
    assert_eq!(endnote_line.line_type, NoteLineType::DoubleWave);
    assert_eq!(endnote_line.width, NoteLineWidth::Mm5_0);
    assert_eq!(endnote_line.color, "#A0B0C0");
    let endnote_spacing = endnote.note_spacing.as_ref().expect("endnote spacing");
    assert_eq!(endnote_spacing.between_notes, 0);
    assert_eq!(endnote_spacing.below_line, 1);
    assert_eq!(endnote_spacing.above_line, u32::MAX);
}

#[test]
fn rejects_active_notes_when_section_note_layout_cannot_be_materialized() {
    let mut builder = HwpxBuilder::new();
    let char_pr = builder.char_pr(CharPr::plain());
    let para_pr = builder.para_pr(ParaPr::default());
    builder.section(format!(
        concat!(
            r#"<hp:p paraPrIDRef="{pp}"><hp:run charPrIDRef="{cp}"><hp:secPr><hp:footNotePr>"#,
            r#"<hp:autoNumFormat type="DIGIT"/><hp:numbering type="CONTINUOUS" newNum="1"/>"#,
            r#"<hp:placement place="EACH_COLUMN" beneathText="0"/>"#,
            r##"<hp:noteLine length="-1" type="SOLID" width="0.12 mm" color="#000000"/>"##,
            r#"<hp:noteSpacing betweenNotes="283" belowLine="567" aboveLine="850"/>"#,
            r#"</hp:footNotePr></hp:secPr><hp:t>본문</hp:t><hp:ctrl><hp:footNote number="1"><hp:subList>"#,
            r#"<hp:p paraPrIDRef="{pp}"><hp:run charPrIDRef="{cp}"><hp:t>실제 각주</hp:t></hp:run></hp:p>"#,
            r#"</hp:subList></hp:footNote></hp:ctrl></hp:run></hp:p>"#,
        ),
        pp = para_pr,
        cp = char_pr,
    ));

    let error = read_document_from(Cursor::new(builder.build()))
        .expect_err("active note layout must fail instead of being approximated");
    assert_eq!(
        error.code,
        officecli_hwpx::error::ErrorCode::UnsupportedFeature
    );
    assert!(error.message.contains("footnote"), "{error:?}");
    assert!(
        error.message.contains("noteLine") && error.message.contains("noteSpacing"),
        "{error:?}"
    );
}

#[test]
fn rejects_malformed_or_unknown_section_note_layout_profiles() {
    let cases = [
        (
            r##"<hp:noteLine length="many" type="SOLID" width="0.12 mm" color="#000000"/>"##,
            "length",
        ),
        (
            r##"<hp:noteLine length="0" type="MYSTERY" width="0.12 mm" color="#000000"/>"##,
            "type",
        ),
        (
            r##"<hp:noteLine length="0" type="SOLID" width="0.11 mm" color="#000000"/>"##,
            "width",
        ),
        (
            r##"<hp:noteLine length="0" type="SOLID" width="0.12 mm" color="#GG0000"/>"##,
            "color",
        ),
        (
            r##"<hp:noteLine length="0" type="SOLID" width="0.12 mm" color="#FF112233"/>"##,
            "color",
        ),
        (
            r#"<hp:noteSpacing betweenNotes="-1" belowLine="567" aboveLine="850"/>"#,
            "betweenNotes",
        ),
        (
            r#"<hp:noteSpacing betweenNotes="283" belowLine="567"/>"#,
            "aboveLine",
        ),
        (r#"<hp:futureNoteLayout value="1"/>"#, "futureNoteLayout"),
    ];

    for (layout, message) in cases {
        let mut builder = HwpxBuilder::new();
        let char_pr = builder.char_pr(CharPr::plain());
        let para_pr = builder.para_pr(ParaPr::default());
        builder.section(format!(
            concat!(
                r#"<hp:p paraPrIDRef="{pp}"><hp:run charPrIDRef="{cp}"><hp:secPr><hp:footNotePr>"#,
                r#"<hp:autoNumFormat type="DIGIT"/><hp:numbering type="CONTINUOUS" newNum="1"/>"#,
                r#"<hp:placement place="EACH_COLUMN" beneathText="0"/>{layout}"#,
                r#"</hp:footNotePr></hp:secPr><hp:t>본문만 있음</hp:t></hp:run></hp:p>"#,
            ),
            pp = para_pr,
            cp = char_pr,
            layout = layout,
        ));

        let error = read_document_from(Cursor::new(builder.build()))
            .expect_err("invalid note layout must fail closed");
        assert_eq!(
            error.code,
            officecli_hwpx::error::ErrorCode::CorruptInput,
            "{layout}"
        );
        assert!(error.message.contains(message), "{layout}: {error:?}");
    }
}

#[test]
fn rejects_duplicate_section_note_layout_elements() {
    for duplicated in [
        concat!(
            r##"<hp:noteLine length="0" type="SOLID" width="0.12 mm" color="#000000"/>"##,
            r##"<hp:noteLine length="0" type="DOT" width="0.1 mm" color="#000000"/>"##,
        ),
        concat!(
            r#"<hp:noteSpacing betweenNotes="0" belowLine="0" aboveLine="0"/>"#,
            r#"<hp:noteSpacing betweenNotes="1" belowLine="1" aboveLine="1"/>"#,
        ),
    ] {
        let mut builder = HwpxBuilder::new();
        let char_pr = builder.char_pr(CharPr::plain());
        let para_pr = builder.para_pr(ParaPr::default());
        builder.section(format!(
            concat!(
                r#"<hp:p paraPrIDRef="{pp}"><hp:run charPrIDRef="{cp}"><hp:secPr><hp:footNotePr>"#,
                r#"<hp:autoNumFormat type="DIGIT"/><hp:numbering type="CONTINUOUS" newNum="1"/>"#,
                r#"<hp:placement place="EACH_COLUMN" beneathText="0"/>{duplicated}"#,
                r#"</hp:footNotePr></hp:secPr></hp:run></hp:p>"#,
            ),
            pp = para_pr,
            cp = char_pr,
            duplicated = duplicated,
        ));

        let error = read_document_from(Cursor::new(builder.build()))
            .expect_err("duplicate note layout elements must fail closed");
        assert_eq!(error.code, officecli_hwpx::error::ErrorCode::CorruptInput);
        assert!(error.message.contains("more than one"), "{error:?}");
    }
}

#[test]
fn rejects_case_confused_note_elements_instead_of_proving_zero_notes() {
    let mut builder = HwpxBuilder::new();
    let char_pr = builder.char_pr(CharPr::plain());
    let para_pr = builder.para_pr(ParaPr::default());
    builder.section(format!(
        r#"<hp:p paraPrIDRef="{para_pr}"><hp:run charPrIDRef="{char_pr}"><hp:ctrl><hp:footnote><hp:subList><hp:p paraPrIDRef="{para_pr}"><hp:run charPrIDRef="{char_pr}"><hp:t>유실되면 안 됨</hp:t></hp:run></hp:p></hp:subList></hp:footnote></hp:ctrl></hp:run></hp:p>"#
    ));

    let error = read_document_from(Cursor::new(builder.build()))
        .expect_err("case-confused note tags must not be silently ignored");
    assert_eq!(error.code, officecli_hwpx::error::ErrorCode::CorruptInput);
    assert!(error.message.contains("footnote"), "{error:?}");
}

#[test]
fn rejects_custom_section_note_marks_until_numbering_semantics_are_proven() {
    let mut builder = HwpxBuilder::new();
    let char_pr = builder.char_pr(CharPr::plain());
    let para_pr = builder.para_pr(ParaPr::default());
    builder.section(format!(
        concat!(
            r#"<hp:p paraPrIDRef="{pp}"><hp:run charPrIDRef="{cp}"><hp:secPr><hp:footNotePr>"#,
            r#"<hp:autoNumFormat type="DIGIT" userChar="*" prefixChar="" suffixChar="" supscript="0"/>"#,
            r#"<hp:numbering type="CONTINUOUS" newNum="1"/>"#,
            r#"<hp:placement place="EACH_COLUMN" beneathText="0"/>"#,
            r#"</hp:footNotePr></hp:secPr><hp:t>본문</hp:t></hp:run></hp:p>"#,
        ),
        pp = para_pr,
        cp = char_pr,
    ));

    let error = read_document_from(Cursor::new(builder.build()))
        .expect_err("custom note marks must not silently become automatic numbers");
    assert_eq!(
        error.code,
        officecli_hwpx::error::ErrorCode::UnsupportedFeature
    );
    assert!(error.message.contains("userChar") || error.message.contains("USER_CHAR"));
}

#[test]
fn rejects_unrepresentable_footnote_column_placement() {
    let mut builder = HwpxBuilder::new();
    let char_pr = builder.char_pr(CharPr::plain());
    let para_pr = builder.para_pr(ParaPr::default());
    builder.section(format!(
        concat!(
            r#"<hp:p paraPrIDRef="{pp}"><hp:run charPrIDRef="{cp}"><hp:secPr><hp:footNotePr>"#,
            r#"<hp:autoNumFormat type="DIGIT"/>"#,
            r#"<hp:numbering type="CONTINUOUS" newNum="1"/>"#,
            r#"<hp:placement place="RIGHT_MOST_COLUMN" beneathText="0"/>"#,
            r#"</hp:footNotePr></hp:secPr><hp:t>본문</hp:t></hp:run></hp:p>"#,
        ),
        pp = para_pr,
        cp = char_pr,
    ));

    let error = read_document_from(Cursor::new(builder.build()))
        .expect_err("DOCX cannot preserve right-most-column footnote placement");
    assert_eq!(
        error.code,
        officecli_hwpx::error::ErrorCode::UnsupportedFeature
    );
    assert!(error.message.contains("RIGHT_MOST_COLUMN"));
}

#[test]
fn preserves_korean_and_special_characters() {
    let doc = parse(&simple_doc(&["한글 & <꺾쇠> \"인용\"", "混在 English 123"]));
    let texts: Vec<String> = doc.paragraphs().map(|p| p.plain_text()).collect();
    assert_eq!(texts[0], "한글 & <꺾쇠> \"인용\"");
    assert_eq!(texts[1], "混在 English 123");
}

#[test]
fn applies_char_properties_from_header() {
    let mut b = HwpxBuilder::new();
    let plain = b.char_pr(CharPr::plain());
    let fancy = b.char_pr(CharPr {
        height: Some(1400),
        text_color: Some("#FF0000".into()),
        bold: true,
        italic: true,
        underline: Some("BOTTOM".into()),
        font_hangul: Some("함초롬돋움".into()),
        ..Default::default()
    });
    let pp = b.para_pr(ParaPr::default());
    b.section(para_with_runs(&pp, &[(&plain, "보통 "), (&fancy, "강조")]));

    let doc = parse(&b);
    let p = doc.paragraphs().next().expect("one paragraph");
    let runs: Vec<_> = p
        .inlines
        .iter()
        .filter_map(|i| match i {
            Inline::Text(r) => Some(r),
            _ => None,
        })
        .collect();
    assert_eq!(runs.len(), 2, "two differently-styled runs");

    assert_eq!(runs[0].text, "보통 ");
    assert!(!runs[0].style.bold);
    assert_eq!(runs[0].style.size_pt, Some(10.0));

    assert_eq!(runs[1].text, "강조");
    assert!(runs[1].style.bold);
    assert!(runs[1].style.italic);
    assert!(runs[1].style.underline);
    assert_eq!(runs[1].style.size_pt, Some(14.0));
    assert_eq!(runs[1].style.color.as_deref(), Some("#FF0000"));
    assert_eq!(runs[1].style.font.as_deref(), Some("함초롬돋움"));
}

#[test]
fn merges_adjacent_runs_with_identical_style() {
    let mut b = HwpxBuilder::new();
    let cp = b.char_pr(CharPr::plain());
    let pp = b.para_pr(ParaPr::default());
    // 같은 charPr을 쓰는 런 3개 → 하나로 합쳐져야 한다.
    b.section(para_with_runs(
        &pp,
        &[(&cp, "가"), (&cp, "나"), (&cp, "다")],
    ));

    let doc = parse(&b);
    let p = doc.paragraphs().next().expect("paragraph");
    let count = p
        .inlines
        .iter()
        .filter(|i| matches!(i, Inline::Text(_)))
        .count();
    assert_eq!(count, 1, "identical styles must merge into one run");
    assert_eq!(p.plain_text(), "가나다");
}

#[test]
fn applies_paragraph_properties() {
    let mut b = HwpxBuilder::new();
    let cp = b.char_pr(CharPr::plain());
    let pp = b.para_pr(ParaPr {
        align: Some("CENTER".into()),
        indent_first: Some(1000),
        margin_left: Some(2000),
        space_before: Some(500),
        space_after: Some(600),
        line_spacing_percent: Some(160),
    });
    b.section(para(&cp, &pp, "가운데 정렬"));

    let doc = parse(&b);
    let p = doc.paragraphs().next().expect("paragraph");
    assert_eq!(p.style.align, Some(Align::Center));
    // HWPUNIT / 5 = twip
    assert_eq!(p.style.indent_first_twip, Some(200));
    assert_eq!(p.style.indent_left_twip, Some(400));
    assert_eq!(p.style.space_before_twip, Some(100));
    assert_eq!(p.style.space_after_twip, Some(120));
    assert_eq!(p.style.line_spacing_ratio, Some(1.6));
}

#[test]
fn reads_superscript_and_subscript() {
    let mut b = HwpxBuilder::new();
    let sup = b.char_pr(CharPr {
        superscript: true,
        ..CharPr::plain()
    });
    let sub = b.char_pr(CharPr {
        subscript: true,
        ..CharPr::plain()
    });
    let pp = b.para_pr(ParaPr::default());
    b.section(para_with_runs(&pp, &[(&sup, "위"), (&sub, "아래")]));

    let doc = parse(&b);
    let p = doc.paragraphs().next().expect("paragraph");
    let runs: Vec<_> = p
        .inlines
        .iter()
        .filter_map(|i| match i {
            Inline::Text(r) => Some(r),
            _ => None,
        })
        .collect();
    assert_eq!(runs[0].style.vert_align, Some(VertAlign::Superscript));
    assert_eq!(runs[1].style.vert_align, Some(VertAlign::Subscript));
}

#[test]
fn reads_line_break_as_inline_not_paragraph_split() {
    let mut b = HwpxBuilder::new();
    let cp = b.char_pr(CharPr::plain());
    let pp = b.para_pr(ParaPr::default());
    b.section(para_with_linebreak(&cp, &pp, "첫줄", "둘째줄"));

    let doc = parse(&b);
    assert_eq!(
        doc.paragraphs().count(),
        1,
        "lineBreak must stay inside one paragraph"
    );
    let p = doc.paragraphs().next().expect("paragraph");
    assert!(
        p.inlines.iter().any(|i| matches!(i, Inline::LineBreak)),
        "lineBreak inline must be recorded"
    );
}

#[test]
fn converts_a_real_hancom_equation_script_to_inline_latex() {
    // Structure and script copied from hwpxlib's Apache-2.0 SimpleEquation.hwpx
    // fixture at pinned commit 96ff157eb5973ba1bcf96c00c1b0993d61a718a0.
    let mut builder = HwpxBuilder::new();
    let char_pr = builder.char_pr(CharPr::plain());
    let para_pr = builder.para_pr(ParaPr::default());
    builder.section(format!(
        r##"<hp:p paraPrIDRef="{para_pr}" styleIDRef="0"><hp:run charPrIDRef="{char_pr}">
        <hp:equation id="1137177714" zOrder="0" numberingType="EQUATION"
          textWrap="TOP_AND_BOTTOM" textFlow="BOTH_SIDES" lock="0"
          version="Equation Version 60" baseLine="61" textColor="#000000"
          baseUnit="1100" lineMode="CHAR" font="HYhwpEQ">
          <hp:sz width="3825" widthRelTo="ABSOLUTE" height="3311" heightRelTo="ABSOLUTE" protect="0"/>
          <hp:pos treatAsChar="1" affectLSpacing="0" flowWithText="1" allowOverlap="0"
            holdAnchorAndSO="0" vertRelTo="PARA" horzRelTo="PARA" vertAlign="TOP"
            horzAlign="LEFT" vertOffset="0" horzOffset="0"/>
          <hp:outMargin left="56" right="56" top="0" bottom="0"/>
          <hp:shapeComment>수식입니다.</hp:shapeComment>
          <hp:script>{{"123"}} over {{123 sqrt {{3466}}}} sum _{{34}} ^{{12}}</hp:script>
        </hp:equation><hp:t>뒤</hp:t></hp:run></hp:p>"##
    ));

    let doc = parse(&builder);
    let items = emit_document(&doc);
    let equation = items
        .iter()
        .find(|item| item.r#type == Some("equation"))
        .expect("the equation must not be silently dropped");

    assert_eq!(equation.parent.as_deref(), Some("/body/p[last()]"));
    assert_eq!(equation.props["mode"], "inline");
    assert_eq!(
        equation.props["formula"],
        r#"\frac{\text{123}}{123\sqrt{3466}}\sum_{34}^{12}"#
    );
}

#[test]
fn preserves_multiple_equations_cdata_color_and_surrounding_text_order() {
    let mut builder = HwpxBuilder::new();
    let char_pr = builder.char_pr(CharPr::plain());
    let para_pr = builder.para_pr(ParaPr::default());
    builder.section(format!(
        r##"<hp:p paraPrIDRef="{para_pr}"><hp:run charPrIDRef="{char_pr}">
        <hp:t>앞</hp:t><hp:equation textColor="#ff0000">
          <hp:pos treatAsChar="true"/><hp:script><![CDATA[alpha + beta]]></hp:script>
        </hp:equation><hp:t>중간</hp:t><hp:equation textColor="#000000">
          <hp:pos treatAsChar="1"/><hp:script>gamma SUB 1</hp:script>
        </hp:equation><hp:t>뒤</hp:t></hp:run></hp:p>"##
    ));

    let doc = parse(&builder);
    let paragraph = doc.paragraphs().next().expect("body paragraph");
    assert_eq!(
        paragraph.plain_text(),
        "앞중간뒤",
        "scripts must not leak into text"
    );

    let items = emit_document(&doc);
    let equation_indexes: Vec<_> = items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| (item.r#type == Some("equation")).then_some(index))
        .collect();
    assert_eq!(equation_indexes.len(), 2);
    let first = equation_indexes[0];
    let second = equation_indexes[1];
    assert_eq!(items[first].props["mode"], "inline");
    assert_eq!(items[first].props["formula"], r#"\color{#FF0000}{α+β}"#);
    assert_eq!(items[second].props["formula"], r#"{γ}_{1}"#);
    assert_eq!(items[first - 1].props["text"], "앞");
    assert_eq!(items[first + 1].props["text"], "중간");
    assert_eq!(items[second - 1].props["text"], "중간");
    assert_eq!(items[second + 1].props["text"], "뒤");
}

#[test]
fn preserves_an_inline_equation_inside_a_table_cell() {
    let mut builder = HwpxBuilder::new();
    builder.char_pr(CharPr::plain());
    builder.para_pr(ParaPr::default());
    let table_xml = table(1, 1, &[CellSpec::new(0, 0, "__EQUATION_CELL__")]);
    let equation_paragraph = concat!(
        r#"<hp:p paraPrIDRef="0"><hp:run charPrIDRef="0"><hp:t>앞</hp:t>"#,
        r#"<hp:equation><hp:pos treatAsChar="1"/><hp:script>x over y</hp:script></hp:equation>"#,
        r#"<hp:t>뒤</hp:t></hp:run></hp:p>"#,
    );
    builder.section(table_xml.replace(&para("0", "0", "__EQUATION_CELL__"), equation_paragraph));

    let items = emit_document(&parse(&builder));
    let cell_parent = "/body/tbl[last()]/tr[1]/tc[1]/p[1]";
    let children: Vec<_> = items
        .iter()
        .filter(|item| item.parent.as_deref() == Some(cell_parent))
        .collect();
    assert_eq!(children.len(), 3);
    assert_eq!(children[0].props["text"], "앞");
    assert_eq!(children[1].r#type, Some("equation"));
    assert_eq!(children[1].props["formula"], r#"\frac{x}{y}"#);
    assert_eq!(children[1].props["mode"], "inline");
    assert_eq!(children[2].props["text"], "뒤");
}

#[test]
fn emits_a_standalone_non_character_equation_in_display_mode() {
    let mut builder = HwpxBuilder::new();
    let char_pr = builder.char_pr(CharPr::plain());
    let para_pr = builder.para_pr(ParaPr::default());
    builder.section(format!(
        r#"<hp:p paraPrIDRef="{para_pr}"><hp:run charPrIDRef="{char_pr}">
        <hp:equation><hp:pos treatAsChar="0"/><hp:script>x over y</hp:script></hp:equation>
        </hp:run></hp:p>"#
    ));

    let items = emit_document(&parse(&builder));
    assert_eq!(items.len(), 1, "do not leave a synthetic empty paragraph");
    assert_eq!(items[0].parent.as_deref(), Some("/body"));
    assert_eq!(items[0].r#type, Some("equation"));
    assert_eq!(items[0].props["mode"], "display");
    assert_eq!(items[0].props["formula"], r#"\frac{x}{y}"#);
}

#[test]
fn rejects_malformed_or_lossy_equations_instead_of_dropping_them() {
    let cases = [
        (
            r#"<hp:equation><hp:pos treatAsChar="1"/></hp:equation>"#,
            officecli_hwpx::error::ErrorCode::CorruptInput,
            "required script",
        ),
        (
            r#"<hp:equation><hp:script>x</hp:script></hp:equation>"#,
            officecli_hwpx::error::ErrorCode::CorruptInput,
            "required pos",
        ),
        (
            r#"<hp:equation><hp:pos treatAsChar="1"/><hp:script><hp:t>x</hp:t></hp:script></hp:equation>"#,
            officecli_hwpx::error::ErrorCode::CorruptInput,
            "nested element",
        ),
        (
            r#"<hp:equation><hp:pos treatAsChar="1"/><hp:script>LONGDIV {2}{3}{6}</hp:script></hp:equation>"#,
            officecli_hwpx::error::ErrorCode::UnsupportedFeature,
            "LONGDIV",
        ),
        (
            r#"<hp:equation><hp:pos treatAsChar="1"/><hp:script>x</hp:script><hp:script>y</hp:script></hp:equation>"#,
            officecli_hwpx::error::ErrorCode::CorruptInput,
            "more than one script",
        ),
        (
            r#"<hp:equation><hp:pos treatAsChar="1"/><hp:pos treatAsChar="1"/><hp:script>x</hp:script></hp:equation>"#,
            officecli_hwpx::error::ErrorCode::CorruptInput,
            "more than one pos",
        ),
        (
            r#"<hp:equation><hp:pos treatAsChar="sometimes"/><hp:script>x</hp:script></hp:equation>"#,
            officecli_hwpx::error::ErrorCode::CorruptInput,
            "invalid treatAsChar",
        ),
        (
            r##"<hp:equation textColor="#GG0000"><hp:pos treatAsChar="1"/><hp:script>x</hp:script></hp:equation>"##,
            officecli_hwpx::error::ErrorCode::CorruptInput,
            "invalid textColor",
        ),
    ];

    for (equation, code, message) in cases {
        let mut builder = HwpxBuilder::new();
        let char_pr = builder.char_pr(CharPr::plain());
        let para_pr = builder.para_pr(ParaPr::default());
        builder.section(format!(
            r#"<hp:p paraPrIDRef="{para_pr}"><hp:run charPrIDRef="{char_pr}">{equation}</hp:run></hp:p>"#
        ));
        let error = read_document_from(Cursor::new(builder.build()))
            .expect_err("malformed or lossy equation must fail closed");
        assert_eq!(error.code, code, "{equation}");
        assert!(error.message.contains(message), "{error:?}");
    }
}

#[test]
fn rejects_display_equation_mixed_with_paragraph_text() {
    let mut builder = HwpxBuilder::new();
    let char_pr = builder.char_pr(CharPr::plain());
    let para_pr = builder.para_pr(ParaPr::default());
    builder.section(format!(
        r#"<hp:p paraPrIDRef="{para_pr}"><hp:run charPrIDRef="{char_pr}"><hp:t>앞</hp:t>
        <hp:equation><hp:pos treatAsChar="0"/><hp:script>x</hp:script></hp:equation>
        </hp:run></hp:p>"#
    ));

    let error = read_document_from(Cursor::new(builder.build()))
        .expect_err("ambiguous display ordering must not be guessed");
    assert_eq!(
        error.code,
        officecli_hwpx::error::ErrorCode::UnsupportedFeature
    );
    assert!(
        error.message.contains("display equation mixed"),
        "{error:?}"
    );
}

#[test]
fn reads_footnotes_and_endnotes_as_ordered_inline_notes() {
    // OWPML ParaList schema: hp:ctrl contains hp:footNote/hp:endNote and each
    // NoteType owns exactly one hp:subList (a ParaListType). The leading
    // autoNum control mirrors an HWPX created in Hancom and published at
    // https://github.com/msjang/pypandoc-hwpx/issues/1.
    let mut b = HwpxBuilder::new();
    let cp = b.char_pr(CharPr::plain());
    let pp = b.para_pr(ParaPr::default());
    b.section(format!(
        r#"<hp:p paraPrIDRef="{pp}" styleIDRef="0"><hp:run charPrIDRef="{cp}">
        <hp:t>앞</hp:t><hp:ctrl><hp:footNote number="3" prefixChar="91" suffixChar="93" instId="41"><hp:subList>
          <hp:p paraPrIDRef="{pp}" styleIDRef="0"><hp:run charPrIDRef="{cp}"><hp:ctrl><hp:autoNum num="3" numType="FOOTNOTE"><hp:autoNumFormat type="DIGIT" suffixChar="" supscript="1"/></hp:autoNum></hp:ctrl><hp:t>각주 첫 문단</hp:t></hp:run></hp:p>
          <hp:p paraPrIDRef="{pp}" styleIDRef="0"><hp:run charPrIDRef="{cp}"><hp:t>각주 둘째</hp:t><hp:lineBreak/><hp:t>줄</hp:t></hp:run></hp:p>
        </hp:subList></hp:footNote></hp:ctrl>
        <hp:t>중간</hp:t><hp:ctrl><hp:endNote number="7" suffixChar="46" instId="42"><hp:subList>
          <hp:p paraPrIDRef="{pp}" styleIDRef="0"><hp:run charPrIDRef="{cp}"><hp:ctrl><hp:autoNum num="7" numType="ENDNOTE"><hp:autoNumFormat type="DIGIT" suffixChar="" supscript="1"/></hp:autoNum></hp:ctrl><hp:t>미주 본문</hp:t></hp:run></hp:p>
        </hp:subList></hp:endNote></hp:ctrl><hp:t>뒤</hp:t>
        </hp:run></hp:p>"#
    ));

    let doc = parse(&b);
    assert_eq!(
        doc.paragraphs().count(),
        1,
        "note body paragraphs are not body blocks"
    );
    let body = doc.paragraphs().next().expect("body paragraph");
    assert_eq!(
        body.plain_text(),
        "앞중간뒤",
        "note text must not leak into body text"
    );

    let notes: Vec<_> = body
        .inlines
        .iter()
        .filter_map(|inline| match inline {
            Inline::Note(note) => Some(note),
            _ => None,
        })
        .collect();
    assert_eq!(notes.len(), 2);
    assert_eq!(notes[0].kind, NoteKind::Footnote);
    assert_eq!(notes[0].number, Some(3));
    assert_eq!(notes[0].instance_id.as_deref(), Some("41"));
    assert_eq!(notes[0].reference_prefix.as_deref(), Some("["));
    assert_eq!(notes[0].reference_suffix.as_deref(), Some("]"));
    assert_eq!(
        notes[0]
            .paragraphs()
            .map(|p| p.plain_text())
            .collect::<Vec<_>>(),
        vec!["각주 첫 문단", "각주 둘째\n줄",]
    );
    assert_eq!(notes[1].kind, NoteKind::Endnote);
    assert_eq!(notes[1].number, Some(7));
    assert_eq!(notes[1].reference_prefix, None);
    assert_eq!(notes[1].reference_suffix.as_deref(), Some("."));
    assert_eq!(
        notes[1]
            .paragraphs()
            .next()
            .expect("endnote paragraph")
            .plain_text(),
        "미주 본문"
    );
}

#[test]
fn rejects_a_note_auto_number_that_does_not_match_its_container() {
    let mut builder = HwpxBuilder::new();
    let char_pr = builder.char_pr(CharPr::plain());
    let para_pr = builder.para_pr(ParaPr::default());
    builder.section(format!(
        r#"<hp:p paraPrIDRef="{para_pr}"><hp:run charPrIDRef="{char_pr}"><hp:ctrl><hp:footNote number="1"><hp:subList><hp:p paraPrIDRef="{para_pr}"><hp:run charPrIDRef="{char_pr}"><hp:ctrl><hp:autoNum num="1" numType="ENDNOTE"><hp:autoNumFormat type="DIGIT"/></hp:autoNum></hp:ctrl><hp:t>각주</hp:t></hp:run></hp:p></hp:subList></hp:footNote></hp:ctrl></hp:run></hp:p>"#
    ));

    let error = read_document_from(Cursor::new(builder.build()))
        .expect_err("a mismatched note marker must not disappear");
    assert_eq!(error.code, officecli_hwpx::error::ErrorCode::CorruptInput);
    assert!(error.message.contains("ENDNOTE") && error.message.contains("footnote"));
}

#[test]
fn rejects_unrepresentable_or_invalid_note_instance_marks() {
    let cases = [
        (
            r#"userChar="42""#,
            officecli_hwpx::error::ErrorCode::UnsupportedFeature,
            "userChar",
        ),
        (
            r#"suffixChar="55296""#,
            officecli_hwpx::error::ErrorCode::CorruptInput,
            "surrogate",
        ),
    ];

    for (attributes, code, message) in cases {
        let mut builder = HwpxBuilder::new();
        let char_pr = builder.char_pr(CharPr::plain());
        let para_pr = builder.para_pr(ParaPr::default());
        builder.section(format!(
            r#"<hp:p paraPrIDRef="{para_pr}"><hp:run charPrIDRef="{char_pr}"><hp:ctrl><hp:footNote {attributes}><hp:subList><hp:p paraPrIDRef="{para_pr}"><hp:run charPrIDRef="{char_pr}"><hp:t>각주</hp:t></hp:run></hp:p></hp:subList></hp:footNote></hp:ctrl></hp:run></hp:p>"#
        ));

        let error = read_document_from(Cursor::new(builder.build()))
            .expect_err("unsupported or malformed marks must fail closed");
        assert_eq!(error.code, code, "{attributes}");
        assert!(error.message.contains(message), "{error:?}");
    }
}

#[test]
fn rejects_a_note_without_the_schema_required_sublist() {
    let mut builder = HwpxBuilder::new();
    let char_pr = builder.char_pr(CharPr::plain());
    let para_pr = builder.para_pr(ParaPr::default());
    builder.section(format!(
        r#"<hp:p paraPrIDRef="{para_pr}"><hp:run charPrIDRef="{char_pr}">
        <hp:ctrl><hp:footNote number="1" instId="9"/></hp:ctrl>
        </hp:run></hp:p>"#
    ));

    let error = read_document_from(Cursor::new(builder.build()))
        .expect_err("a NoteType without subList must be rejected");
    assert_eq!(error.code, officecli_hwpx::error::ErrorCode::CorruptInput);
    assert!(error.message.contains("required subList"), "{error:?}");
}

#[test]
fn rejects_note_content_outside_the_schema_required_sublist() {
    let mut builder = HwpxBuilder::new();
    let char_pr = builder.char_pr(CharPr::plain());
    let para_pr = builder.para_pr(ParaPr::default());
    builder.section(format!(
        r#"<hp:p paraPrIDRef="{para_pr}"><hp:run charPrIDRef="{char_pr}">
        <hp:ctrl><hp:footNote number="1" instId="9">
          <hp:p paraPrIDRef="{para_pr}"><hp:run charPrIDRef="{char_pr}"><hp:t>유실되면 안 됨</hp:t></hp:run></hp:p>
          <hp:subList/>
        </hp:footNote></hp:ctrl>
        </hp:run></hp:p>"#
    ));

    let error = read_document_from(Cursor::new(builder.build()))
        .expect_err("NoteType children outside subList must not be dropped");
    assert_eq!(error.code, officecli_hwpx::error::ErrorCode::CorruptInput);
    assert!(error.message.contains("outside subList"), "{error:?}");
}

#[test]
fn rejects_unknown_direct_children_inside_a_note_sublist() {
    let mut builder = HwpxBuilder::new();
    let char_pr = builder.char_pr(CharPr::plain());
    let para_pr = builder.para_pr(ParaPr::default());
    builder.section(format!(
        r#"<hp:p paraPrIDRef="{para_pr}"><hp:run charPrIDRef="{char_pr}"><hp:ctrl><hp:footNote number="1"><hp:subList><hp:unexpected><hp:p paraPrIDRef="{para_pr}"><hp:run charPrIDRef="{char_pr}"><hp:t>유실되면 안 됨</hp:t></hp:run></hp:p></hp:unexpected></hp:subList></hp:footNote></hp:ctrl></hp:run></hp:p>"#
    ));

    let error = read_document_from(Cursor::new(builder.build()))
        .expect_err("unknown note subList children must not be flattened or dropped");
    assert_eq!(error.code, officecli_hwpx::error::ErrorCode::CorruptInput);
    assert!(
        error.message.contains("unexpected direct child"),
        "{error:?}"
    );
}

#[test]
fn rejects_notes_nested_inside_another_note() {
    let mut builder = HwpxBuilder::new();
    let char_pr = builder.char_pr(CharPr::plain());
    let para_pr = builder.para_pr(ParaPr::default());
    builder.section(format!(
        r#"<hp:p paraPrIDRef="{para_pr}"><hp:run charPrIDRef="{char_pr}">
        <hp:ctrl><hp:footNote number="1"><hp:subList>
          <hp:p paraPrIDRef="{para_pr}"><hp:run charPrIDRef="{char_pr}">
            <hp:t>바깥</hp:t><hp:ctrl><hp:endNote number="1"><hp:subList>
              <hp:p paraPrIDRef="{para_pr}"><hp:run charPrIDRef="{char_pr}"><hp:t>안쪽</hp:t></hp:run></hp:p>
            </hp:subList></hp:endNote></hp:ctrl>
          </hp:run></hp:p>
        </hp:subList></hp:footNote></hp:ctrl>
        </hp:run></hp:p>"#
    ));

    let error = read_document_from(Cursor::new(builder.build()))
        .expect_err("nested notes are not a valid Hancom note graph");
    assert_eq!(error.code, officecli_hwpx::error::ErrorCode::CorruptInput);
    assert!(error.message.contains("nested footnote or endnote"));
}

#[test]
fn reads_checkbox_form_controls() {
    // 실측: 양식 문서는 체크박스를 문자가 아니라 `hp:checkBtn`으로 넣는다.
    // 무시하면 체크 안 된 상자가 통째로 사라진다.
    let mut b = HwpxBuilder::new();
    let cp = b.char_pr(CharPr::plain());
    b.para_pr(ParaPr::default());
    let mut body = para_with_checkbox(&cp, "264자 시  ", "CheckBox2", false);
    body.push_str(&para_with_checkbox(&cp, "815자 에세이 ", "CheckBox3", true));
    b.section(body);

    let doc = parse(&b);
    let boxes: Vec<_> = doc
        .paragraphs()
        .flat_map(|p| p.inlines.iter())
        .filter_map(|i| match i {
            Inline::CheckBox(cb) => Some(cb),
            _ => None,
        })
        .collect();

    assert_eq!(boxes.len(), 2, "both checkboxes must be parsed");
    assert_eq!(boxes[0].name.as_deref(), Some("CheckBox2"));
    assert!(!boxes[0].checked, "UNCHECKED must parse as false");
    assert_eq!(boxes[1].name.as_deref(), Some("CheckBox3"));
    assert!(boxes[1].checked, "CHECKED must parse as true");

    // 체크박스 앞 텍스트도 보존돼야 한다.
    let texts: Vec<String> = doc.paragraphs().map(|p| p.plain_text()).collect();
    assert_eq!(texts, vec!["264자 시  ", "815자 에세이 "]);
}

#[test]
fn preserves_checkboxes_inside_nested_tables() {
    // 회귀 테스트. 실측(2026 대구문학관 참가신청서)에서 체크박스 8개 중 4개가
    // 중첩표 깊이 2에 있었다. 중첩표를 평문으로 낮추면 그 4개가 통째로 사라진다.
    let mut b = HwpxBuilder::new();
    let cp = b.char_pr(CharPr::plain());
    b.para_pr(ParaPr::default());

    // 바깥 표의 셀 안에 다시 표를 넣고, 그 안쪽 셀에 체크박스를 둔다.
    let inner = table(
        1,
        2,
        &[
            CellSpec::new(0, 0, "__INNER_A__"),
            CellSpec::new(0, 1, "__INNER_B__"),
        ],
    );
    // 안쪽 셀 텍스트를 체크박스 문단으로 바꿔치기한다.
    let inner = inner.replace(
        &para(&cp, "0", "__INNER_A__"),
        &para_with_checkbox(&cp, "청소년부 ", "CheckBox11", false),
    );
    let inner = inner.replace(
        &para(&cp, "0", "__INNER_B__"),
        &para_with_checkbox(&cp, "대학·일반부 ", "CheckBox12", true),
    );

    let outer = table(1, 1, &[CellSpec::new(0, 0, "__OUTER__")]);
    let outer = outer.replace(&para("0", "0", "__OUTER__"), &inner);
    b.section(outer);

    let doc = parse(&b);

    // 중첩표가 평탄화되지 않고 셀 블록으로 남아 있어야 한다.
    let outer = doc.tables().next().expect("outer table");
    let nested: Vec<_> = outer.cells.iter().flat_map(|c| c.tables()).collect();
    assert_eq!(nested.len(), 1, "nested table must survive as a table");

    // 체크박스를 재귀로 모은다.
    fn collect_boxes<'a>(
        blocks: &'a [officecli_hwpx::owpml::model::Block],
        out: &mut Vec<&'a officecli_hwpx::owpml::model::CheckBox>,
    ) {
        use officecli_hwpx::owpml::model::Block;
        for b in blocks {
            match b {
                Block::Paragraph(p) => {
                    for i in &p.inlines {
                        if let Inline::CheckBox(cb) = i {
                            out.push(cb);
                        }
                    }
                }
                Block::Table(t) => {
                    for c in &t.cells {
                        collect_boxes(&c.blocks, out);
                    }
                }
            }
        }
    }
    let mut boxes = Vec::new();
    for section in &doc.sections {
        collect_boxes(&section.blocks, &mut boxes);
    }

    assert_eq!(boxes.len(), 2, "checkboxes in a nested table must survive");
    let names: Vec<&str> = boxes.iter().filter_map(|b| b.name.as_deref()).collect();
    assert!(names.contains(&"CheckBox11"), "got {names:?}");
    assert!(names.contains(&"CheckBox12"), "got {names:?}");
    // 체크 상태도 보존돼야 한다.
    assert_eq!(boxes.iter().filter(|b| b.checked).count(), 1);
}

#[test]
fn empty_click_here_field_becomes_a_text_field() {
    // 실측: hp:fieldBegin type="CLICK_HERE"는 HWP의 누름틀 = 양식 입력란.
    let mut b = HwpxBuilder::new();
    let cp = b.char_pr(CharPr::plain());
    b.para_pr(ParaPr::default());
    b.section(para_with_click_here(
        &cp,
        "1520616239",
        "기재하지 마세요.",
        "",
    ));

    let doc = parse(&b);
    let p = doc.paragraphs().next().expect("paragraph");
    let field = p
        .inlines
        .iter()
        .find_map(|i| match i {
            Inline::TextField(f) => Some(f),
            _ => None,
        })
        .expect("text field");
    assert_eq!(field.hint.as_deref(), Some("기재하지 마세요."));
}

#[test]
fn filled_click_here_field_stays_as_styled_text() {
    // 내용이 든 누름틀은 그 내용이 문서 내용이다. 폼필드로 바꾸면
    // `view text`에서 사라지고 글자 서식도 잃는다.
    let mut b = HwpxBuilder::new();
    let styled = b.char_pr(CharPr {
        height: Some(1200),
        text_color: Some("#A6A6A6".into()),
        italic: true,
        ..Default::default()
    });
    b.para_pr(ParaPr::default());
    b.section(para_with_click_here(
        &styled,
        "1520616240",
        "이곳에 입력하세요.",
        "실제로 작성된 내용",
    ));

    let doc = parse(&b);
    let p = doc.paragraphs().next().expect("paragraph");
    assert!(
        !p.inlines.iter().any(|i| matches!(i, Inline::TextField(_))),
        "filled field must not become a form field"
    );
    let run = p
        .inlines
        .iter()
        .find_map(|i| match i {
            Inline::Text(r) => Some(r),
            _ => None,
        })
        .expect("text run");
    assert_eq!(run.text, "실제로 작성된 내용");
    // 글자 서식이 보존돼야 한다.
    assert!(run.style.italic);
    assert_eq!(run.style.color.as_deref(), Some("#A6A6A6"));
    assert_eq!(run.style.size_pt, Some(12.0));
}

#[test]
fn reads_table_structure_and_cells() {
    let mut b = HwpxBuilder::new();
    b.char_pr(CharPr::plain());
    b.para_pr(ParaPr::default());
    b.section(table(
        2,
        2,
        &[
            CellSpec::new(0, 0, "항목"),
            CellSpec::new(0, 1, "값"),
            CellSpec::new(1, 0, "매출"),
            CellSpec::new(1, 1, "1,000"),
        ],
    ));

    let doc = parse(&b);
    let t = doc.tables().next().expect("one table");
    assert_eq!((t.rows, t.cols), (2, 2));
    assert_eq!(t.cells.len(), 4);
    assert_eq!(t.cell_at(0, 0).expect("cell").plain_text(), "항목");
    assert_eq!(t.cell_at(1, 1).expect("cell").plain_text(), "1,000");
    // cellSz width 4000 HWPUNIT → 800 twip
    assert_eq!(t.col_widths_twip, vec![800, 800]);
}

#[test]
fn reads_cell_spans_and_fill() {
    let mut b = HwpxBuilder::new();
    b.char_pr(CharPr::plain());
    b.para_pr(ParaPr::default());
    b.section(table(
        2,
        2,
        &[
            CellSpec::new(0, 0, "머리").span(1, 2).fill("#EEEEEE"),
            CellSpec::new(1, 0, "a"),
            CellSpec::new(1, 1, "b"),
        ],
    ));

    let doc = parse(&b);
    let t = doc.tables().next().expect("table");
    let head = t.cell_at(0, 0).expect("head cell");
    assert_eq!(head.col_span, 2);
    assert_eq!(head.row_span, 1);
    assert_eq!(head.fill.as_deref(), Some("#EEEEEE"));
}

#[test]
fn table_inside_paragraph_becomes_sibling_block() {
    // HWPX는 표를 문단 안에 넣지만 docx는 본문 형제다.
    // 파서는 이를 평탄화해서 Block::Table로 내놔야 한다.
    let mut b = HwpxBuilder::new();
    let cp = b.char_pr(CharPr::plain());
    let pp = b.para_pr(ParaPr::default());
    let mut body = para(&cp, &pp, "표 앞 문단");
    body.push_str(&table(1, 1, &[CellSpec::new(0, 0, "셀")]));
    body.push_str(&para(&cp, &pp, "표 뒤 문단"));
    b.section(body);

    let doc = parse(&b);
    let kinds: Vec<&str> = doc.sections[0]
        .blocks
        .iter()
        .map(|b| match b {
            Block::Paragraph(_) => "p",
            Block::Table(_) => "tbl",
        })
        .collect();
    assert_eq!(kinds, vec!["p", "tbl", "p"]);
}

#[test]
fn reads_image_reference_and_resolves_bindata() {
    let mut b = HwpxBuilder::new();
    b.char_pr(CharPr::plain());
    b.para_pr(ParaPr::default());
    b.section(picture("image1", 7200, 3600, Some("설명")));
    b.bindata("image1", "image1.png", tiny_png(), "image/png");

    let doc = parse(&b);
    let p = doc.paragraphs().next().expect("paragraph");
    let img = p
        .inlines
        .iter()
        .find_map(|i| match i {
            Inline::Image(img) => Some(img),
            _ => None,
        })
        .expect("image inline");

    assert_eq!(img.bin_item_id, "image1");
    // 7200 HWPUNIT = 1 inch = 1440 twip
    assert_eq!(img.width_twip, Some(1440));
    assert_eq!(img.height_twip, Some(720));
    assert_eq!(img.alt.as_deref(), Some("설명"));
    assert_eq!(
        img.data.as_deref().map(|d| d.len()),
        Some(tiny_png().len()),
        "BinData bytes must be resolved"
    );
    assert_eq!(img.content_type.as_deref(), Some("image/png"));
}

#[test]
fn image_without_bindata_leaves_data_none() {
    let mut b = HwpxBuilder::new();
    b.char_pr(CharPr::plain());
    b.para_pr(ParaPr::default());
    b.section(picture("ghost", 7200, 7200, None));
    // BinData를 일부러 넣지 않는다.

    let doc = parse(&b);
    let p = doc.paragraphs().next().expect("paragraph");
    let img = p
        .inlines
        .iter()
        .find_map(|i| match i {
            Inline::Image(img) => Some(img),
            _ => None,
        })
        .expect("image inline");
    assert!(img.data.is_none());
}

#[test]
fn repeated_image_references_share_resolved_bytes() {
    let mut b = HwpxBuilder::new();
    b.char_pr(CharPr::plain());
    b.para_pr(ParaPr::default());
    let body = [
        picture("shared", 7200, 7200, Some("첫 번째")),
        picture("shared", 7200, 7200, Some("두 번째")),
    ]
    .concat();
    b.section(body);
    b.bindata("shared", "shared.png", tiny_png(), "image/png");

    let doc = parse(&b);
    let images: Vec<_> = doc
        .paragraphs()
        .flat_map(|p| p.inlines.iter())
        .filter_map(|inline| match inline {
            Inline::Image(image) => Some(image),
            _ => None,
        })
        .collect();
    assert_eq!(images.len(), 2);

    let first = images[0].data.as_deref().expect("first image data");
    let second = images[1].data.as_deref().expect("second image data");
    assert_eq!(
        first.as_ptr(),
        second.as_ptr(),
        "repeated references must share one resolved allocation"
    );
}

#[test]
fn rejects_excessive_image_reference_count() {
    let mut b = HwpxBuilder::new();
    b.char_pr(CharPr::plain());
    b.para_pr(ParaPr::default());
    let body: String = (0..513)
        .map(|_| picture("shared", 7200, 7200, None))
        .collect();
    b.section(body);
    b.bindata("shared", "shared.png", tiny_png(), "image/png");

    let err = read_document_from(Cursor::new(b.build())).expect_err("image count must reject");
    assert_eq!(err.code, officecli_hwpx::error::ErrorCode::CorruptInput);
    assert!(err.message.contains("image"), "{}", err.message);
}

#[test]
fn rejects_excessive_embedded_image_output_bytes() {
    let mut state = 0x9e37_79b9_u32;
    let image_bytes: Vec<u8> = (0..1024 * 1024)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as u8
        })
        .collect();

    let mut b = HwpxBuilder::new();
    b.char_pr(CharPr::plain());
    b.para_pr(ParaPr::default());
    let body: String = (0..65)
        .map(|_| picture("large", 7200, 7200, None))
        .collect();
    b.section(body);
    b.bindata("large", "large.png", image_bytes, "image/png");

    let err = read_document_from(Cursor::new(b.build())).expect_err("image bytes must reject");
    assert_eq!(err.code, officecli_hwpx::error::ErrorCode::CorruptInput);
    assert!(err.message.contains("image"), "{}", err.message);
}

#[test]
fn concatenates_multiple_sections_in_spine_order() {
    let mut b = HwpxBuilder::new();
    let cp = b.char_pr(CharPr::plain());
    let pp = b.para_pr(ParaPr::default());
    b.section(para(&cp, &pp, "섹션0"));
    b.section(para(&cp, &pp, "섹션1"));
    b.section(para(&cp, &pp, "섹션2"));

    let doc = parse(&b);
    let texts: Vec<String> = doc.paragraphs().map(|p| p.plain_text()).collect();
    assert_eq!(texts, vec!["섹션0", "섹션1", "섹션2"]);
}

#[test]
fn falls_back_to_filename_order_without_hpf() {
    let mut b = HwpxBuilder::new();
    let cp = b.char_pr(CharPr::plain());
    let pp = b.para_pr(ParaPr::default());
    b.section(para(&cp, &pp, "A"));
    b.section(para(&cp, &pp, "B"));
    b.without_hpf();

    let doc = parse(&b);
    let texts: Vec<String> = doc.paragraphs().map(|p| p.plain_text()).collect();
    assert_eq!(texts, vec!["A", "B"]);
}

#[test]
fn works_without_header_xml_losing_only_formatting() {
    let mut b = HwpxBuilder::new();
    b.section(para("0", "0", "서식 없는 텍스트"));
    b.without_header();

    let doc = parse(&b);
    let p = doc.paragraphs().next().expect("paragraph");
    assert_eq!(p.plain_text(), "서식 없는 텍스트");
    assert!(p.style.is_plain(), "no header means no formatting");
}

#[test]
fn accepts_missing_mimetype() {
    let mut b = simple_doc(&["텍스트"]);
    b.without_mimetype();
    let doc = parse(&b);
    assert_eq!(doc.paragraphs().count(), 1);
}

#[test]
fn rejects_wrong_mimetype() {
    let mut b = simple_doc(&["텍스트"]);
    b.with_bad_mimetype("application/epub+zip");
    let err = read_document_from(Cursor::new(b.build())).expect_err("must reject");
    assert_eq!(err.code, officecli_hwpx::error::ErrorCode::CorruptInput);
}

#[test]
fn rejects_non_zip_input() {
    let err = read_document_from(Cursor::new(b"this is not a zip file".to_vec()))
        .expect_err("must reject");
    assert_eq!(err.code, officecli_hwpx::error::ErrorCode::CorruptInput);
}

#[test]
fn rejects_zip_without_sections() {
    let mut b = HwpxBuilder::new();
    b.char_pr(CharPr::plain());
    // 섹션을 하나도 넣지 않는다.
    let err = read_document_from(Cursor::new(b.build())).expect_err("must reject");
    assert_eq!(err.code, officecli_hwpx::error::ErrorCode::CorruptInput);
}

#[test]
fn rejects_section_xml_larger_than_resource_budget() {
    let mut b = HwpxBuilder::new();
    b.section("x".repeat(17 * 1024 * 1024));

    let err = read_document_from(Cursor::new(b.build())).expect_err("oversized XML must reject");
    assert_eq!(err.code, officecli_hwpx::error::ErrorCode::CorruptInput);
    assert!(err.message.contains("resource limit"), "{}", err.message);
}

#[test]
fn rejects_excessive_archive_entry_count() {
    let mut b = simple_doc(&["본문"]);
    for i in 0..4093 {
        b.extra_entry(format!("Meta/extra{i}.bin"), Vec::new());
    }

    let err = read_document_from(Cursor::new(b.build())).expect_err("entry budget must reject");
    assert_eq!(err.code, officecli_hwpx::error::ErrorCode::CorruptInput);
    assert!(err.message.contains("entry count"), "{}", err.message);
}

#[test]
fn rejects_table_dimensions_above_grid_budget() {
    let mut b = HwpxBuilder::new();
    b.char_pr(CharPr::plain());
    b.para_pr(ParaPr::default());
    b.section(wrap_para(
        "0",
        r#"<hp:run charPrIDRef="0"><hp:tbl rowCnt="2000" colCnt="1000"></hp:tbl></hp:run>"#,
    ));

    let err = read_document_from(Cursor::new(b.build())).expect_err("table budget must reject");
    assert_eq!(err.code, officecli_hwpx::error::ErrorCode::CorruptInput);
    assert!(err.message.contains("table"), "{}", err.message);
}

#[test]
fn oversized_cell_span_returns_error_instead_of_panicking() {
    let mut b = HwpxBuilder::new();
    b.char_pr(CharPr::plain());
    b.para_pr(ParaPr::default());
    b.section(table(
        2,
        2,
        &[CellSpec::new(1, 1, "overflow").span(usize::MAX, usize::MAX)],
    ));

    let outcome = std::panic::catch_unwind(|| read_document_from(Cursor::new(b.build())));
    assert!(outcome.is_ok(), "malformed span must not panic");
    let err = outcome
        .expect("checked above")
        .expect_err("oversized span must reject");
    assert_eq!(err.code, officecli_hwpx::error::ErrorCode::CorruptInput);
}

#[test]
fn duplicate_spine_references_are_parsed_once() {
    let mut b = simple_doc(&["한 번만"]);
    b.repeat_section_in_spine(0, 2);

    let doc = parse(&b);
    let texts: Vec<_> = doc.paragraphs().map(|p| p.plain_text()).collect();
    assert_eq!(texts, vec!["한 번만"]);
}

#[test]
fn rejects_excessive_spine_reference_count() {
    let mut b = simple_doc(&["본문"]);
    b.repeat_section_in_spine(0, 2048);

    let err = read_document_from(Cursor::new(b.build())).expect_err("spine budget must reject");
    assert_eq!(err.code, officecli_hwpx::error::ErrorCode::CorruptInput);
    assert!(err.message.contains("spine"), "{}", err.message);
}

#[test]
fn preserves_empty_paragraphs_as_blank_lines() {
    let doc = parse(&simple_doc(&["위", "", "아래"]));
    let texts: Vec<String> = doc.paragraphs().map(|p| p.plain_text()).collect();
    assert_eq!(texts, vec!["위", "", "아래"]);
}

#[test]
fn emits_active_named_styles_before_paragraphs_without_losing_direct_formatting() {
    let mut builder = HwpxBuilder::new();
    let base_char = builder.char_pr(CharPr::plain());
    let style_char = builder.char_pr(CharPr {
        height: Some(1400),
        italic: true,
        ..CharPr::plain()
    });
    let direct_char = builder.char_pr(CharPr::bold());
    let base_para = builder.para_pr(ParaPr::default());
    let style_para = builder.para_pr(ParaPr {
        align: Some("CENTER".into()),
        space_after: Some(600),
        ..Default::default()
    });
    let direct_para = builder.para_pr(ParaPr {
        align: Some("RIGHT".into()),
        space_before: Some(400),
        ..Default::default()
    });
    builder.ref_list_xml(format!(
        concat!(
            r#"<hh:styles itemCnt="2">"#,
            r#"<hh:style id="0" type="PARA" name="바탕글" engName="Normal" paraPrIDRef="{base_para}" charPrIDRef="{base_char}" nextStyleIDRef="0" langID="1042" lockForm="0"/>"#,
            r#"<hh:style id="7" type="PARA" name="제목 &amp; 개요" engName="Title &amp; Outline" paraPrIDRef="{style_para}" charPrIDRef="{style_char}" nextStyleIDRef="0" langID="1042" lockForm="1"/>"#,
            r#"</hh:styles>"#,
        ),
        base_para = base_para,
        base_char = base_char,
        style_para = style_para,
        style_char = style_char,
    ));
    builder.section(format!(
        r#"<hp:p paraPrIDRef="{direct_para}" styleIDRef="7"><hp:run charPrIDRef="{direct_char}"><hp:t>직접 서식 유지</hp:t></hp:run></hp:p>"#
    ));

    let items = emit_document(&parse(&builder));
    let styles: Vec<_> = items
        .iter()
        .filter(|item| item.r#type == Some("style"))
        .collect();
    assert_eq!(styles.len(), 2, "active style and its next dependency");
    assert_eq!(styles[0].props["id"], "0");
    assert!(
        !styles[0].props.contains_key("next"),
        "self-next is redundant"
    );

    let title = styles
        .iter()
        .find(|item| item.props.get("id").and_then(|value| value.as_str()) == Some("7"))
        .expect("active title style");
    assert_eq!(title.props["name"], "제목 & 개요");
    assert_eq!(title.props["type"], "paragraph");
    assert_eq!(title.props["next"], "0");
    assert!(
        !title.props.contains_key("locked"),
        "Hancom's lockForm metadata is not DOCX style locking"
    );
    assert_eq!(title.props["customStyle"], "false");
    assert_eq!(title.props["uiPriority"], "7");
    assert_eq!(title.props["align"], "center");
    assert_eq!(title.props["spaceAfter"], "120");
    assert_eq!(title.props["italic"], "true");
    assert_eq!(title.props["size"], "14pt");

    let paragraph_index = items
        .iter()
        .position(|item| {
            item.props.get("text").and_then(|value| value.as_str()) == Some("직접 서식 유지")
        })
        .expect("body paragraph");
    assert!(
        items
            .iter()
            .enumerate()
            .filter(|(_, item)| item.r#type == Some("style"))
            .all(|(index, _)| index < paragraph_index),
        "style definitions must precede every consumer"
    );
    let paragraph = &items[paragraph_index];
    assert_eq!(paragraph.props["style"], "7");
    assert_eq!(paragraph.props["align"], "right");
    assert_eq!(paragraph.props["spaceBefore"], "80");
    assert_eq!(paragraph.props["bold"], "true");
}

#[test]
fn maps_active_outline_style_level_even_when_direct_para_pr_differs() {
    let mut builder = HwpxBuilder::new();
    let char_pr = builder.char_pr(CharPr::plain());
    let direct_para = builder.para_pr(ParaPr::default());
    let outline_para = builder.para_pr_with_heading(ParaPr::centered(), "OUTLINE", "0", 2);
    builder.ref_list_xml(concat!(
        r#"<hh:numberings itemCnt="1"><hh:numbering id="9" start="0">"#,
        r#"<hh:paraHead start="1" level="1" align="LEFT" useInstWidth="1" autoIndent="1" widthAdjust="0" textOffsetType="PERCENT" textOffset="50" numFormat="DIGIT" charPrIDRef="4294967295" checkable="0">^1.</hh:paraHead>"#,
        r#"<hh:paraHead start="1" level="2" align="LEFT" useInstWidth="1" autoIndent="1" widthAdjust="0" textOffsetType="PERCENT" textOffset="50" numFormat="DIGIT" charPrIDRef="4294967295" checkable="0">^1.^2.</hh:paraHead>"#,
        r#"<hh:paraHead start="1" level="3" align="LEFT" useInstWidth="1" autoIndent="1" widthAdjust="0" textOffsetType="PERCENT" textOffset="50" numFormat="DIGIT" charPrIDRef="4294967295" checkable="0">^1.^2.^3.</hh:paraHead>"#,
        r#"</hh:numbering></hh:numberings>"#,
    ));
    builder.ref_list_xml(format!(
        concat!(
            r#"<hh:styles itemCnt="1">"#,
            r#"<hh:style id="2" type="PARA" name="개요 3" engName="Outline 3" paraPrIDRef="{outline_para}" charPrIDRef="{char_pr}" nextStyleIDRef="2" langID="1042" lockForm="0"/>"#,
            r#"</hh:styles>"#,
        ),
        outline_para = outline_para,
        char_pr = char_pr,
    ));
    builder.section(format!(
        r#"<hp:p paraPrIDRef="{direct_para}" styleIDRef="2"><hp:run charPrIDRef="{char_pr}"><hp:secPr outlineShapeIDRef="9"/><hp:t>구조적 제목</hp:t></hp:run></hp:p>"#
    ));

    let items = emit_document(&parse(&builder));
    let style = items
        .iter()
        .find(|item| item.r#type == Some("style"))
        .expect("outline style");
    assert_eq!(style.props["id"], "2");
    assert_eq!(style.props["outlineLvl"], "2");
    assert_eq!(style.props["numId"], "1");
    assert_eq!(style.props["ilvl"], "2");
    assert_eq!(style.props["align"], "center");
    let paragraph = items
        .iter()
        .find(|item| item.props.get("text").and_then(|value| value.as_str()) == Some("구조적 제목"))
        .expect("body paragraph");
    assert_eq!(paragraph.props["style"], "2");
    assert!(!paragraph.props.contains_key("outlineLvl"));
    assert!(
        !paragraph.props.contains_key("numId"),
        "outline numbering is inherited from the named style"
    );
}

#[test]
fn materializes_hancoms_implicit_outline_zero_profile_for_named_styles() {
    let mut builder = HwpxBuilder::new();
    let char_pr = builder.char_pr(CharPr::plain());
    let direct_para = builder.para_pr(ParaPr::default());
    let outline_para = builder.para_pr_with_heading(ParaPr::default(), "OUTLINE", "0", 4);
    builder.ref_list_xml(format!(
        r#"<hh:styles itemCnt="1"><hh:style id="18" type="PARA" name="개요 5" paraPrIDRef="{outline_para}" charPrIDRef="{char_pr}" nextStyleIDRef="18" lockForm="0"/></hh:styles>"#
    ));
    builder.section(format!(
        r#"<hp:p paraPrIDRef="{direct_para}" styleIDRef="18"><hp:run charPrIDRef="{char_pr}"><hp:secPr outlineShapeIDRef="0"/><hp:t>암묵 개요</hp:t></hp:run></hp:p>"#
    ));

    let items = emit_document(&parse(&builder));
    let style = items
        .iter()
        .find(|item| item.r#type == Some("style"))
        .expect("outline style");
    assert_eq!(style.props["numId"], "1");
    assert_eq!(style.props["ilvl"], "4");
    assert_eq!(style.props["outlineLvl"], "4");
    let levels: Vec<_> = items
        .iter()
        .filter(|item| item.r#type == Some("level"))
        .collect();
    assert_eq!(levels.len(), 5);
    assert_eq!(levels[4].props["format"], "decimal");
    assert_eq!(levels[4].props["lvlText"], "%1.%1.%1.%1.%1.");
}

#[test]
fn rejects_outline_style_shared_by_sections_with_different_outline_numberings() {
    let mut builder = HwpxBuilder::new();
    let char_pr = builder.char_pr(CharPr::plain());
    let direct_para = builder.para_pr(ParaPr::default());
    let outline_para = builder.para_pr_with_heading(ParaPr::default(), "OUTLINE", "0", 0);
    builder.ref_list_xml(concat!(
        r#"<hh:numberings itemCnt="2">"#,
        r#"<hh:numbering id="1" start="0"><hh:paraHead start="1" level="1" align="LEFT" useInstWidth="1" autoIndent="1" widthAdjust="0" textOffsetType="PERCENT" textOffset="50" numFormat="DIGIT" charPrIDRef="4294967295" checkable="0">^1.</hh:paraHead></hh:numbering>"#,
        r#"<hh:numbering id="2" start="0"><hh:paraHead start="1" level="1" align="LEFT" useInstWidth="1" autoIndent="1" widthAdjust="0" textOffsetType="PERCENT" textOffset="50" numFormat="ROMAN_CAPITAL" charPrIDRef="4294967295" checkable="0">^1.</hh:paraHead></hh:numbering>"#,
        r#"</hh:numberings>"#,
    ));
    builder.ref_list_xml(format!(
        r#"<hh:styles itemCnt="1"><hh:style id="2" type="PARA" name="개요" paraPrIDRef="{outline_para}" charPrIDRef="{char_pr}" nextStyleIDRef="2" lockForm="0"/></hh:styles>"#
    ));
    builder.section(format!(
        r#"<hp:p paraPrIDRef="{direct_para}" styleIDRef="2"><hp:run charPrIDRef="{char_pr}"><hp:secPr outlineShapeIDRef="1"/><hp:t>첫 구역</hp:t></hp:run></hp:p>"#
    ));
    builder.section(format!(
        r#"<hp:p paraPrIDRef="{direct_para}" styleIDRef="2"><hp:run charPrIDRef="{char_pr}"><hp:secPr outlineShapeIDRef="2"/><hp:t>둘째 구역</hp:t></hp:run></hp:p>"#
    ));

    let error = read_document_from(Cursor::new(builder.build()))
        .expect_err("one DOCX style cannot own two section-specific outline definitions");
    assert_eq!(
        error.code,
        officecli_hwpx::error::ErrorCode::UnsupportedFeature
    );
    assert!(error
        .message
        .contains("multiple section outline numberings"));
}

#[test]
fn materializes_numbering_owned_only_by_an_active_named_style() {
    let mut builder = HwpxBuilder::new();
    let char_pr = builder.char_pr(CharPr::plain());
    let direct_para = builder.para_pr(ParaPr::default());
    let style_para = builder.para_pr_with_heading(ParaPr::default(), "NUMBER", "5", 1);
    builder.ref_list_xml(concat!(
        r#"<hh:numberings itemCnt="1"><hh:numbering id="5" start="0">"#,
        r#"<hh:paraHead start="1" level="1" align="LEFT" useInstWidth="1" autoIndent="1" widthAdjust="0" textOffsetType="PERCENT" textOffset="50" numFormat="DIGIT" charPrIDRef="4294967295" checkable="0">^1.</hh:paraHead>"#,
        r#"<hh:paraHead start="1" level="2" align="LEFT" useInstWidth="1" autoIndent="1" widthAdjust="0" textOffsetType="PERCENT" textOffset="50" numFormat="HANGUL_SYLLABLE" charPrIDRef="4294967295" checkable="0">^1.^2.</hh:paraHead>"#,
        r#"</hh:numbering></hh:numberings>"#,
    ));
    builder.ref_list_xml(format!(
        r#"<hh:styles itemCnt="1"><hh:style id="7" type="PARA" name="번호 스타일" paraPrIDRef="{style_para}" charPrIDRef="{char_pr}" nextStyleIDRef="7" lockForm="0"/></hh:styles>"#
    ));
    builder.section(format!(
        r#"<hp:p paraPrIDRef="{direct_para}" styleIDRef="7"><hp:run charPrIDRef="{char_pr}"><hp:t>스타일 번호</hp:t></hp:run></hp:p>"#
    ));

    let items = emit_document(&parse(&builder));
    let style_index = items
        .iter()
        .position(|item| item.r#type == Some("style"))
        .expect("numbered named style");
    assert!(
        items[..style_index]
            .iter()
            .any(|item| item.r#type == Some("num")),
        "style-owned numbering must be defined first"
    );
    let style = &items[style_index];
    assert_eq!(style.props["numId"], "1");
    assert_eq!(style.props["ilvl"], "1");
    let levels = items
        .iter()
        .filter(|item| item.r#type == Some("level"))
        .count();
    assert_eq!(levels, 2);

    let paragraph = items
        .iter()
        .find(|item| item.props.get("text").and_then(|value| value.as_str()) == Some("스타일 번호"))
        .expect("styled paragraph");
    assert_eq!(paragraph.props["style"], "7");
    assert!(
        !paragraph.props.contains_key("numId"),
        "numbering is inherited from the named style, not invented as direct formatting"
    );
}

#[test]
fn collects_named_styles_used_only_in_header_stories() {
    let mut builder = HwpxBuilder::new();
    let char_pr = builder.char_pr(CharPr::plain());
    let para_pr = builder.para_pr(ParaPr::default());
    builder.ref_list_xml(format!(
        concat!(
            r#"<hh:styles itemCnt="2">"#,
            r#"<hh:style id="0" type="PARA" name="본문" paraPrIDRef="{para_pr}" charPrIDRef="{char_pr}" nextStyleIDRef="0" lockForm="0"/>"#,
            r#"<hh:style id="5" type="PARA" name="머리말" paraPrIDRef="{para_pr}" charPrIDRef="{char_pr}" nextStyleIDRef="0" lockForm="0"/>"#,
            r#"</hh:styles>"#,
        ),
        para_pr = para_pr,
        char_pr = char_pr,
    ));
    builder.section(format!(
        concat!(
            r#"<hp:p paraPrIDRef="{para_pr}" styleIDRef="0"><hp:run charPrIDRef="{char_pr}">"#,
            r#"<hp:ctrl><hp:header id="1" applyPageType="BOTH"><hp:subList>"#,
            r#"<hp:p paraPrIDRef="{para_pr}" styleIDRef="5"><hp:run charPrIDRef="{char_pr}"><hp:t>전용 머리말</hp:t></hp:run></hp:p>"#,
            r#"</hp:subList></hp:header></hp:ctrl><hp:t>본문</hp:t></hp:run></hp:p>"#,
        ),
        para_pr = para_pr,
        char_pr = char_pr,
    ));

    let items = emit_document(&parse(&builder));
    let style_ids: Vec<_> = items
        .iter()
        .filter(|item| item.r#type == Some("style"))
        .map(|item| item.props["id"].as_str().expect("style id"))
        .collect();
    assert_eq!(style_ids, vec!["0", "5"]);
    let header_paragraph = items
        .iter()
        .find(|item| item.props.get("text").and_then(|value| value.as_str()) == Some("전용 머리말"))
        .expect("header paragraph");
    assert_eq!(header_paragraph.props["style"], "5");
}

#[test]
fn ignores_malformed_dormant_named_styles() {
    let styles = concat!(
        r#"<hh:style id="0" type="PARA" name="바탕글" paraPrIDRef="0" charPrIDRef="0" nextStyleIDRef="0" lockForm="0"/>"#,
        // This definition is deliberately unrepresentable, but no paragraph or
        // active dependency reaches it.
        r#"<hh:style id="99" type="TABLE" name="휴면 손상" paraPrIDRef="404" charPrIDRef="405" nextStyleIDRef="missing" lockForm="maybe"/>"#,
    );
    let builder = document_with_named_styles(styles, "0");
    let items = emit_document(&parse(&builder));
    let emitted: Vec<_> = items
        .iter()
        .filter(|item| item.r#type == Some("style"))
        .collect();
    assert_eq!(emitted.len(), 1);
    assert_eq!(emitted[0].props["id"], "0");
}

#[test]
fn rejects_active_named_style_table_count_mismatch() {
    let mut builder = HwpxBuilder::new();
    let char_pr = builder.char_pr(CharPr::plain());
    let para_pr = builder.para_pr(ParaPr::default());
    builder.ref_list_xml(format!(
        r#"<hh:styles itemCnt="2"><hh:style id="0" type="PARA" name="바탕글" paraPrIDRef="{para_pr}" charPrIDRef="{char_pr}" nextStyleIDRef="0" lockForm="0"/></hh:styles>"#
    ));
    builder.section(format!(
        r#"<hp:p paraPrIDRef="{para_pr}" styleIDRef="0"><hp:run charPrIDRef="{char_pr}"><hp:t>본문</hp:t></hp:run></hp:p>"#
    ));

    let error = read_document_from(Cursor::new(builder.build()))
        .expect_err("an active style table must honor itemCnt");
    assert_eq!(error.code, officecli_hwpx::error::ErrorCode::CorruptInput);
    assert!(error.message.contains("itemCnt"));
}

#[test]
fn rejects_missing_or_blank_paragraph_style_references_when_the_table_exists() {
    for active_style in [None, Some(""), Some(" ")] {
        let mut builder = HwpxBuilder::new();
        let char_pr = builder.char_pr(CharPr::plain());
        let para_pr = builder.para_pr(ParaPr::default());
        builder.ref_list_xml(format!(
            r#"<hh:styles itemCnt="1"><hh:style id="0" type="PARA" name="바탕글" paraPrIDRef="{para_pr}" charPrIDRef="{char_pr}" nextStyleIDRef="0" lockForm="0"/></hh:styles>"#
        ));
        let style_attribute = active_style
            .map(|id| format!(r#" styleIDRef="{id}""#))
            .unwrap_or_default();
        builder.section(format!(
            r#"<hp:p paraPrIDRef="{para_pr}"{style_attribute}><hp:run charPrIDRef="{char_pr}"><hp:t>필수 참조</hp:t></hp:run></hp:p>"#
        ));

        let error = read_document_from(Cursor::new(builder.build()))
            .expect_err("a present style table requires a non-empty paragraph style reference");
        assert_eq!(
            error.code,
            officecli_hwpx::error::ErrorCode::CorruptInput,
            "case {active_style:?}: {error:?}"
        );
        assert!(
            error.message.contains("styleIDRef"),
            "case {active_style:?}: {error:?}"
        );
    }
}

#[test]
fn rejects_ambiguous_or_incomplete_active_named_styles() {
    let cases = [
        (
            "missing active definition",
            r#"<hh:style id="1" type="PARA" name="다른 스타일" paraPrIDRef="0" charPrIDRef="0" nextStyleIDRef="1" lockForm="0"/>"#,
            officecli_hwpx::error::ErrorCode::CorruptInput,
            "style id 0 has no definition",
        ),
        (
            "duplicate active id",
            concat!(
                r#"<hh:style id="0" type="PARA" name="첫째" paraPrIDRef="0" charPrIDRef="0" nextStyleIDRef="0" lockForm="0"/>"#,
                r#"<hh:style id="0" type="PARA" name="둘째" paraPrIDRef="0" charPrIDRef="0" nextStyleIDRef="0" lockForm="0"/>"#,
            ),
            officecli_hwpx::error::ErrorCode::CorruptInput,
            "duplicate definitions",
        ),
        (
            "wrong active type",
            r#"<hh:style id="0" type="CHAR" name="문자 스타일" paraPrIDRef="0" charPrIDRef="0" nextStyleIDRef="0" lockForm="0"/>"#,
            officecli_hwpx::error::ErrorCode::UnsupportedFeature,
            "type CHAR",
        ),
        (
            "missing primary name",
            r#"<hh:style id="0" type="PARA" engName="Normal" paraPrIDRef="0" charPrIDRef="0" nextStyleIDRef="0" lockForm="0"/>"#,
            officecli_hwpx::error::ErrorCode::CorruptInput,
            "has no name",
        ),
        (
            "missing paraPr target",
            r#"<hh:style id="0" type="PARA" name="손상" paraPrIDRef="99" charPrIDRef="0" nextStyleIDRef="0" lockForm="0"/>"#,
            officecli_hwpx::error::ErrorCode::CorruptInput,
            "paraPr 99",
        ),
        (
            "missing charPr target",
            r#"<hh:style id="0" type="PARA" name="손상" paraPrIDRef="0" charPrIDRef="99" nextStyleIDRef="0" lockForm="0"/>"#,
            officecli_hwpx::error::ErrorCode::CorruptInput,
            "charPr 99",
        ),
        (
            "missing next dependency",
            r#"<hh:style id="0" type="PARA" name="손상" paraPrIDRef="0" charPrIDRef="0" nextStyleIDRef="9" lockForm="0"/>"#,
            officecli_hwpx::error::ErrorCode::CorruptInput,
            "style id 9 has no definition",
        ),
        (
            "invalid lock flag",
            r#"<hh:style id="0" type="PARA" name="손상" paraPrIDRef="0" charPrIDRef="0" nextStyleIDRef="0" lockForm="sometimes"/>"#,
            officecli_hwpx::error::ErrorCode::CorruptInput,
            "lockForm",
        ),
    ];

    for (label, styles, expected_code, expected_message) in cases {
        let builder = document_with_named_styles(styles, "0");
        let error = read_document_from(Cursor::new(builder.build()))
            .expect_err("active invalid style must fail closed");
        assert_eq!(error.code, expected_code, "case {label}: {error:?}");
        assert!(
            error.message.contains(expected_message),
            "case {label}: {error:?}"
        );
    }
}

#[test]
fn skips_linesegarray_render_cache() {
    // linesegarray는 렌더링 좌표 캐시다. 본문에 섞여 나오면 안 된다.
    let doc = parse(&simple_doc(&["본문"]));
    let p = doc.paragraphs().next().expect("paragraph");
    assert_eq!(p.plain_text(), "본문");
    assert_eq!(p.inlines.len(), 1);
}

#[test]
fn emits_number_bullet_and_section_outline_definitions_before_body() {
    let mut b = HwpxBuilder::new();
    let cp = b.char_pr(CharPr::plain());
    let plain = b.para_pr(ParaPr::default());
    let number0 = b.para_pr_with_heading(ParaPr::default(), "NUMBER", "7", 0);
    let number1 = b.para_pr_with_heading(ParaPr::default(), "NUMBER", "7", 1);
    // NUMBER and BULLET deliberately reuse source id=7. Their typed tables are
    // distinct in OWPML and must not collide in the DOCX numbering id space.
    let bullet = b.para_pr_with_heading(ParaPr::default(), "BULLET", "7", 0);
    let outline = b.para_pr_with_heading(ParaPr::default(), "OUTLINE", "0", 0);
    b.ref_list_xml(concat!(
        r#"<hh:numberings itemCnt="2">"#,
        r#"<hh:numbering id="7" start="1">"#,
        r#"<hh:paraHead start="1" level="1" align="LEFT" useInstWidth="1" autoIndent="1" widthAdjust="0" textOffsetType="PERCENT" textOffset="50" numFormat="DIGIT" charPrIDRef="4294967295" checkable="0">^1.</hh:paraHead>"#,
        r#"<hh:paraHead start="1" level="2" align="LEFT" useInstWidth="1" autoIndent="1" widthAdjust="0" textOffsetType="PERCENT" textOffset="50" numFormat="HANGUL_SYLLABLE" charPrIDRef="4294967295" checkable="0">^1.^2.</hh:paraHead>"#,
        r#"</hh:numbering>"#,
        r#"<hh:numbering id="9" start="0">"#,
        r#"<hh:paraHead start="1" level="1" align="LEFT" useInstWidth="1" autoIndent="1" widthAdjust="0" textOffsetType="PERCENT" textOffset="50" numFormat="DIGIT" charPrIDRef="4294967295" checkable="0">^1.</hh:paraHead>"#,
        r#"</hh:numbering></hh:numberings>"#,
        r#"<hh:bullets itemCnt="1"><hh:bullet id="7" char="-" useImage="0">"#,
        r#"<hh:paraHead level="0" align="LEFT" useInstWidth="0" autoIndent="1" widthAdjust="0" textOffsetType="PERCENT" textOffset="50" numFormat="DIGIT" charPrIDRef="4294967295" checkable="0"/>"#,
        r#"</hh:bullet></hh:bullets>"#,
    ));
    b.section(format!(
        concat!(
            r#"<hp:p paraPrIDRef="{plain}"><hp:run charPrIDRef="{cp}">"#,
            r#"<hp:secPr outlineShapeIDRef="9"/><hp:t>본문</hp:t></hp:run></hp:p>"#,
            r#"<hp:p paraPrIDRef="{number0}"><hp:run charPrIDRef="{cp}"><hp:t>번호 1</hp:t></hp:run></hp:p>"#,
            r#"<hp:p paraPrIDRef="{number1}"><hp:run charPrIDRef="{cp}"><hp:t>번호 1-가</hp:t></hp:run></hp:p>"#,
            r#"<hp:p paraPrIDRef="{bullet}"><hp:run charPrIDRef="{cp}"><hp:t>글머리</hp:t></hp:run></hp:p>"#,
            r#"<hp:p paraPrIDRef="{outline}"><hp:run charPrIDRef="{cp}"><hp:t>개요</hp:t></hp:run></hp:p>"#,
        ),
        plain = plain,
        cp = cp,
        number0 = number0,
        number1 = number1,
        bullet = bullet,
        outline = outline,
    ));

    let items = emit_document(&parse(&b));
    let abstract_nums: Vec<_> = items
        .iter()
        .filter(|item| item.r#type == Some("abstractNum"))
        .collect();
    let nums: Vec<_> = items
        .iter()
        .filter(|item| item.r#type == Some("num"))
        .collect();
    assert_eq!(abstract_nums.len(), 3);
    assert_eq!(nums.len(), 3);
    let first_body = items
        .iter()
        .position(|item| item.r#type == Some("paragraph"))
        .expect("body paragraph");
    assert!(
        items.iter().take(first_body).all(|item| matches!(
            item.r#type,
            Some("abstractNum") | Some("level") | Some("num")
        )),
        "all numbering resources must precede body items: {items:#?}"
    );
    let level = |id: usize, ilvl: usize| {
        let parent = format!("/numbering/abstractNum[@id={id}]");
        let ilvl = ilvl.to_string();
        items
            .iter()
            .find(|item| {
                item.r#type == Some("level")
                    && item.parent.as_deref() == Some(parent.as_str())
                    && item.props.get("ilvl").and_then(|value| value.as_str())
                        == Some(ilvl.as_str())
            })
            .expect("numbering level")
    };

    assert_eq!(abstract_nums[0].props["id"], "1");
    assert_eq!(abstract_nums[0].props["type"], "multilevel");
    assert_eq!(level(1, 0).props["format"], "decimal");
    assert_eq!(level(1, 0).props["lvlText"], "%1.");
    assert_eq!(level(1, 1).props["format"], "ganada");
    assert_eq!(level(1, 1).props["lvlText"], "%1.%2.");
    assert_eq!(level(1, 0).props["indent"], "0");
    assert_eq!(level(1, 0).props["hanging"], "0");
    assert_eq!(level(1, 0).props["suff"], "space");

    assert_eq!(abstract_nums[1].props["id"], "2");
    assert_eq!(abstract_nums[2].props["id"], "3");
    assert_eq!(abstract_nums[2].props["type"], "hybridMultilevel");
    assert_eq!(level(3, 0).props["format"], "bullet");
    assert_eq!(level(3, 0).props["lvlText"], "-");
    for (index, num) in nums.iter().enumerate() {
        let id = (index + 1).to_string();
        assert_eq!(num.props["id"], id);
        assert_eq!(num.props["abstractNumId"], id);
        assert_eq!(num.props["continue"], "true");
    }

    let paragraph = |text: &str| {
        items
            .iter()
            .find(|item| item.props.get("text").and_then(|v| v.as_str()) == Some(text))
            .unwrap_or_else(|| panic!("missing paragraph {text:?}"))
    };
    assert_eq!(paragraph("번호 1").props["numId"], "1");
    assert_eq!(paragraph("번호 1").props["numLevel"], "0");
    assert_eq!(paragraph("번호 1-가").props["numId"], "1");
    assert_eq!(paragraph("번호 1-가").props["numLevel"], "1");
    assert_eq!(paragraph("글머리").props["numId"], "3");
    assert_eq!(paragraph("개요").props["numId"], "2");
    assert_eq!(paragraph("개요").props["outlineLvl"], "0");
}

#[test]
fn expands_official_numbering_path_tokens_without_repeating_level_one() {
    let mut b = HwpxBuilder::new();
    let cp = b.char_pr(CharPr::plain());
    let numbered = b.para_pr_with_heading(ParaPr::default(), "NUMBER", "5", 2);
    b.ref_list_xml(concat!(
        r#"<hh:numberings itemCnt="1"><hh:numbering id="5" start="0">"#,
        r#"<hh:paraHead start="1" level="1" align="LEFT" useInstWidth="1" autoIndent="1" widthAdjust="0" textOffsetType="PERCENT" textOffset="50" numFormat="DIGIT" charPrIDRef="4294967295" checkable="0">^n</hh:paraHead>"#,
        r#"<hh:paraHead start="1" level="2" align="LEFT" useInstWidth="1" autoIndent="1" widthAdjust="0" textOffsetType="PERCENT" textOffset="50" numFormat="DIGIT" charPrIDRef="4294967295" checkable="0">^N</hh:paraHead>"#,
        r#"<hh:paraHead start="1" level="3" align="LEFT" useInstWidth="1" autoIndent="1" widthAdjust="0" textOffsetType="PERCENT" textOffset="50" numFormat="DIGIT" charPrIDRef="4294967295" checkable="0">(^1-^3)</hh:paraHead>"#,
        r#"</hh:numbering></hh:numberings>"#,
    ));
    b.section(para(&cp, &numbered, "셋째 단계"));

    let items = emit_document(&parse(&b));
    let levels: Vec<_> = items
        .iter()
        .filter(|item| item.r#type == Some("level"))
        .collect();
    assert_eq!(levels[0].props["lvlText"], "%1");
    assert_eq!(levels[1].props["lvlText"], "%1.%2.");
    assert_eq!(levels[2].props["lvlText"], "(%1-%3)");
}

#[test]
fn emits_every_verified_hwp_number_format() {
    let cases = [
        ("DIGIT", "decimal"),
        ("CIRCLED_DIGIT", "decimalEnclosedCircle"),
        ("ROMAN_CAPITAL", "upperRoman"),
        ("ROMAN_SMALL", "lowerRoman"),
        ("LATIN_CAPITAL", "upperLetter"),
        ("LATIN_SMALL", "lowerLetter"),
        ("HANGUL_SYLLABLE", "ganada"),
        ("HANGUL_JAMO", "chosung"),
    ];

    for (source_format, target_format) in cases {
        let mut b = HwpxBuilder::new();
        let cp = b.char_pr(CharPr::plain());
        let numbered = b.para_pr_with_heading(ParaPr::default(), "NUMBER", "1", 0);
        b.ref_list_xml(format!(
            concat!(
                r#"<hh:numberings itemCnt="1"><hh:numbering id="1" start="0">"#,
                r#"<hh:paraHead start="1" level="1" align="LEFT" useInstWidth="1" autoIndent="1" widthAdjust="0" textOffsetType="PERCENT" textOffset="50" numFormat="{source_format}" charPrIDRef="4294967295" checkable="0">^1.</hh:paraHead>"#,
                r#"</hh:numbering></hh:numberings>"#,
            ),
            source_format = source_format,
        ));
        b.section(para(&cp, &numbered, source_format));

        let level = emit_document(&parse(&b))
            .into_iter()
            .find(|item| item.r#type == Some("level"))
            .expect("numbering level");
        assert_eq!(level.props["format"], target_format, "{source_format}");
    }
}

#[test]
fn rejects_unsafe_numbering_marker_templates() {
    let cases = [
        (
            "^2.",
            officecli_hwpx::error::ErrorCode::CorruptInput,
            "unavailable level",
        ),
        (
            "%1.",
            officecli_hwpx::error::ErrorCode::UnsupportedFeature,
            "literal %",
        ),
        (
            "^x",
            officecli_hwpx::error::ErrorCode::UnsupportedFeature,
            "unsupported ^x",
        ),
        (
            "^",
            officecli_hwpx::error::ErrorCode::CorruptInput,
            "incomplete ^ token",
        ),
    ];

    for (template, expected_code, expected_message) in cases {
        let mut b = HwpxBuilder::new();
        let cp = b.char_pr(CharPr::plain());
        let numbered = b.para_pr_with_heading(ParaPr::default(), "NUMBER", "1", 0);
        b.ref_list_xml(format!(
            concat!(
                r#"<hh:numberings itemCnt="1"><hh:numbering id="1" start="0">"#,
                r#"<hh:paraHead start="1" level="1" align="LEFT" useInstWidth="1" autoIndent="1" widthAdjust="0" textOffsetType="PERCENT" textOffset="50" numFormat="DIGIT" charPrIDRef="4294967295" checkable="0">{template}</hh:paraHead>"#,
                r#"</hh:numbering></hh:numberings>"#,
            ),
            template = template,
        ));
        b.section(para(&cp, &numbered, "안전하지 않은 번호 표식"));

        let error = read_document_from(Cursor::new(b.build()))
            .expect_err("unsafe numbering marker must fail closed");
        assert_eq!(error.code, expected_code, "template={template:?}");
        assert!(
            error.message.contains(expected_message),
            "template={template:?}: {error:?}"
        );
    }
}

#[test]
fn shares_one_numbering_instance_across_number_outline_and_plain_gaps() {
    let mut b = HwpxBuilder::new();
    let cp = b.char_pr(CharPr::plain());
    let plain = b.para_pr(ParaPr::default());
    let numbered = b.para_pr_with_heading(ParaPr::default(), "NUMBER", "5", 0);
    let outline = b.para_pr_with_heading(ParaPr::default(), "OUTLINE", "0", 0);
    b.ref_list_xml(concat!(
        r#"<hh:numberings itemCnt="1"><hh:numbering id="5" start="0">"#,
        r#"<hh:paraHead start="1" level="1" align="LEFT" useInstWidth="1" autoIndent="1" widthAdjust="0" textOffsetType="PERCENT" textOffset="50" numFormat="DIGIT" charPrIDRef="4294967295" checkable="0">^1.</hh:paraHead>"#,
        r#"</hh:numbering></hh:numberings>"#,
    ));
    b.section(format!(
        concat!(
            r#"<hp:p paraPrIDRef="{plain}"><hp:run charPrIDRef="{cp}"><hp:secPr outlineShapeIDRef="5"/><hp:t>일반 앞</hp:t></hp:run></hp:p>"#,
            r#"<hp:p paraPrIDRef="{numbered}"><hp:run charPrIDRef="{cp}"><hp:t>직접 번호</hp:t></hp:run></hp:p>"#,
            r#"<hp:p paraPrIDRef="{plain}"><hp:run charPrIDRef="{cp}"><hp:t>일반 사이</hp:t></hp:run></hp:p>"#,
            r#"<hp:p paraPrIDRef="{outline}"><hp:run charPrIDRef="{cp}"><hp:t>개요 번호</hp:t></hp:run></hp:p>"#,
            r#"<hp:p paraPrIDRef="{numbered}"><hp:run charPrIDRef="{cp}"><hp:t>직접 번호 계속</hp:t></hp:run></hp:p>"#,
        ),
        plain = plain,
        numbered = numbered,
        outline = outline,
        cp = cp,
    ));

    let items = emit_document(&parse(&b));
    assert_eq!(
        items
            .iter()
            .filter(|item| item.r#type == Some("abstractNum"))
            .count(),
        1
    );
    assert_eq!(
        items
            .iter()
            .filter(|item| item.r#type == Some("num"))
            .count(),
        1
    );
    for text in ["직접 번호", "개요 번호", "직접 번호 계속"] {
        let paragraph = items
            .iter()
            .find(|item| item.props.get("text").and_then(|v| v.as_str()) == Some(text))
            .expect("numbered paragraph");
        assert_eq!(paragraph.props["numId"], "1", "{text}");
    }
    let plain_between = items
        .iter()
        .find(|item| item.props.get("text").and_then(|v| v.as_str()) == Some("일반 사이"))
        .expect("plain paragraph");
    assert!(!plain_between.props.contains_key("numId"));
}

#[test]
fn duplicate_number_definition_does_not_poison_same_id_bullet_namespace() {
    let mut b = HwpxBuilder::new();
    let cp = b.char_pr(CharPr::plain());
    let bullet = b.para_pr_with_heading(ParaPr::default(), "BULLET", "7", 0);
    b.ref_list_xml(concat!(
        r#"<hh:numberings itemCnt="2">"#,
        r#"<hh:numbering id="7" start="0"><hh:paraHead start="1" level="1" align="LEFT" useInstWidth="1" autoIndent="1" widthAdjust="0" textOffsetType="PERCENT" textOffset="50" numFormat="DIGIT" charPrIDRef="4294967295" checkable="0">^1.</hh:paraHead></hh:numbering>"#,
        r#"<hh:numbering id="7" start="0"><hh:paraHead start="1" level="1" align="LEFT" useInstWidth="1" autoIndent="1" widthAdjust="0" textOffsetType="PERCENT" textOffset="50" numFormat="DIGIT" charPrIDRef="4294967295" checkable="0">^1)</hh:paraHead></hh:numbering>"#,
        r#"</hh:numberings>"#,
        r#"<hh:bullets itemCnt="1"><hh:bullet id="7" char="-" useImage="0"><hh:paraHead level="0" align="LEFT" useInstWidth="0" autoIndent="1" widthAdjust="0" textOffsetType="PERCENT" textOffset="50" numFormat="DIGIT" charPrIDRef="4294967295" checkable="0"/></hh:bullet></hh:bullets>"#,
    ));
    b.section(para(&cp, &bullet, "분리된 글머리표 ID 공간"));

    let items = emit_document(&parse(&b));
    let definition = items
        .iter()
        .find(|item| item.r#type == Some("abstractNum"))
        .expect("active bullet definition");
    assert_eq!(definition.props["id"], "2");
    let paragraph = items
        .iter()
        .find(|item| item.r#type == Some("paragraph"))
        .expect("bullet paragraph");
    assert_eq!(paragraph.props["numId"], "2");
}

#[test]
fn rejects_an_ambiguous_active_number_definition() {
    let mut b = HwpxBuilder::new();
    let cp = b.char_pr(CharPr::plain());
    let numbered = b.para_pr_with_heading(ParaPr::default(), "NUMBER", "7", 0);
    b.ref_list_xml(concat!(
        r#"<hh:numberings itemCnt="2">"#,
        r#"<hh:numbering id="7" start="0"><hh:paraHead start="1" level="1" align="LEFT" useInstWidth="1" autoIndent="1" widthAdjust="0" textOffsetType="PERCENT" textOffset="50" numFormat="DIGIT" charPrIDRef="4294967295" checkable="0">^1.</hh:paraHead></hh:numbering>"#,
        r#"<hh:numbering id="7" start="0"><hh:paraHead start="1" level="1" align="LEFT" useInstWidth="1" autoIndent="1" widthAdjust="0" textOffsetType="PERCENT" textOffset="50" numFormat="DIGIT" charPrIDRef="4294967295" checkable="0">^1)</hh:paraHead></hh:numbering>"#,
        r#"</hh:numberings>"#,
    ));
    b.section(para(&cp, &numbered, "모호한 번호 정의"));

    let error = read_document_from(Cursor::new(b.build()))
        .expect_err("an active duplicate id must be rejected");
    assert_eq!(error.code, officecli_hwpx::error::ErrorCode::CorruptInput);
    assert!(error.message.contains("ambiguous duplicate numbering id 7"));
}

#[test]
fn rejects_numbering_start_outside_the_docx_integer_range() {
    let mut b = HwpxBuilder::new();
    let cp = b.char_pr(CharPr::plain());
    let numbered = b.para_pr_with_heading(ParaPr::default(), "NUMBER", "1", 0);
    b.ref_list_xml(concat!(
        r#"<hh:numberings itemCnt="1"><hh:numbering id="1" start="0">"#,
        r#"<hh:paraHead start="4294967295" level="1" align="LEFT" useInstWidth="1" autoIndent="1" widthAdjust="0" textOffsetType="PERCENT" textOffset="50" numFormat="DIGIT" charPrIDRef="4294967295" checkable="0">^1.</hh:paraHead>"#,
        r#"</hh:numbering></hh:numberings>"#,
    ));
    b.section(para(&cp, &numbered, "너무 큰 시작 번호"));

    let error = read_document_from(Cursor::new(b.build()))
        .expect_err("start values outside the target integer range must fail");
    assert_eq!(
        error.code,
        officecli_hwpx::error::ErrorCode::UnsupportedFeature
    );
    assert!(error.message.contains("DOCX integer range"), "{error:?}");
}

#[test]
fn rejects_active_dangling_or_unrepresentable_numbering() {
    let cases = [
        (
            "dangling numbering",
            "NUMBER",
            "404",
            "",
            officecli_hwpx::error::ErrorCode::CorruptInput,
        ),
        (
            "image bullet",
            "BULLET",
            "4",
            concat!(
                r#"<hh:bullets itemCnt="1"><hh:bullet id="4" char="" useImage="1">"#,
                r#"<hh:img binaryItemIDRef="image1"/>"#,
                r#"<hh:paraHead level="0" align="LEFT" useInstWidth="0" autoIndent="1" widthAdjust="0" textOffsetType="PERCENT" textOffset="50" numFormat="DIGIT" charPrIDRef="4294967295" checkable="0"/>"#,
                r#"</hh:bullet></hh:bullets>"#,
            ),
            officecli_hwpx::error::ErrorCode::UnsupportedFeature,
        ),
        (
            "checkable bullet",
            "BULLET",
            "5",
            concat!(
                r#"<hh:bullets itemCnt="1"><hh:bullet id="5" char="□" checkedChar="☑" useImage="0">"#,
                r#"<hh:paraHead level="0" align="LEFT" useInstWidth="0" autoIndent="1" widthAdjust="0" textOffsetType="PERCENT" textOffset="50" numFormat="DIGIT" charPrIDRef="4294967295" checkable="1"/>"#,
                r#"</hh:bullet></hh:bullets>"#,
            ),
            officecli_hwpx::error::ErrorCode::UnsupportedFeature,
        ),
    ];

    for (label, kind, id_ref, definitions, code) in cases {
        let mut b = HwpxBuilder::new();
        let cp = b.char_pr(CharPr::plain());
        let pp = b.para_pr_with_heading(ParaPr::default(), kind, id_ref, 0);
        b.ref_list_xml(definitions);
        b.section(para(&cp, &pp, label));
        let error = read_document_from(Cursor::new(b.build()))
            .expect_err("active lossy numbering must fail closed");
        assert_eq!(error.code, code, "case {label}: {error:?}");
        assert!(
            error.message.contains("number") || error.message.contains("bullet"),
            "case {label}: {error:?}"
        );
    }
}

#[test]
fn resolves_outline_numbering_per_section_and_ignores_dormant_lossy_templates() {
    let mut b = HwpxBuilder::new();
    let cp = b.char_pr(CharPr::plain());
    let plain = b.para_pr(ParaPr::default());
    let outline = b.para_pr_with_heading(ParaPr::default(), "OUTLINE", "0", 0);
    b.ref_list_xml(concat!(
        r#"<hh:numberings itemCnt="2">"#,
        r#"<hh:numbering id="1" start="0">"#,
        r#"<hh:paraHead start="1" level="1" align="LEFT" useInstWidth="1" autoIndent="1" widthAdjust="0" textOffsetType="PERCENT" textOffset="50" numFormat="DIGIT" charPrIDRef="4294967295" checkable="0">^1.</hh:paraHead>"#,
        // The unused second level is intentionally unrepresentable. It must
        // remain dormant because no paragraph activates level 1.
        r#"<hh:paraHead start="1" level="2" align="LEFT" useInstWidth="1" autoIndent="1" widthAdjust="99" textOffsetType="PERCENT" textOffset="50" numFormat="CIRCLED_HANGUL_SYLLABLE" charPrIDRef="4294967295" checkable="1">^2.</hh:paraHead>"#,
        r#"</hh:numbering>"#,
        r#"<hh:numbering id="2" start="0">"#,
        r#"<hh:paraHead start="1" level="1" align="LEFT" useInstWidth="0" autoIndent="0" widthAdjust="0" textOffsetType="PERCENT" textOffset="0" numFormat="ROMAN_SMALL" charPrIDRef="4294967295" checkable="0">^1)</hh:paraHead>"#,
        r#"</hh:numbering></hh:numberings>"#,
        // A wholly dormant image bullet must not block unrelated outlines.
        r#"<hh:bullets itemCnt="1"><hh:bullet id="8" char="" useImage="1"><hh:img binaryItemIDRef="image1"/><hh:paraHead level="0" align="LEFT" useInstWidth="0" autoIndent="1" widthAdjust="0" textOffsetType="PERCENT" textOffset="50" numFormat="DIGIT" charPrIDRef="4294967295" checkable="0"/></hh:bullet></hh:bullets>"#,
    ));
    b.section(format!(
        concat!(
            r#"<hp:p paraPrIDRef="{plain}"><hp:run charPrIDRef="{cp}"><hp:secPr outlineShapeIDRef="1"/><hp:t>구역 1</hp:t></hp:run></hp:p>"#,
            r#"<hp:p paraPrIDRef="{outline}"><hp:run charPrIDRef="{cp}"><hp:t>첫 개요</hp:t></hp:run></hp:p>"#,
        ),
        plain = plain,
        cp = cp,
        outline = outline,
    ));
    b.section(format!(
        concat!(
            r#"<hp:p paraPrIDRef="{plain}"><hp:run charPrIDRef="{cp}"><hp:secPr outlineShapeIDRef="2"/><hp:t>구역 2</hp:t></hp:run></hp:p>"#,
            r#"<hp:p paraPrIDRef="{outline}"><hp:run charPrIDRef="{cp}"><hp:t>둘째 개요</hp:t></hp:run></hp:p>"#,
        ),
        plain = plain,
        cp = cp,
        outline = outline,
    ));

    let doc = parse(&b);
    assert_eq!(doc.numberings.len(), 2, "dormant image bullet stays absent");
    assert_eq!(doc.numberings[0].levels.len(), 1);
    let items = emit_document(&doc);
    let paragraph = |text: &str| {
        items
            .iter()
            .find(|item| item.props.get("text").and_then(|v| v.as_str()) == Some(text))
            .expect("authored outline paragraph")
    };
    assert_eq!(paragraph("첫 개요").props["numId"], "1");
    assert_eq!(paragraph("둘째 개요").props["numId"], "2");
    let second = items
        .iter()
        .find(|item| {
            item.r#type == Some("level")
                && item.parent.as_deref() == Some("/numbering/abstractNum[@id=2]")
                && item.props["ilvl"] == "0"
        })
        .expect("second outline level");
    assert_eq!(second.props["format"], "lowerRoman");
}

#[test]
fn ignores_a_dormant_tenth_hwp_numbering_level() {
    let mut b = HwpxBuilder::new();
    let cp = b.char_pr(CharPr::plain());
    let numbered = b.para_pr_with_heading(ParaPr::default(), "NUMBER", "3", 0);
    b.ref_list_xml(concat!(
        r#"<hh:numberings itemCnt="1"><hh:numbering id="3" start="0">"#,
        r#"<hh:paraHead start="1" level="1" align="LEFT" useInstWidth="1" autoIndent="1" widthAdjust="0" textOffsetType="PERCENT" textOffset="50" numFormat="DIGIT" charPrIDRef="4294967295" checkable="0">^1.</hh:paraHead>"#,
        // RHWP preserves HWP's tenth definition level. DOCX has only levels
        // 0..=8, but this entry is dormant when authored paragraphs use level 0.
        r#"<hh:paraHead start="1" level="10" align="LEFT" useInstWidth="1" autoIndent="1" widthAdjust="99" textOffsetType="PERCENT" textOffset="25" numFormat="CIRCLED_HANGUL_SYLLABLE" charPrIDRef="4294967295" checkable="1">^1.^A</hh:paraHead>"#,
        r#"</hh:numbering></hh:numberings>"#,
    ));
    b.section(para(&cp, &numbered, "첫 단계만 활성"));

    let levels: Vec<_> = emit_document(&parse(&b))
        .into_iter()
        .filter(|item| item.r#type == Some("level"))
        .collect();
    assert_eq!(levels.len(), 1);
    assert_eq!(levels[0].props["ilvl"], "0");
}

#[test]
fn preserves_pua_bullet_and_verified_marker_character_style() {
    let mut b = HwpxBuilder::new();
    let body_cp = b.char_pr(CharPr::plain());
    let marker_cp = b.char_pr(CharPr {
        height: Some(1400),
        text_color: Some("#123456".into()),
        bold: true,
        italic: true,
        font_hangul: Some("Wingdings".into()),
        ..CharPr::default()
    });
    let bullet = b.para_pr_with_heading(ParaPr::default(), "BULLET", "4", 0);
    b.ref_list_xml(format!(
        concat!(
            r#"<hh:bullets itemCnt="1"><hh:bullet id="4" char="&#xF06C;" useImage="0">"#,
            r#"<hh:paraHead level="0" align="CENTER" useInstWidth="0" autoIndent="1" widthAdjust="0" textOffsetType="PERCENT" textOffset="50" numFormat="DIGIT" charPrIDRef="{marker_cp}" checkable="0"/>"#,
            r#"</hh:bullet></hh:bullets>"#,
        ),
        marker_cp = marker_cp,
    ));
    b.section(para(&body_cp, &bullet, "PUA 글머리"));

    let doc = parse(&b);
    assert_eq!(doc.count_private_use_chars(), 1);
    let definition = emit_document(&doc)
        .into_iter()
        .find(|item| item.r#type == Some("level"))
        .expect("bullet level");
    assert_eq!(definition.props["lvlText"], "\u{F06C}");
    assert_eq!(definition.props["justification"], "center");
    assert_eq!(definition.props["font"], "Wingdings");
    assert_eq!(definition.props["size"], "14pt");
    assert_eq!(definition.props["color"], "#123456");
    assert_eq!(definition.props["bold"], "true");
    assert_eq!(definition.props["italic"], "true");
}

#[test]
fn accepts_hancom_oracle_auto_indent_bullet_offsets() {
    for offset in ["10", "15", "50"] {
        let mut b = HwpxBuilder::new();
        let cp = b.char_pr(CharPr::plain());
        let bullet = b.para_pr_with_heading(ParaPr::default(), "BULLET", "6", 0);
        b.ref_list_xml(format!(
            concat!(
                r#"<hh:bullets itemCnt="1"><hh:bullet id="6" char="-" useImage="0">"#,
                r#"<hh:paraHead level="0" align="LEFT" useInstWidth="0" autoIndent="1" widthAdjust="0" textOffsetType="PERCENT" textOffset="{offset}" numFormat="DIGIT" charPrIDRef="4294967295" checkable="0"/>"#,
                r#"</hh:bullet></hh:bullets>"#,
            ),
            offset = offset,
        ));
        b.section(para(&cp, &bullet, "한컴 글머리표 배치"));

        let items = emit_document(&parse(&b));
        let level = items
            .iter()
            .find(|item| item.r#type == Some("level"))
            .expect("bullet numbering level");
        assert_eq!(level.props["format"], "bullet", "offset={offset}");
        assert_eq!(level.props["indent"], "0", "offset={offset}");
        assert_eq!(level.props["hanging"], "0", "offset={offset}");
        assert_eq!(level.props["suff"], "space", "offset={offset}");
    }
}

#[test]
fn rejects_non_integer_numbering_geometry_without_rounding() {
    let cases = [
        ("0.4", "50", "widthAdjust"),
        ("NaN", "50", "widthAdjust"),
        ("0", "15.5", "textOffset"),
        ("0", "NaN", "textOffset"),
    ];
    for (width, offset, message) in cases {
        let mut b = HwpxBuilder::new();
        let cp = b.char_pr(CharPr::plain());
        let numbered = b.para_pr_with_heading(ParaPr::default(), "NUMBER", "6", 0);
        b.ref_list_xml(format!(
            concat!(
                r#"<hh:numberings itemCnt="1"><hh:numbering id="6" start="0">"#,
                r#"<hh:paraHead start="1" level="1" align="LEFT" useInstWidth="1" autoIndent="1" widthAdjust="{width}" textOffsetType="PERCENT" textOffset="{offset}" numFormat="DIGIT" charPrIDRef="4294967295" checkable="0">^1.</hh:paraHead>"#,
                r#"</hh:numbering></hh:numberings>"#,
            ),
            width = width,
            offset = offset,
        ));
        b.section(para(&cp, &numbered, "비정수 번호 배치"));

        let error = read_document_from(Cursor::new(b.build()))
            .expect_err("non-integer numbering geometry must not be rounded");
        assert_eq!(
            error.code,
            officecli_hwpx::error::ErrorCode::CorruptInput,
            "width={width} offset={offset}: {error:?}"
        );
        assert!(error.message.contains(message), "{error:?}");
    }
}

#[test]
fn rejects_active_unverified_numbering_layouts_before_emission() {
    let cases = [
        ("20", "1", "PERCENT", "50", "widthAdjust"),
        ("0", "1", "PERCENT", "15", "unverified list layout"),
        ("0", "1", "PERCENT", "25", "unverified list layout"),
        ("0", "1", "HWPUNIT", "50", "unverified list layout"),
        ("0", "0", "PERCENT", "50", "unverified list layout"),
    ];
    for (width, auto, offset_type, offset, message) in cases {
        let mut b = HwpxBuilder::new();
        let cp = b.char_pr(CharPr::plain());
        let numbered = b.para_pr_with_heading(ParaPr::default(), "NUMBER", "6", 0);
        b.ref_list_xml(format!(
            concat!(
                r#"<hh:numberings itemCnt="1"><hh:numbering id="6" start="0">"#,
                r#"<hh:paraHead start="1" level="1" align="LEFT" useInstWidth="1" autoIndent="{auto}" widthAdjust="{width}" textOffsetType="{offset_type}" textOffset="{offset}" numFormat="DIGIT" charPrIDRef="4294967295" checkable="0">^1.</hh:paraHead>"#,
                r#"</hh:numbering></hh:numberings>"#,
            ),
            auto = auto,
            width = width,
            offset_type = offset_type,
            offset = offset,
        ));
        b.section(para(&cp, &numbered, "검증되지 않은 번호 배치"));

        let error = read_document_from(Cursor::new(b.build()))
            .expect_err("active unverified layout cannot be emitted");
        assert_eq!(
            error.code,
            officecli_hwpx::error::ErrorCode::UnsupportedFeature
        );
        assert!(error.message.contains(message), "{error:?}");
    }
}
