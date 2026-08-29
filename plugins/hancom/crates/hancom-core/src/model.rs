//! HWPX(OWPML)와 OfficeCLI 어휘 사이의 중간 문서 모델.
//!
//! 파서는 OWPML을 이 모델로 옮기고, emitter는 이 모델만 보고 BatchItem을 만든다.
//! 두 단계를 분리해야 각각을 독립적으로 테스트할 수 있다.

use std::sync::Arc;

/// HWPUNIT은 1/7200 inch.
///
/// 근거: `unhwp-0.7.0/src/hwpx/styles.rs:107` — "HWPML uses height in
/// charShape (in hwpunit = 1/7200 inch)", "1 point = 100 hwpunit".
pub const HWPUNIT_PER_INCH: f64 = 7200.0;
/// twip은 1/1440 inch.
pub const TWIP_PER_INCH: f64 = 1440.0;
/// pt는 1/72 inch.
pub const HWPUNIT_PER_POINT: f64 = HWPUNIT_PER_INCH / 72.0; // = 100

/// HWPUNIT → twip. 7200/1440 = 5.
pub fn hwpunit_to_twip(v: i64) -> i64 {
    (v as f64 * (TWIP_PER_INCH / HWPUNIT_PER_INCH)).round() as i64
}

/// HWPUNIT → pt.
pub fn hwpunit_to_point(v: i64) -> f64 {
    v as f64 / HWPUNIT_PER_POINT
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
    Justify,
    Distribute,
}

impl Align {
    /// OWPML `hh:align/@horizontal` 값을 해석한다.
    ///
    /// OWPML은 대문자(`LEFT`), unhwp가 다루는 변형은 소문자(`left`, `both`)를
    /// 모두 포함하므로 대소문자 무시로 처리한다.
    pub fn from_owpml(s: &str) -> Option<Self> {
        match s.to_ascii_uppercase().as_str() {
            "LEFT" => Some(Self::Left),
            "CENTER" => Some(Self::Center),
            "RIGHT" => Some(Self::Right),
            // OWPML은 양쪽정렬을 JUSTIFY, 일부 문서는 BOTH로 쓴다.
            "JUSTIFY" | "BOTH" => Some(Self::Justify),
            "DISTRIBUTE" | "DISTRIBUTE_SPACE" => Some(Self::Distribute),
            _ => None,
        }
    }

    /// OfficeCLI docx `align` 속성값 (`schemas/help/docx/paragraph.json`).
    pub fn as_docx(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Center => "center",
            Self::Right => "right",
            Self::Justify => "both",
            // `officecli help docx paragraph`의 align 허용값에 distribute가 있다:
            // left, center, right, justify, both, distribute
            Self::Distribute => "distribute",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VertAlign {
    Superscript,
    Subscript,
}

/// 글자 서식. HWPX `hh:charPr` 하나에 대응한다.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CharStyle {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    /// `#RRGGBB` 형태로 정규화된 색상.
    pub color: Option<String>,
    /// 형광펜 색상.
    pub highlight: Option<String>,
    /// pt 단위 글자 크기.
    pub size_pt: Option<f64>,
    pub font: Option<String>,
    pub vert_align: Option<VertAlign>,
}

impl CharStyle {
    /// 서식이 하나도 지정되지 않았는지. emit 시 불필요한 prop을 줄이는 데 쓴다.
    pub fn is_plain(&self) -> bool {
        *self == CharStyle::default()
    }
}

/// 문단 서식. HWPX `hh:paraPr` 하나에 대응한다.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParaStyle {
    pub align: Option<Align>,
    /// 왼쪽 들여쓰기 (twip).
    pub indent_left_twip: Option<i64>,
    /// 첫 줄 들여쓰기 (twip). **양수만.**
    pub indent_first_twip: Option<i64>,
    /// 내어쓰기 (twip). **양수로 저장한다.**
    ///
    /// HWP는 내어쓰기를 `hc:intent`의 **음수**로 표현한다. docx는 별개 속성이다:
    /// `w:ind/@firstLine`은 음수를 받지 않고 `w:ind/@hanging`을 써야 한다
    /// (실측: `firstLineIndent=-500` → `<w:ind w:firstLine="-500"/>` 라는
    /// 유효하지 않은 OOXML이 만들어진다).
    ///
    /// 그래서 파싱 단계에서 부호로 갈라 둔다. 두 값은 상호 배타적이다.
    pub indent_hanging_twip: Option<i64>,
    /// 문단 위 여백 (twip).
    pub space_before_twip: Option<i64>,
    /// 문단 아래 여백 (twip).
    pub space_after_twip: Option<i64>,
    /// 줄간격 배수 (예: 1.6).
    pub line_spacing_ratio: Option<f64>,
}

