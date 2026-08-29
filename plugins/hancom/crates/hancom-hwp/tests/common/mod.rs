//! 테스트용 HWPX 파일 생성기.
//!
//! 시드 코드는 `<hwpx><body><p>...</p></body></hwpx>` 같은 존재하지 않는 문자열을
//! 픽스처로 썼다. 그건 HWPX가 아니라서 아무것도 검증하지 못한다.
//! 여기서는 실제 구조 — ZIP + `Contents/content.hpf`(OPF) + `header.xml` +
//! `sectionN.xml` + `BinData/` — 를 갖춘 파일을 만든다.
//!
//! 구조 근거: `unhwp-0.7.0/src/hwpx/container.rs`, `section.rs`, `styles.rs`.

#![allow(dead_code)] // 테스트 파일마다 쓰는 헬퍼가 달라 일부는 미사용일 수 있다.

use std::io::{Cursor, Write};

use zip::write::SimpleFileOptions;
use zip::ZipWriter;

pub const NS_DECL: &str = concat!(
    r#" xmlns:hs="http://www.hancom.co.kr/hwpml/2011/section""#,
    r#" xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph""#,
    r#" xmlns:hh="http://www.hancom.co.kr/hwpml/2011/head""#,
    r#" xmlns:hc="http://www.hancom.co.kr/hwpml/2011/core""#,
);

/// HWPX 파일 빌더.
pub struct HwpxBuilder {
    char_prs: Vec<String>,
    para_prs: Vec<String>,
    ref_list_extras: Vec<String>,
    sections: Vec<String>,
    bindata: Vec<(String, Vec<u8>)>,
    manifest_extra: Vec<(String, String, String)>, // (id, href, media-type)
    spine_extra: Vec<usize>,
    extra_entries: Vec<(String, Vec<u8>)>,
    include_mimetype: bool,
    include_hpf: bool,
    include_header: bool,
    mimetype_override: Option<String>,
}

impl Default for HwpxBuilder {
    fn default() -> Self {
        Self {
            char_prs: Vec::new(),
            para_prs: Vec::new(),
            ref_list_extras: Vec::new(),
            sections: Vec::new(),
            bindata: Vec::new(),
            manifest_extra: Vec::new(),
            spine_extra: Vec::new(),
            extra_entries: Vec::new(),
            include_mimetype: true,
            include_hpf: true,
            include_header: true,
            mimetype_override: None,
        }
    }
}

