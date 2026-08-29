//! 실제 ZIP+OWPML 파일을 대상으로 한 파서 검증.

mod common;

use std::io::Cursor;

use common::*;
use officecli_hwpx::owpml::model::{Align, Block, Inline, NoteKind, VertAlign};
use officecli_hwpx::owpml::read_document_from;

fn parse(b: &HwpxBuilder) -> officecli_hwpx::owpml::model::Document {
    read_document_from(Cursor::new(b.build())).expect("document parses")
}

#[test]
fn reads_paragraph_text_in_order() {
    let doc = parse(&simple_doc(&["첫 번째 문단", "두 번째 문단", "세 번째"]));
    let texts: Vec<String> = doc.paragraphs().map(|p| p.plain_text()).collect();
    assert_eq!(
        texts,
        vec!["첫 번째 문단", "두 번째 문단", "세 번째"]
    );
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
    b.section(para_with_runs(
        &pp,
        &[(&plain, "보통 "), (&fancy, "강조")],
    ));

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
    b.section(para_with_runs(&pp, &[(&cp, "가"), (&cp, "나"), (&cp, "다")]));

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
    assert_eq!(doc.paragraphs().count(), 1, "lineBreak must stay inside one paragraph");
    let p = doc.paragraphs().next().expect("paragraph");
    assert!(
        p.inlines.iter().any(|i| matches!(i, Inline::LineBreak)),
        "lineBreak inline must be recorded"
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
        <hp:t>앞</hp:t><hp:ctrl><hp:footNote number="3" instId="41"><hp:subList>
          <hp:p paraPrIDRef="{pp}" styleIDRef="0"><hp:run charPrIDRef="{cp}"><hp:ctrl><hp:autoNum num="3" numType="FOOTNOTE"><hp:autoNumFormat type="DIGIT" suffixChar="" supscript="1"/></hp:autoNum></hp:ctrl><hp:t>각주 첫 문단</hp:t></hp:run></hp:p>
          <hp:p paraPrIDRef="{pp}" styleIDRef="0"><hp:run charPrIDRef="{cp}"><hp:t>각주 둘째</hp:t><hp:lineBreak/><hp:t>줄</hp:t></hp:run></hp:p>
        </hp:subList></hp:footNote></hp:ctrl>
        <hp:t>중간</hp:t><hp:ctrl><hp:endNote number="7" instId="42"><hp:subList>
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
    assert_eq!(
        notes[0]
            .paragraphs()
            .map(|p| p.plain_text())
            .collect::<Vec<_>>(),
        vec!["각주 첫 문단", "각주 둘째\n줄",]
    );
    assert_eq!(notes[1].kind, NoteKind::Endnote);
    assert_eq!(notes[1].number, Some(7));
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
    collect_boxes(&doc.blocks, &mut boxes);

    assert_eq!(
        boxes.len(),
        2,
        "checkboxes in a nested table must survive"
    );
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
    b.section(para_with_click_here(&cp, "1520616239", "기재하지 마세요.", ""));

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
    let kinds: Vec<&str> = doc
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
fn skips_linesegarray_render_cache() {
    // linesegarray는 렌더링 좌표 캐시다. 본문에 섞여 나오면 안 된다.
    let doc = parse(&simple_doc(&["본문"]));
    let p = doc.paragraphs().next().expect("paragraph");
    assert_eq!(p.plain_text(), "본문");
    assert_eq!(p.inlines.len(), 1);
}