impl ParaStyle {
    pub fn is_plain(&self) -> bool {
        *self == ParaStyle::default()
    }

    /// `hc:intent` 값(twip)을 부호에 따라 첫줄 들여쓰기 / 내어쓰기로 나눠 넣는다.
    pub fn set_first_line_indent(&mut self, twip: i64) {
        // 둘은 상호 배타적이므로 항상 함께 갱신한다.
        if twip < 0 {
            self.indent_hanging_twip = Some(-twip);
            self.indent_first_twip = None;
        } else if twip > 0 {
            self.indent_first_twip = Some(twip);
            self.indent_hanging_twip = None;
        } else {
            self.indent_first_twip = None;
            self.indent_hanging_twip = None;
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TextRun {
    pub text: String,
    pub style: CharStyle,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    /// `hp:img/@binaryItemIDRef`.
    pub bin_item_id: String,
    pub width_twip: Option<i64>,
    pub height_twip: Option<i64>,
    pub alt: Option<String>,
    /// BinData에서 읽어낸 원본 바이트. 반복 참조는 같은 할당을 공유한다.
    pub data: Option<Arc<[u8]>>,
    /// `image/png` 등. 파일 확장자에서 추론한다.
    pub content_type: Option<String>,
}

/// HWPX 폼 컨트롤 체크박스 (`hp:checkBtn`).
///
/// 실측(2026 대구문학관 참가신청서): 양식 문서는 체크박스를 문자(`☑`)가 아니라
/// 폼 컨트롤로 넣는다. 문자로만 취급하면 체크 안 된 상자가 통째로 사라진다.
#[derive(Debug, Clone, PartialEq)]
pub struct CheckBox {
    /// `hp:checkBtn/@name`. docx 폼필드의 안정 식별자로 쓴다.
    pub name: Option<String>,
    /// `hp:checkBtn/@value` 가 `CHECKED` 인지.
    pub checked: bool,
}

/// HWPX 누름틀 (`hp:fieldBegin type="CLICK_HERE"`).
///
/// HWP에서 "클릭해서 입력"하는 자리다. 즉 양식의 입력란이다. 한글은 클릭 전까지
/// `Direction` 안내 문구를 그 자리에 보여준다.
#[derive(Debug, Clone, PartialEq)]
pub struct TextField {
    /// `hp:fieldBegin/@name`. 실제 문서에서는 비어 있는 경우가 많다.
    pub name: Option<String>,
    /// 안내 문구. `Command` 문자열의 `Direction:wstring:<len>:<문구>`에서 뽑는다.
    ///
    /// 한글은 누름틀을 클릭하기 전까지 이 문구를 그 자리에 보여준다.
    pub hint: Option<String>,
}

impl TextField {
    /// 폼필드에 넣을 초기 텍스트. 안내 문구가 있으면 그것을 쓴다.
    pub fn initial_text(&self) -> Option<String> {
        self.hint.clone().filter(|h| !h.trim().is_empty())
    }
}

/// 문단 안의 각주/미주 참조가 가리키는 주석 종류.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteKind {
    Footnote,
    Endnote,
}

/// HWPX `hp:footNote`/`hp:endNote`와 그 `hp:subList` 본문.
///
/// 주석 본문도 문단과 표를 가질 수 있으므로 셀과 마찬가지로 블록 순서를 그대로
/// 보존한다. `number`는 표시 번호가 아니라 원본의 `number` 속성이다. DOCX 쪽
/// ID는 출력 순서에 맞춰 호스트가 새로 할당한다.
#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    pub kind: NoteKind,
    pub number: Option<usize>,
    pub instance_id: Option<String>,
    pub blocks: Vec<Block>,
}

impl Note {
    pub fn paragraphs(&self) -> impl Iterator<Item = &Paragraph> {
        self.blocks.iter().filter_map(|block| match block {
            Block::Paragraph(paragraph) => Some(paragraph),
            Block::Table(_) => None,
        })
    }