impl HwpxBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// `<hh:charPr id="N" ...>` 를 추가하고 그 id를 돌려준다.
    pub fn char_pr(&mut self, inner_and_attrs: CharPr) -> String {
        let id = self.char_prs.len().to_string();
        self.char_prs.push(inner_and_attrs.to_xml(&id));
        id
    }

    /// `<hh:paraPr id="N" ...>` 를 추가하고 그 id를 돌려준다.
    pub fn para_pr(&mut self, p: ParaPr) -> String {
        let id = self.para_prs.len().to_string();
        self.para_prs.push(p.to_xml(&id));
        id
    }

    /// 목록/글머리표처럼 `hh:refList` 직속에 놓이는 테스트 XML을 추가한다.
    pub fn ref_list_xml(&mut self, xml: impl Into<String>) -> &mut Self {
        self.ref_list_extras.push(xml.into());
        self
    }

    /// `hh:heading`을 가진 문단 모양을 추가한다.
    pub fn para_pr_with_heading(
        &mut self,
        p: ParaPr,
        heading_type: &str,
        id_ref: &str,
        level: usize,
    ) -> String {
        let id = self.para_prs.len().to_string();
        let heading =
            format!(r#"<hh:heading type="{heading_type}" idRef="{id_ref}" level="{level}"/>"#);
        self.para_prs.push(p.to_xml_with_extra(&id, &heading));
        id
    }

    /// 섹션 본문(`hs:sec` 안에 들어갈 내용)을 추가한다.
    pub fn section(&mut self, body_xml: impl Into<String>) -> &mut Self {
        self.sections.push(body_xml.into());
        self
    }

    /// BinData 항목을 추가한다. `id`는 `binaryItemIDRef`로 참조할 이름.
    pub fn bindata(&mut self, id: &str, filename: &str, bytes: Vec<u8>, media: &str) -> &mut Self {
        self.bindata.push((format!("BinData/{filename}"), bytes));
        self.manifest_extra.push((
            id.to_string(),
            format!("BinData/{filename}"),
            media.to_string(),
        ));
        self
    }

    pub fn without_mimetype(&mut self) -> &mut Self {
        self.include_mimetype = false;
        self
    }

    pub fn with_bad_mimetype(&mut self, v: &str) -> &mut Self {
        self.mimetype_override = Some(v.to_string());
        self
    }

    /// content.hpf를 뺀다. 파서는 `Contents/section*.xml` 폴백을 써야 한다.
    pub fn without_hpf(&mut self) -> &mut Self {
        self.include_hpf = false;
        self
    }

    pub fn without_header(&mut self) -> &mut Self {
        self.include_header = false;
        self
    }

    /// 이미 선언된 section을 spine에 추가로 참조한다.
    pub fn repeat_section_in_spine(&mut self, index: usize, times: usize) -> &mut Self {
        self.spine_extra.extend(std::iter::repeat_n(index, times));
        self
    }

    /// 패키지 자원 제한 테스트용 임의 ZIP 엔트리를 추가한다.
    pub fn extra_entry(&mut self, name: impl Into<String>, bytes: Vec<u8>) -> &mut Self {
        self.extra_entries.push((name.into(), bytes));
        self
    }

    /// 실제 ZIP 바이트를 만든다.
    pub fn build(&self) -> Vec<u8> {
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut zip = ZipWriter::new(&mut cursor);
            let opts =
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            // mimetype은 관례상 무압축 첫 항목이다.
            let stored =
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);

            if self.include_mimetype {
                let v = self
                    .mimetype_override
                    .clone()
                    .unwrap_or_else(|| "application/hwp+zip".to_string());
                zip.start_file("mimetype", stored).expect("start mimetype");
                zip.write_all(v.as_bytes()).expect("write mimetype");
            }

            if self.include_hpf {
                zip.start_file("Contents/content.hpf", opts)
                    .expect("start hpf");
                zip.write_all(self.hpf().as_bytes()).expect("write hpf");
            }

            if self.include_header {
                zip.start_file("Contents/header.xml", opts)
                    .expect("start header");
                zip.write_all(self.header_xml().as_bytes())
                    .expect("write header");
            }

            for (i, body) in self.sections.iter().enumerate() {
                zip.start_file(format!("Contents/section{i}.xml"), opts)
                    .expect("start section");
                zip.write_all(section_xml(body).as_bytes())
                    .expect("write section");
            }

            for (name, bytes) in &self.bindata {
                zip.start_file(name.clone(), opts).expect("start bindata");
                zip.write_all(bytes).expect("write bindata");
            }

            for (name, bytes) in &self.extra_entries {
                zip.start_file(name.clone(), opts)
                    .expect("start extra entry");
                zip.write_all(bytes).expect("write extra entry");
            }

            zip.finish().expect("finish zip");
        }
        cursor.into_inner()
    }

    /// 임시 디렉터리에 `.hwpx` 파일로 쓴다.
    ///
    /// `TempDir`을 **반환값에 담아** 돌려준다. 시드 코드는 이걸 함수 안에서
    /// drop해서 파일이 즉시 삭제됐다. 호출자가 살려둬야 한다.
    pub fn write_to_temp(&self, name: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("create temp dir");
        let path = dir.path().join(name);
        std::fs::write(&path, self.build()).expect("write hwpx");
        (dir, path)
    }

    fn hpf(&self) -> String {
        let mut items = String::new();
        let mut spine = String::new();

        if self.include_header {
            items.push_str(
                r#"<opf:item id="header" href="Contents/header.xml" media-type="application/xml"/>"#,
            );
            spine.push_str(r#"<opf:itemref idref="header" linear="yes"/>"#);
        }
        for i in 0..self.sections.len() {
            items.push_str(&format!(
                r#"<opf:item id="section{i}" href="Contents/section{i}.xml" media-type="application/xml"/>"#
            ));
            spine.push_str(&format!(
                r#"<opf:itemref idref="section{i}" linear="yes"/>"#
            ));
        }
        for i in &self.spine_extra {
            spine.push_str(&format!(
                r#"<opf:itemref idref="section{i}" linear="yes"/>"#
            ));
        }
        for (id, href, media) in &self.manifest_extra {
            items.push_str(&format!(
                r#"<opf:item id="{id}" href="{href}" media-type="{media}"/>"#
            ));
        }

        // 실제 HWPX처럼 한 줄로 만든다. 행 단위 파서를 쓰면 여기서 깨진다.
        format!(
            concat!(
                r#"<?xml version="1.0" encoding="UTF-8"?>"#,
                r#"<opf:package xmlns:opf="http://www.idpf.org/2007/opf/" version="">"#,
                r#"<opf:manifest>{}</opf:manifest>"#,
                r#"<opf:spine>{}</opf:spine>"#,
                r#"</opf:package>"#,
            ),
            items, spine
        )
    }

    fn header_xml(&self) -> String {
        format!(
            concat!(
                r#"<?xml version="1.0" encoding="UTF-8"?>"#,
                r#"<hh:head{ns} version="1.4" secCnt="1">"#,
                r#"<hh:refList>"#,
                r#"<hh:charProperties itemCnt="{cc}">{chars}</hh:charProperties>"#,
                r#"<hh:paraProperties itemCnt="{pc}">{paras}</hh:paraProperties>"#,
                r#"{extras}"#,
                r#"</hh:refList>"#,
                r#"</hh:head>"#,
            ),
            ns = NS_DECL,
            cc = self.char_prs.len(),
            chars = self.char_prs.join(""),
            pc = self.para_prs.len(),
            paras = self.para_prs.join(""),
            extras = self.ref_list_extras.join(""),
        )
    }
}