    pub fn tables(&self) -> impl Iterator<Item = &Table> {
        self.blocks.iter().filter_map(|block| match block {
            Block::Table(table) => Some(table),
            Block::Paragraph(_) => None,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Inline {
    Text(TextRun),
    Image(Image),
    CheckBox(CheckBox),
    TextField(TextField),
    Note(Note),
    /// `hp:lineBreak` — 문단 내 줄바꿈.
    LineBreak,
    /// `hp:tab`.
    Tab,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Paragraph {
    pub style: ParaStyle,
    pub inlines: Vec<Inline>,
}

impl Paragraph {
    /// 텍스트 런만 이어붙인 평문. 단일 런 병합 판단과 테스트에 쓴다.
    pub fn plain_text(&self) -> String {
        let mut out = String::new();
        for inline in &self.inlines {
            match inline {
                Inline::Text(r) => out.push_str(&r.text),
                Inline::Tab => out.push('\t'),
                Inline::LineBreak => out.push('\n'),
                Inline::Image(_) | Inline::CheckBox(_) | Inline::TextField(_) | Inline::Note(_) => {
                }
            }
        }
        out
    }

    /// 텍스트 런들이 모두 같은 서식이고 이미지가 없으면 그 서식을 돌려준다.
    ///
    /// `officecli dump`의 "단일 런 문단은 `add p` 한 줄로 병합" 규칙을 적용할 수
    /// 있는지 판정한다 (`docs/01-protocol-contract.md` C8).
    pub fn uniform_style(&self) -> Option<&CharStyle> {
        let mut found: Option<&CharStyle> = None;
        for inline in &self.inlines {
            match inline {
                Inline::Text(r) => match found {
                    None => found = Some(&r.style),
                    Some(s) if s == &r.style => {}
                    Some(_) => return None,
                },
                // 탭/줄바꿈은 text prop 안에서 문자로 표현할 수 있으므로 허용한다.
                Inline::Tab | Inline::LineBreak => {}
                // 이미지와 체크박스는 별도 자식 명령이 필요하므로 병합 불가.
                Inline::Image(_) | Inline::CheckBox(_) | Inline::TextField(_) | Inline::Note(_) => {
                    return None
                }
            }
        }
        found
    }

    pub fn has_image(&self) -> bool {
        self.inlines.iter().any(|i| matches!(i, Inline::Image(_)))
    }

    /// 별도 자식 명령이 필요한 인라인(이미지·체크박스)이 있는지.
    ///
    /// 있으면 문단을 `text` prop 한 줄로 병합할 수 없다.
    pub fn needs_child_commands(&self) -> bool {
        self.inlines.iter().any(|i| {
            matches!(
                i,
                Inline::Image(_) | Inline::CheckBox(_) | Inline::TextField(_) | Inline::Note(_)
            )
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Cell {
    /// 0-based. `hp:cellAddr/@rowAddr`.
    pub row: usize,
    /// 0-based. `hp:cellAddr/@colAddr`.
    pub col: usize,
    /// `hp:cellSpan/@rowSpan`. 최소 1.
    pub row_span: usize,
    /// `hp:cellSpan/@colSpan`. 최소 1.
    pub col_span: usize,
    /// `hp:cellSz/@width` (twip).
    pub width_twip: Option<i64>,
    /// 배경색 `#RRGGBB`.
    pub fill: Option<String>,
    /// `hp:subList` 안의 내용. 문단과 **중첩표**가 등장 순서대로 들어간다.
    ///
    /// 문단만 담지 않는 이유: HWPX 셀은 표를 다시 품을 수 있고, docx도 셀 안에
    /// 표를 넣을 수 있다(실측 확인). 순서를 잃지 않으려면 블록 목록이어야 한다.
    pub blocks: Vec<Block>,
}

impl Cell {
    /// 셀 안 문단들만 순서대로.
    pub fn paragraphs(&self) -> impl Iterator<Item = &Paragraph> {
        self.blocks.iter().filter_map(|b| match b {
            Block::Paragraph(p) => Some(p),
            Block::Table(_) => None,
        })
    }

    /// 셀 안 중첩표들만 순서대로.
    pub fn tables(&self) -> impl Iterator<Item = &Table> {
        self.blocks.iter().filter_map(|b| match b {
            Block::Table(t) => Some(t),
            Block::Paragraph(_) => None,
        })
    }

    /// 셀의 평문. 여러 문단은 개행으로 잇는다. 중첩표는 세지 않는다.
    pub fn plain_text(&self) -> String {
        self.paragraphs()
            .map(|p| p.plain_text())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Table {
    /// `hp:tbl/@rowCnt`. 없으면 셀 주소에서 유도한다.
    pub rows: usize,
    /// `hp:tbl/@colCnt`. 없으면 셀 주소에서 유도한다.
    pub cols: usize,
    /// 열 너비 (twip). 첫 행 셀들에서 유도한다.
    pub col_widths_twip: Vec<i64>,
    pub cells: Vec<Cell>,
}

impl Table {
    pub fn cell_at(&self, row: usize, col: usize) -> Option<&Cell> {
        self.cells.iter().find(|c| c.row == row && c.col == col)
    }
}

/// 열 너비를 유도한다 (twip).
///
/// HWPX는 열 너비를 따로 저장하지 않고 셀마다 `hp:cellSz/@width`만 갖는다.
/// 병합이 많은 양식 문서에서는 `colSpan == 1`인 셀만으로 모든 열을 채울 수 없다.
/// 실측(2026 대구문학관 참가신청서, 7열 15행): 단일 열 셀은 0·1·6번 열만 커버.
///
/// 그래서 병합 셀의 전체 너비를 제약으로 삼아 반복 해소한다. 매 회차마다
/// **미지 열이 가장 적은** 병합 셀을 골라 푼다.
///
/// - 미지 열이 1개면 나머지를 빼서 정확히 확정한다.
/// - 2개 이상이면 남은 너비를 균등 분배한다 (더 나은 정보가 없다).
///
/// 위 실측 문서에서는 이 방법으로 7열이 모두 정확히 풀리고, 합계가 문서의
/// 전체 폭 셀 너비와 일치한다.
///
/// 하나라도 채우지 못하면 **빈 벡터**를 돌려준다. 부분적인 `colWidths`는
/// 표를 더 망친다.
pub fn derive_col_widths(cells: &[Cell], cols: usize) -> Vec<i64> {
    if cols == 0 {
        return Vec::new();
    }
    let mut known: Vec<Option<i64>> = vec![None; cols];

    // 1단계: 단일 열 셀에서 바로 얻는다.
    for cell in cells {
        if cell.col_span.max(1) == 1 && cell.col < cols {
            if let Some(w) = cell.width_twip.filter(|w| *w > 0) {
                known[cell.col].get_or_insert(w);
            }
        }
    }

    // 2단계: 병합 셀을 제약으로 반복 해소.
    while known.iter().any(|w| w.is_none()) {
        // (미지 열 수, span) 이 작은 제약을 우선한다.
        //
        // 미지 열 수가 같으면 **span이 작은** 쪽이 낫다. 국소적인 제약일수록
        // 다른 열의 반올림 오차가 덜 섞인다. 실측 문서에서 이 동점 처리가 없으면
        // 7열 전체 병합 셀이 2열 병합 셀을 이겨 c3·c4가 862/863 대신 899가 됐다.
        let mut best: Option<(usize, usize, Vec<usize>, i64)> = None;

        for cell in cells {
            let span = cell.col_span.max(1);
            if span < 2 || cell.col >= cols {
                continue;
            }
            let Some(total) = cell.width_twip.filter(|w| *w > 0) else {
                continue;
            };
            let end = cell.col.saturating_add(span).min(cols);
            let unknown: Vec<usize> = (cell.col..end).filter(|&i| known[i].is_none()).collect();
            if unknown.is_empty() {
                continue;
            }
            let sum_known: i64 = (cell.col..end).filter_map(|i| known[i]).sum();
            let rest = total - sum_known;
            if rest <= 0 {
                continue;
            }
            let key = (unknown.len(), span);
            if best.as_ref().is_none_or(|(u, s, _, _)| key < (*u, *s)) {
                best = Some((unknown.len(), span, unknown, rest));
            }
        }

        match best {
            Some((_, _, unknown, rest)) => {
                let n = unknown.len() as i64;
                let each = (rest / n).max(1);
                for (i, col) in unknown.iter().enumerate() {
                    // 마지막 열에 나머지를 몰아 합계를 보존한다.
                    known[*col] = Some(if i + 1 == unknown.len() {
                        (rest - each * (n - 1)).max(1)
                    } else {
                        each
                    });
                }
            }
            // 더 쓸 제약이 없다.
            None => break,
        }
    }

    if known.iter().any(|w| w.is_none()) {
        return Vec::new();
    }
    known.into_iter().map(|w| w.unwrap_or(0)).collect()
}

#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    Paragraph(Paragraph),
    Table(Table),
}

/// HWPX 문서 하나 전체. 모든 섹션의 블록을 spine 순서로 이어붙인 것.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Document {
    pub blocks: Vec<Block>,
}

impl Document {
    pub fn paragraphs(&self) -> impl Iterator<Item = &Paragraph> {
        self.blocks.iter().filter_map(|b| match b {
            Block::Paragraph(p) => Some(p),
            Block::Table(_) => None,
        })
    }

    pub fn tables(&self) -> impl Iterator<Item = &Table> {
        self.blocks.iter().filter_map(|b| match b {
            Block::Table(t) => Some(t),
            Block::Paragraph(_) => None,
        })
    }

    /// 사용자 정의 영역(PUA) 문자 개수. 중첩표까지 재귀로 센다.
    ///
    /// 한글은 일부 특수문자를 PUA 코드포인트로 저장한다(실측: 2026년 외식업소
    /// 모집안내문에 `U+F0854`/`U+F0855` 한 쌍). 한컴 글꼴 밖에서는 빈 사각형으로
    /// 보인다.
    ///
    /// **매핑을 추측하지 않는다.** 해당 런에 `fontRef`가 없어 어느 글꼴의 어느
    /// 글리프인지 특정할 수 없었다. 문맥으로 짐작해 치환하면 틀릴 수 있고,
    /// 이 프로젝트에서 추측이 틀린 사례가 이미 여럿 있었다.
    /// 대신 문자는 그대로 보존하고 개수를 진단으로 보고한다.
    pub fn count_private_use_chars(&self) -> usize {
        fn walk(blocks: &[Block]) -> usize {
            let mut n = 0;
            for b in blocks {
                match b {
                    Block::Paragraph(p) => {
                        for inline in &p.inlines {
                            match inline {
                                Inline::Text(r) => {
                                    n += r.text.chars().filter(|c| is_private_use(*c)).count();
                                }
                                Inline::Note(note) => n += walk(&note.blocks),
                                _ => {}
                            }
                        }
                    }
                    Block::Table(t) => {
                        for cell in &t.cells {
                            n += walk(&cell.blocks);
                        }
                    }
                }
            }
            n
        }
        walk(&self.blocks)
    }
}

/// 유니코드 사용자 정의 영역(Private Use Area)인지.
///
/// BMP의 `U+E000..U+F8FF`, 그리고 두 보충 평면
/// `U+F0000..U+FFFFD`(Plane 15), `U+100000..U+10FFFD`(Plane 16).
pub fn is_private_use(c: char) -> bool {
    let o = c as u32;
    (0xE000..=0xF8FF).contains(&o)
        || (0xF_0000..=0xF_FFFD).contains(&o)
        || (0x10_0000..=0x10_FFFD).contains(&o)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell_w(row: usize, col: usize, cspan: usize, width_twip: i64) -> Cell {
        Cell {
            row,
            col,
            row_span: 1,
            col_span: cspan,
            width_twip: Some(width_twip),
            fill: None,
            blocks: Vec::new(),
        }
    }

    #[test]
    fn derives_col_widths_from_single_span_cells() {
        let cells = vec![cell_w(0, 0, 1, 800), cell_w(0, 1, 1, 1200)];
        assert_eq!(derive_col_widths(&cells, 2), vec![800, 1200]);
    }

    #[test]
    fn derives_remaining_column_by_subtracting_from_a_merged_cell() {
        // 2열 표: c0은 알고, c0..c1 병합 셀 전체 폭이 2000 → c1 = 1200
        let cells = vec![cell_w(1, 0, 1, 800), cell_w(0, 0, 2, 2000)];
        assert_eq!(derive_col_widths(&cells, 2), vec![800, 1200]);
    }

    #[test]
    fn splits_evenly_when_several_columns_are_unknown() {
        // c0만 알고 c1..c2가 미지: 병합 셀 3000 - 800 = 2200을 둘로 나눈다
        let cells = vec![cell_w(1, 0, 1, 800), cell_w(0, 0, 3, 3000)];
        let w = derive_col_widths(&cells, 3);
        assert_eq!(w[0], 800);
        assert_eq!(w[1] + w[2], 2200, "the remainder must be preserved");
    }

    #[test]
    fn derives_real_form_document_widths() {
        // 실측: 2026 대구문학관 참가신청서 표1 (7열 15행).
        // 단일 열 셀은 0·1·6번 열만 커버하므로 병합 제약을 반드시 써야 한다.
        // 값은 원본 HWPUNIT을 twip으로 환산(÷5)한 것.
        let cells = vec![
            cell_w(0, 0, 7, 48123 / 5),
            cell_w(1, 0, 3, 25376 / 5),
            cell_w(1, 3, 2, 8628 / 5),
            cell_w(1, 5, 2, 13758 / 5),
            cell_w(2, 0, 1, 7962 / 5),
            cell_w(2, 1, 6, 40161 / 5),
            cell_w(3, 1, 1, 6718 / 5),
            cell_w(3, 6, 1, 11289 / 5),
        ];
        let w = derive_col_widths(&cells, 7);
        assert_eq!(w.len(), 7, "all 7 columns must be resolved");
        assert!(w.iter().all(|v| *v > 0), "no zero widths: {w:?}");
        // 알려진 열은 정확히 나와야 한다.
        assert_eq!(w[0], 7962 / 5);
        assert_eq!(w[1], 6718 / 5);
        assert_eq!(w[6], 11289 / 5);
        // 합계가 문서의 전체 폭 셀(47762 HWPUNIT)과 근접해야 한다.
        let total: i64 = w.iter().sum();
        let expected = 47762 / 5;
        assert!(
            (total - expected).abs() <= 7,
            "sum {total} should be near {expected}, got {w:?}"
        );
    }

    #[test]
    fn gives_up_entirely_when_a_column_cannot_be_resolved() {
        // 부분적인 colWidths는 표를 더 망친다. 전부 못 채우면 빈 벡터.
        let cells = vec![cell_w(0, 0, 1, 800)];
        assert!(derive_col_widths(&cells, 3).is_empty());
        assert!(derive_col_widths(&[], 2).is_empty());
    }

    #[test]
    fn hwpunit_converts_to_twip_by_factor_five() {
        // 7200 hwpunit = 1 inch = 1440 twip
        assert_eq!(hwpunit_to_twip(7200), 1440);
        assert_eq!(hwpunit_to_twip(1000), 200);
        assert_eq!(hwpunit_to_twip(0), 0);
    }

    #[test]
    fn hwpunit_converts_to_point_by_factor_hundred() {
        // 1000 hwpunit = 10pt (unhwp styles.rs:114 "1 point = 100 hwpunit")
        assert_eq!(hwpunit_to_point(1000), 10.0);
        assert_eq!(hwpunit_to_point(1200), 12.0);
    }

    #[test]
    fn detects_private_use_area_codepoints() {
        // 실측: 2026년 외식업소 모집안내문에 U+F0854/U+F0855 한 쌍.
        // BMP PUA
        assert!(is_private_use('\u{E000}'));
        assert!(is_private_use('\u{F8FF}'));
        // Plane 15 (한글이 쓰는 영역)
        assert!(is_private_use('\u{F0854}'));
        assert!(is_private_use('\u{F0855}'));
        // Plane 16
        assert!(is_private_use('\u{100000}'));
        // 일반 문자는 아니다
        assert!(!is_private_use('가'));
        assert!(!is_private_use('『'));
        assert!(!is_private_use('A'));
        assert!(!is_private_use('☑'));
    }

    #[test]
    fn counts_private_use_chars_including_nested_tables() {
        let pua = |t: &str| {
            Block::Paragraph(Paragraph {
                style: ParaStyle::default(),
                inlines: vec![Inline::Text(TextRun {
                    text: t.into(),
                    style: CharStyle::default(),
                })],
            })
        };
        let inner = Table {
            rows: 1,
            cols: 1,
            col_widths_twip: vec![],
            cells: vec![Cell {
                row: 0,
                col: 0,
                row_span: 1,
                col_span: 1,
                width_twip: None,
                fill: None,
                blocks: vec![pua("중첩\u{F0856}")],
            }],
        };
        let outer = Table {
            rows: 1,
            cols: 1,
            col_widths_twip: vec![],
            cells: vec![Cell {
                row: 0,
                col: 0,
                row_span: 1,
                col_span: 1,
                width_twip: None,
                fill: None,
                blocks: vec![pua("셀\u{F0855}"), Block::Table(inner)],
            }],
        };
        let doc = Document {
            blocks: vec![pua("본문\u{F0854}이후"), Block::Table(outer)],
        };
        // 본문 1 + 셀 1 + 중첩 1
        assert_eq!(doc.count_private_use_chars(), 3);

        // PUA가 없으면 0
        let clean = Document {
            blocks: vec![pua("깨끗한 텍스트 『인용』")],
        };
        assert_eq!(clean.count_private_use_chars(), 0);
    }

    #[test]
    fn negative_first_line_indent_becomes_hanging() {
        // HWP는 내어쓰기를 hc:intent의 음수로 표현한다. docx는 별개 속성이다.
        let mut s = ParaStyle::default();
        s.set_first_line_indent(-1714);
        assert_eq!(s.indent_hanging_twip, Some(1714), "음수는 내어쓰기로");
        assert_eq!(s.indent_first_twip, None);

        s.set_first_line_indent(400);
        assert_eq!(s.indent_first_twip, Some(400));
        assert_eq!(s.indent_hanging_twip, None, "갱신 시 반대쪽은 비워야 한다");

        s.set_first_line_indent(0);
        assert_eq!(s.indent_first_twip, None);
        assert_eq!(s.indent_hanging_twip, None);
    }

    #[test]
    fn align_parses_owpml_case_insensitively() {
        assert_eq!(Align::from_owpml("LEFT"), Some(Align::Left));
        assert_eq!(Align::from_owpml("center"), Some(Align::Center));
        assert_eq!(Align::from_owpml("JUSTIFY"), Some(Align::Justify));
        assert_eq!(Align::from_owpml("both"), Some(Align::Justify));
        assert_eq!(Align::from_owpml("nonsense"), None);
    }

    #[test]
    fn justify_maps_to_docx_both() {
        assert_eq!(Align::Justify.as_docx(), "both");
        assert_eq!(Align::Center.as_docx(), "center");
    }

    #[test]
    fn uniform_style_detects_mixed_runs() {
        let plain = CharStyle::default();
        let bold = CharStyle {
            bold: true,
            ..Default::default()
        };

        let same = Paragraph {
            style: ParaStyle::default(),
            inlines: vec![
                Inline::Text(TextRun {
                    text: "가".into(),
                    style: plain.clone(),
                }),
                Inline::Text(TextRun {
                    text: "나".into(),
                    style: plain.clone(),
                }),
            ],
        };
        assert_eq!(same.uniform_style(), Some(&plain));

        let mixed = Paragraph {
            style: ParaStyle::default(),
            inlines: vec![
                Inline::Text(TextRun {
                    text: "가".into(),
                    style: plain,
                }),
                Inline::Text(TextRun {
                    text: "나".into(),
                    style: bold,
                }),
            ],
        };
        assert_eq!(mixed.uniform_style(), None);
    }

    #[test]
    fn paragraph_with_image_is_never_uniform() {
        let p = Paragraph {
            style: ParaStyle::default(),
            inlines: vec![
                Inline::Text(TextRun {
                    text: "그림:".into(),
                    style: CharStyle::default(),
                }),
                Inline::Image(Image {
                    bin_item_id: "image1".into(),
                    width_twip: None,
                    height_twip: None,
                    alt: None,
                    data: None,
                    content_type: None,
                }),
            ],
        };
        assert_eq!(p.uniform_style(), None);
        assert!(p.has_image());
    }

    #[test]
    fn plain_text_renders_tab_and_linebreak() {
        let p = Paragraph {
            style: ParaStyle::default(),
            inlines: vec![
                Inline::Text(TextRun {
                    text: "가".into(),
                    style: CharStyle::default(),
                }),
                Inline::Tab,
                Inline::Text(TextRun {
                    text: "나".into(),
                    style: CharStyle::default(),
                }),
                Inline::LineBreak,
                Inline::Text(TextRun {
                    text: "다".into(),
                    style: CharStyle::default(),
                }),
            ],
        };
        assert_eq!(p.plain_text(), "가\t나\n다");
    }
}