fn section_xml(body: &str) -> String {
    format!(r#"<?xml version="1.0" encoding="UTF-8"?><hs:sec{NS_DECL}>{body}</hs:sec>"#)
}

/// `hh:charPr` 명세.
#[derive(Default, Clone)]
pub struct CharPr {
    /// HWPUNIT (1/100 pt). 1000 → 10pt.
    pub height: Option<i64>,
    pub text_color: Option<String>,
    pub shade_color: Option<String>,
    pub bold: bool,
    pub italic: bool,
    /// `None`이면 요소 없음, `Some("NONE")`이면 명시적 없음.
    pub underline: Option<String>,
    pub strikeout: Option<String>,
    pub font_hangul: Option<String>,
    pub superscript: bool,
    pub subscript: bool,
}

impl CharPr {
    pub fn plain() -> Self {
        Self {
            height: Some(1000),
            text_color: Some("#000000".into()),
            ..Default::default()
        }
    }

    pub fn bold() -> Self {
        Self {
            bold: true,
            ..Self::plain()
        }
    }

    fn to_xml(&self, id: &str) -> String {
        let mut attrs = format!(r#" id="{id}""#);
        if let Some(h) = self.height {
            attrs.push_str(&format!(r#" height="{h}""#));
        }
        if let Some(c) = &self.text_color {
            attrs.push_str(&format!(r#" textColor="{c}""#));
        }
        if let Some(c) = &self.shade_color {
            attrs.push_str(&format!(r#" shadeColor="{c}""#));
        }

        let mut inner = String::new();
        if let Some(f) = &self.font_hangul {
            inner.push_str(&format!(r#"<hh:fontRef hangul="{f}" latin="{f}"/>"#));
        }
        if self.bold {
            inner.push_str("<hh:bold/>");
        }
        if self.italic {
            inner.push_str("<hh:italic/>");
        }
        if let Some(t) = &self.underline {
            inner.push_str(&format!(r#"<hh:underline type="{t}"/>"#));
        }
        if let Some(t) = &self.strikeout {
            inner.push_str(&format!(r#"<hh:strikeout type="{t}"/>"#));
        }
        if self.superscript {
            inner.push_str("<hh:supscript/>");
        }
        if self.subscript {
            inner.push_str("<hh:subscript/>");
        }

        if inner.is_empty() {
            format!("<hh:charPr{attrs}/>")
        } else {
            format!("<hh:charPr{attrs}>{inner}</hh:charPr>")
        }
    }
}

/// `hh:paraPr` 명세. 길이는 HWPUNIT.
#[derive(Default, Clone)]
pub struct ParaPr {
    pub align: Option<String>,
    pub indent_first: Option<i64>,
    pub margin_left: Option<i64>,
    pub space_before: Option<i64>,
    pub space_after: Option<i64>,
    /// 퍼센트 (160 → 1.6배).
    pub line_spacing_percent: Option<i64>,
}

impl ParaPr {
    pub fn centered() -> Self {
        Self {
            align: Some("CENTER".into()),
            ..Default::default()
        }
    }

    fn to_xml(&self, id: &str) -> String {
        self.to_xml_with_extra(id, "")
    }

    fn to_xml_with_extra(&self, id: &str, extra: &str) -> String {
        let mut inner = String::new();
        if let Some(a) = &self.align {
            inner.push_str(&format!(
                r#"<hh:align horizontal="{a}" vertical="BASELINE"/>"#
            ));
        }
        let has_margin = self.indent_first.is_some()
            || self.margin_left.is_some()
            || self.space_before.is_some()
            || self.space_after.is_some();
        if has_margin {
            inner.push_str("<hh:margin>");
            if let Some(v) = self.indent_first {
                inner.push_str(&format!(r#"<hh:intent value="{v}" unit="HWPUNIT"/>"#));
            }
            if let Some(v) = self.margin_left {
                inner.push_str(&format!(r#"<hh:left value="{v}" unit="HWPUNIT"/>"#));
            }
            if let Some(v) = self.space_before {
                inner.push_str(&format!(r#"<hh:prev value="{v}" unit="HWPUNIT"/>"#));
            }
            if let Some(v) = self.space_after {
                inner.push_str(&format!(r#"<hh:next value="{v}" unit="HWPUNIT"/>"#));
            }
            inner.push_str("</hh:margin>");
        }
        if let Some(p) = self.line_spacing_percent {
            inner.push_str(&format!(
                r#"<hh:lineSpacing type="PERCENT" value="{p}" unit="HWPUNIT"/>"#
            ));
        }
        inner.push_str(extra);

        if inner.is_empty() {
            format!(r#"<hh:paraPr id="{id}"/>"#)
        } else {
            format!(r#"<hh:paraPr id="{id}">{inner}</hh:paraPr>"#)
        }
    }
}

// ── 본문 XML 조각 헬퍼 ──

/// 런 하나짜리 문단.
pub fn para(char_pr: &str, para_pr: &str, text: &str) -> String {
    para_with_runs(para_pr, &[(char_pr, text)])
}

/// 여러 런을 가진 문단.
pub fn para_with_runs(para_pr: &str, runs: &[(&str, &str)]) -> String {
    let body: String = runs
        .iter()
        .map(|(cp, t)| {
            format!(
                r#"<hp:run charPrIDRef="{cp}"><hp:t>{}</hp:t></hp:run>"#,
                escape(t)
            )
        })
        .collect();
    wrap_para(para_pr, &body)
}

/// 임의의 run 내용을 담은 문단.
pub fn wrap_para(para_pr: &str, inner: &str) -> String {
    format!(
        r#"<hp:p id="0" paraPrIDRef="{para_pr}" styleIDRef="0">{inner}<hp:linesegarray><hp:lineSeg textpos="0" vertpos="0"/></hp:linesegarray></hp:p>"#
    )
}

/// `hp:lineBreak`를 포함한 문단.
pub fn para_with_linebreak(char_pr: &str, para_pr: &str, first: &str, second: &str) -> String {
    let inner = format!(
        r#"<hp:run charPrIDRef="{cp}"><hp:t>{a}</hp:t></hp:run><hp:run charPrIDRef="{cp}"><hp:lineBreak/></hp:run><hp:run charPrIDRef="{cp}"><hp:t>{b}</hp:t></hp:run>"#,
        cp = char_pr,
        a = escape(first),
        b = escape(second)
    );
    wrap_para(para_pr, &inner)
}

/// 셀 하나. `text`가 여러 개면 문단 여러 개가 된다.
pub struct CellSpec {
    pub row: usize,
    pub col: usize,
    pub row_span: usize,
    pub col_span: usize,
    /// HWPUNIT.
    pub width: i64,
    pub texts: Vec<String>,
    pub char_pr: String,
    pub fill: Option<String>,
}

impl CellSpec {
    pub fn new(row: usize, col: usize, text: &str) -> Self {
        Self {
            row,
            col,
            row_span: 1,
            col_span: 1,
            width: 4000,
            texts: vec![text.to_string()],
            char_pr: "0".into(),
            fill: None,
        }
    }

    pub fn span(mut self, row_span: usize, col_span: usize) -> Self {
        self.row_span = row_span;
        self.col_span = col_span;
        self
    }

    pub fn fill(mut self, color: &str) -> Self {
        self.fill = Some(color.to_string());
        self
    }

    fn to_xml(&self) -> String {
        let paras: String = self
            .texts
            .iter()
            .map(|t| para(&self.char_pr, "0", t))
            .collect();
        let fill = match &self.fill {
            Some(c) => format!(
                r##"<hp:cellBrush><hc:fillBrush faceColor="{c}" hatchColor="#000000"/></hp:cellBrush>"##
            ),
            None => String::new(),
        };
        format!(
            concat!(
                r#"<hp:tc name="" header="0" hasMargin="0" protect="0" editable="0" dirty="0" borderFillIDRef="1">"#,
                r#"<hp:subList id="" textDirection="HORIZONTAL" lineWrap="BREAK" vertAlign="TOP">{paras}</hp:subList>"#,
                r#"<hp:cellAddr colAddr="{col}" rowAddr="{row}"/>"#,
                r#"<hp:cellSpan colSpan="{cspan}" rowSpan="{rspan}"/>"#,
                r#"<hp:cellSz width="{w}" height="1000"/>"#,
                r#"<hp:cellMargin left="0" right="0" top="0" bottom="0"/>"#,
                r#"{fill}"#,
                r#"</hp:tc>"#,
            ),
            paras = paras,
            col = self.col,
            row = self.row,
            cspan = self.col_span,
            rspan = self.row_span,
            w = self.width,
            fill = fill,
        )
    }
}

/// 표를 담은 문단을 만든다. HWPX는 표를 `hp:p > hp:run > hp:tbl`에 넣는다.
pub fn table(rows: usize, cols: usize, cells: &[CellSpec]) -> String {
    let mut trs = String::new();
    for r in 0..rows {
        let tcs: String = cells
            .iter()
            .filter(|c| c.row == r)
            .map(|c| c.to_xml())
            .collect();
        trs.push_str(&format!("<hp:tr>{tcs}</hp:tr>"));
    }
    let tbl = format!(
        r#"<hp:tbl id="1" zOrder="0" numberingType="TABLE" textWrap="TOP_AND_BOTTOM" rowCnt="{rows}" colCnt="{cols}" cellSpacing="0" borderFillIDRef="1"><hp:sz width="8000" height="2000"/>{trs}</hp:tbl>"#
    );
    wrap_para("0", &format!(r#"<hp:run charPrIDRef="0">{tbl}</hp:run>"#))
}

/// 이미지를 담은 문단.
pub fn picture(bin_item_id: &str, width: i64, height: i64, alt: Option<&str>) -> String {
    let alt_attr = match alt {
        Some(a) => format!(r#" alt="{}""#, escape(a)),
        None => String::new(),
    };
    let pic = format!(
        concat!(
            r#"<hp:pic reverse="0" id="2" zOrder="1" textWrap="SQUARE">"#,
            r#"<hp:sz width="{w}" height="{h}"/>"#,
            r#"<hp:img binaryItemIDRef="{id}" bright="0" contrast="0" effect="REAL_PIC"{alt}/>"#,
            r#"<hp:imgRect><hc:pt0 x="0" y="0"/></hp:imgRect>"#,
            r#"</hp:pic>"#,
        ),
        w = width,
        h = height,
        id = bin_item_id,
        alt = alt_attr,
    );
    wrap_para("0", &format!(r#"<hp:run charPrIDRef="0">{pic}</hp:run>"#))
}

/// `hp:checkBtn` 폼 컨트롤을 담은 문단.
///
/// 실측 구조 근거: 2026 대구문학관 참가신청서 `Contents/section0.xml`.
pub fn para_with_checkbox(char_pr: &str, text: &str, name: &str, checked: bool) -> String {
    let value = if checked { "CHECKED" } else { "UNCHECKED" };
    let inner = format!(
        concat!(
            r#"<hp:run charPrIDRef="{cp}"><hp:t>{t}</hp:t>"#,
            r#"<hp:checkBtn caption="" value="{v}" radioGroupName="" triState="0""#,
            r##" backStyle="1" name="{n}" foreColor="#000000" backColor="#FFFFFF""##,
            r#" groupName="" tabStop="1" editable="1" tabOrder="2" enabled="1""#,
            r#" borderTypeIDRef="0" drawFrame="1" printable="1" command="">"#,
            r#"<hp:formCharPr charPrIDRef="0" followContext="0" autoSz="0" wordWrap="0"/>"#,
            r#"<hp:sz width="1168" widthRelTo="ABSOLUTE" height="1433" heightRelTo="ABSOLUTE" protect="0"/>"#,
            r#"<hp:pos treatAsChar="1" affectLSpacing="0" flowWithText="1"/>"#,
            r#"</hp:checkBtn></hp:run>"#,
        ),
        cp = char_pr,
        t = escape(text),
        v = value,
        n = name,
    );
    wrap_para("0", &inner)
}

/// `hp:fieldBegin type="CLICK_HERE"` 누름틀. 실측 구조 그대로.
///
/// `inner`가 비어 있으면 빈 입력 슬롯, 내용이 있으면 이미 작성된 필드다.
pub fn para_with_click_here(char_pr: &str, field_id: &str, hint: &str, inner_text: &str) -> String {
    let hint_len = hint.chars().count();
    let command =
        format!("Clickhere:set:51:Direction:wstring:{hint_len}:{hint} HelpState:wstring:0:  ");
    let content = if inner_text.is_empty() {
        format!(r#"<hp:run charPrIDRef="{char_pr}"><hp:t></hp:t></hp:run>"#)
    } else {
        format!(
            r#"<hp:run charPrIDRef="{char_pr}"><hp:t>{}</hp:t></hp:run>"#,
            escape(inner_text)
        )
    };
    let inner = format!(
        concat!(
            r#"<hp:run charPrIDRef="{cp}"><hp:ctrl>"#,
            r#"<hp:fieldBegin id="{id}" type="CLICK_HERE" name="" editable="1">"#,
            r#"<hp:parameters cnt="1" name="">"#,
            r#"<hp:stringParam name="Command">{cmd}</hp:stringParam>"#,
            r#"</hp:parameters></hp:fieldBegin></hp:ctrl></hp:run>"#,
            r#"{content}"#,
            r#"<hp:run charPrIDRef="{cp}"><hp:ctrl>"#,
            r#"<hp:fieldEnd beginIDRef="{id}"/>"#,
            r#"</hp:ctrl></hp:run>"#,
        ),
        cp = char_pr,
        id = field_id,
        cmd = escape(&command),
        content = content,
    );
    wrap_para("0", &inner)
}

/// 최소한의 유효 PNG 바이트 (1x1 투명).
pub fn tiny_png() -> Vec<u8> {
    vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ]
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// 가장 흔한 형태: 문단 몇 개만 있는 문서.
pub fn simple_doc(paragraphs: &[&str]) -> HwpxBuilder {
    let mut b = HwpxBuilder::new();
    let cp = b.char_pr(CharPr::plain());
    let pp = b.para_pr(ParaPr::default());
    let body: String = paragraphs.iter().map(|t| para(&cp, &pp, t)).collect();
    b.section(body);
    b
}
