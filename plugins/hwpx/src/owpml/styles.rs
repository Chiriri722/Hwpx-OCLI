//! `Contents/header.xml`의 글자모양(`hh:charPr`) / 문단모양(`hh:paraPr`) 표.
//!
//! 본문의 `hp:run/@charPrIDRef`와 `hp:p/@paraPrIDRef`가 이 표를 가리킨다.
//! 요소·속성 이름 근거: `unhwp-0.7.0/src/hwpx/styles.rs`.

use std::collections::HashMap;

use quick_xml::events::Event;
use quick_xml::Reader;

use super::model::{hwpunit_to_point, hwpunit_to_twip, Align, CharStyle, ParaStyle, VertAlign};
use super::xml::{attr, local_name};
use crate::error::Result;

#[derive(Debug, Default)]
pub struct StyleTable {
    pub char_styles: HashMap<String, CharStyle>,
    pub para_styles: HashMap<String, ParaStyle>,
    /// `hh:fontfaces` 의 폰트 이름표. `hh:font/@id` → `@face`.
    ///
    /// `hh:fontRef/@hangul` 은 **폰트 이름이 아니라 이 표의 인덱스**다.
    /// 실측: `<hh:fontRef hangul="0" latin="0" .../>` 이고 실제 이름은
    /// `<hh:fontfaces><hh:fontface lang="HANGUL"><hh:font id="0" face="한컴바탕"/>`
    /// 에 있다. 이 표 없이 `@hangul` 을 그대로 쓰면 `font: "0"` 같은 값이 나간다.
    pub fonts: HashMap<String, String>,
}

impl StyleTable {
    pub fn char_style(&self, id: Option<&str>) -> CharStyle {
        id.and_then(|i| self.char_styles.get(i))
            .cloned()
            .unwrap_or_default()
    }

    pub fn para_style(&self, id: Option<&str>) -> ParaStyle {
        id.and_then(|i| self.para_styles.get(i))
            .cloned()
            .unwrap_or_default()
    }

    /// header.xml을 파싱한다.
    ///
    /// header.xml이 없거나 깨져도 실패로 보지 않는다. 서식 없이 텍스트만
    /// 뽑는 것이 아무것도 못 하는 것보다 낫다.
    pub fn parse(xml: &str) -> Result<Self> {
        // 폰트 표를 먼저 훑는다. `hh:fontfaces`가 `hh:charProperties`보다 앞에
        // 오지만 순서를 가정하지 않는다.
        let font_names = parse_font_table(xml);

        let mut table = StyleTable {
            fonts: font_names.clone(),
            ..Default::default()
        };
        let mut reader = Reader::from_str(xml);
        let config = reader.config_mut();
        config.trim_text(false);
        // 자기닫힘 태그(`<hh:charPr .../>`)를 Start+End 쌍으로 펼친다.
        // 그래야 charPr가 자식 없이 닫혀도 End 처리에서 표에 등록된다.
        config.expand_empty_elements = true;

        // 현재 열려 있는 charPr / paraPr
        let mut cur_char: Option<(String, CharStyle)> = None;
        let mut cur_para: Option<(String, ParaStyle)> = None;
        let mut buf = Vec::new();

        loop {
            match reader.read_event_into(&mut buf) {
                Err(_) => break, // 손상된 header는 무시하고 모은 것까지 쓴다.
                Ok(Event::Eof) => break,
                Ok(Event::Start(e)) => {
                    let name_owned = e.name();
                    let name = local_name(name_owned.as_ref());
                    match name.as_str() {
                        "charPr" => {
                            let id = attr(&e, "id").unwrap_or_default();
                            let mut style = CharStyle::default();
                            // charPr 자신의 속성에서 크기/색을 읽는다.
                            if let Some(h) = attr(&e, "height").and_then(|v| parse_i64(&v)) {
                                style.size_pt = Some(hwpunit_to_point(h));
                            }
                            if let Some(c) = attr(&e, "textColor").and_then(normalize_color) {
                                style.color = Some(c);
                            }
                            if let Some(c) = attr(&e, "shadeColor").and_then(normalize_color) {
                                style.highlight = Some(c);
                            }
                            cur_char = Some((id, style));
                        }
                        "paraPr" => {
                            let id = attr(&e, "id").unwrap_or_default();
                            cur_para = Some((id, ParaStyle::default()));
                        }
                        // ── charPr 자식들 ──
                        "bold" if cur_char.is_some() => {
                            set_char(&mut cur_char, |s| s.bold = true);
                        }
                        "italic" if cur_char.is_some() => {
                            set_char(&mut cur_char, |s| s.italic = true);
                        }
                        "underline" if cur_char.is_some() => {
                            // type="NONE"이면 밑줄 없음.
                            let ty = attr(&e, "type").unwrap_or_else(|| "BOTTOM".into());
                            let on = !ty.eq_ignore_ascii_case("NONE");
                            set_char(&mut cur_char, move |s| s.underline = on);
                        }
                        "strikeout" if cur_char.is_some() => {
                            // 실제 문서는 `type`이 아니라 `shape`를 쓴다.
                            // 실측: `<hh:strikeout shape="NONE" color="#000000"/>`
                            // 5개 문서 241개 charPr 전부 `shape`만 갖고 `type`은 없다.
                            //
                            // `type`을 읽고 없으면 켜는 기본값을 주면 모든 글자에
                            // 취소선이 붙는다. 실제로 그랬다(152건 오적용).
                            //
                            // 둘 다 받아주되, **아무 것도 없으면 끈 것으로 본다.**
                            // 취소선은 드문 서식이므로 모호할 때 끄는 쪽이 안전하다.
                            let raw = attr(&e, "shape").or_else(|| attr(&e, "type"));
                            let on = raw.is_some_and(|v| {
                                !v.eq_ignore_ascii_case("NONE") && !v.trim().is_empty()
                            });
                            set_char(&mut cur_char, move |s| s.strike = on);
                        }
                        "supscript" | "superscript" if cur_char.is_some() => {
                            set_char(&mut cur_char, |s| {
                                s.vert_align = Some(VertAlign::Superscript)
                            });
                        }
                        "subscript" if cur_char.is_some() => {
                            set_char(&mut cur_char, |s| {
                                s.vert_align = Some(VertAlign::Subscript)
                            });
                        }
                        "fontRef" if cur_char.is_some() => {
                            // 한글 폰트를 우선하고 없으면 라틴을 쓴다.
                            let raw = attr(&e, "hangul").or_else(|| attr(&e, "latin"));
                            if let Some(r) = raw.filter(|s| !s.trim().is_empty()) {
                                // 실제 문서에서 이 값은 폰트 표의 **인덱스**다.
                                // 표에 있으면 이름으로 바꾸고, 없으면 이름 자체로 본다
                                // (일부 생성기는 이름을 직접 쓴다).
                                let resolved = font_names.get(&r).cloned();
                                let name = match resolved {
                                    Some(n) => Some(n),
                                    // 순수 숫자인데 표에 없으면 폰트 정보를 버린다.
                                    // `font: "14"` 같은 값을 내보내는 것보다 낫다.
                                    None if r.chars().all(|c| c.is_ascii_digit()) => None,
                                    None => Some(r),
                                };
                                if let Some(n) = name {
                                    set_char(&mut cur_char, move |s| s.font = Some(n));
                                }
                            }
                        }
                        // ── paraPr 자식들 ──
                        "align" if cur_para.is_some() => {
                            if let Some(a) =
                                attr(&e, "horizontal").as_deref().and_then(Align::from_owpml)
                            {
                                set_para(&mut cur_para, move |s| s.align = Some(a));
                            }
                        }
                        "margin" if cur_para.is_some() => { /* 자식 intent/left/prev/next에서 읽는다 */ }
                        "intent" if cur_para.is_some() => {
                            if let Some(v) = attr(&e, "value").and_then(|v| parse_i64(&v)) {
                                let t = hwpunit_to_twip(v);
                                // 음수는 내어쓰기다. docx는 별개 속성을 쓴다.
                                set_para(&mut cur_para, move |s| s.set_first_line_indent(t));
                            }
                        }
                        "left" if cur_para.is_some() => {
                            if let Some(v) = attr(&e, "value").and_then(|v| parse_i64(&v)) {
                                let t = hwpunit_to_twip(v);
                                set_para(&mut cur_para, move |s| s.indent_left_twip = Some(t));
                            }
                        }
                        "prev" if cur_para.is_some() => {
                            if let Some(v) = attr(&e, "value").and_then(|v| parse_i64(&v)) {
                                let t = hwpunit_to_twip(v);
                                set_para(&mut cur_para, move |s| s.space_before_twip = Some(t));
                            }
                        }
                        "next" if cur_para.is_some() => {
                            if let Some(v) = attr(&e, "value").and_then(|v| parse_i64(&v)) {
                                let t = hwpunit_to_twip(v);
                                set_para(&mut cur_para, move |s| s.space_after_twip = Some(t));
                            }
                        }
                        "lineSpacing" if cur_para.is_some() => {
                            // type="PERCENT" value="160" → 1.6배
                            let ty = attr(&e, "type").unwrap_or_else(|| "PERCENT".into());
                            if ty.eq_ignore_ascii_case("PERCENT") {
                                if let Some(v) = attr(&e, "value").and_then(|v| parse_i64(&v)) {
                                    if v > 0 {
                                        let ratio = v as f64 / 100.0;
                                        set_para(&mut cur_para, move |s| {
                                            s.line_spacing_ratio = Some(ratio)
                                        });
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
                Ok(Event::End(e)) => {
                    let name_owned = e.name();
                    match local_name(name_owned.as_ref()).as_str() {
                        "charPr" => {
                            if let Some((id, style)) = cur_char.take() {
                                table.char_styles.insert(id, style);
                            }
                        }
                        "paraPr" => {
                            if let Some((id, style)) = cur_para.take() {
                                table.para_styles.insert(id, style);
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
            buf.clear();
        }

        // 자기닫힘 태그로 끝난 경우를 위해 남은 것을 흘려보낸다.
        if let Some((id, style)) = cur_char.take() {
            table.char_styles.insert(id, style);
        }
        if let Some((id, style)) = cur_para.take() {
            table.para_styles.insert(id, style);
        }

        Ok(table)
    }
}

/// `hh:fontfaces` 에서 `hh:font/@id` → `@face` 표를 만든다.
///
/// 여러 `hh:fontface`(HANGUL/LATIN/HANJA/…)가 각각 같은 id 공간을 쓴다.
/// `hh:fontRef/@hangul` 이 우리가 우선하는 값이므로 **HANGUL을 먼저** 등록하고,
/// 나머지 언어는 빈 자리만 채운다.
fn parse_font_table(xml: &str) -> HashMap<String, String> {
    let mut hangul: HashMap<String, String> = HashMap::new();
    let mut other: HashMap<String, String> = HashMap::new();

    let mut reader = Reader::from_str(xml);
    let config = reader.config_mut();
    config.trim_text(false);
    config.expand_empty_elements = true;

    let mut in_hangul = false;
    let mut depth_in_faces = false;
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Err(_) | Ok(Event::Eof) => break,
            Ok(Event::Start(e)) => {
                let owned = e.name();
                match local_name(owned.as_ref()).as_str() {
                    "fontfaces" => depth_in_faces = true,
                    "fontface" => {
                        in_hangul = attr(&e, "lang")
                            .is_some_and(|l| l.eq_ignore_ascii_case("HANGUL"));
                    }
                    "font" if depth_in_faces => {
                        if let (Some(id), Some(face)) = (attr(&e, "id"), attr(&e, "face")) {
                            let face = face.trim().to_string();
                            if !face.is_empty() {
                                if in_hangul {
                                    hangul.insert(id, face);
                                } else {
                                    other.entry(id).or_insert(face);
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                let owned = e.name();
                if local_name(owned.as_ref()) == "fontfaces" {
                    depth_in_faces = false;
                }
            }
            _ => {}
        }
        buf.clear();
    }

    // HANGUL이 이기고, 없는 id만 다른 언어로 채운다.
    for (k, v) in other {
        hangul.entry(k).or_insert(v);
    }
    hangul
}

fn set_char(slot: &mut Option<(String, CharStyle)>, f: impl FnOnce(&mut CharStyle)) {
    if let Some((_, s)) = slot.as_mut() {
        f(s);
    }
}

fn set_para(slot: &mut Option<(String, ParaStyle)>, f: impl FnOnce(&mut ParaStyle)) {
    if let Some((_, s)) = slot.as_mut() {
        f(s);
    }
}

fn parse_i64(s: &str) -> Option<i64> {
    s.trim().parse::<i64>().ok().or_else(|| {
        // "1000.0" 같은 값도 받아준다.
        s.trim().parse::<f64>().ok().map(|f| f.round() as i64)
    })
}

/// HWPX 색상값을 `#RRGGBB`로 정규화한다.
///
/// `#RRGGBB`, `RRGGBB`, 그리고 일부 문서가 쓰는 10진 정수 표기를 받는다.
/// `none`은 색 없음으로 본다.
pub fn normalize_color(raw: String) -> Option<String> {
    let s = raw.trim();
    if s.is_empty() || s.eq_ignore_ascii_case("none") {
        return None;
    }
    let hex = s.strip_prefix('#').unwrap_or(s);
    if hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Some(format!("#{}", hex.to_ascii_uppercase()));
    }
    if hex.len() == 8 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
        // AARRGGBB로 보고 알파를 버린다.
        return Some(format!("#{}", hex[2..].to_ascii_uppercase()));
    }
    // 10진 정수 표기 (일부 생성기).
    if let Ok(v) = s.parse::<u32>() {
        return Some(format!("#{:06X}", v & 0x00FF_FFFF));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_color_forms() {
        assert_eq!(normalize_color("#ff0000".into()).as_deref(), Some("#FF0000"));
        assert_eq!(normalize_color("00FF00".into()).as_deref(), Some("#00FF00"));
        assert_eq!(normalize_color("none".into()), None);
        assert_eq!(normalize_color("".into()), None);
        // AARRGGBB → 알파 제거
        assert_eq!(
            normalize_color("FF123456".into()).as_deref(),
            Some("#123456")
        );
        // 10진
        assert_eq!(normalize_color("255".into()).as_deref(), Some("#0000FF"));
    }

    #[test]
    fn parses_char_properties() {
        let xml = r##"<hh:head xmlns:hh="http://www.hancom.co.kr/hwpml/2011/head">
          <hh:charProperties itemCnt="2">
            <hh:charPr id="0" height="1000" textColor="#000000">
              <hh:fontRef hangul="함초롬바탕" latin="Times New Roman"/>
            </hh:charPr>
            <hh:charPr id="1" height="1200" textColor="#FF0000">
              <hh:bold/>
              <hh:italic/>
              <hh:underline type="BOTTOM"/>
            </hh:charPr>
          </hh:charProperties>
        </hh:head>"##;

        let t = StyleTable::parse(xml).expect("parses");
        let s0 = t.char_style(Some("0"));
        assert_eq!(s0.size_pt, Some(10.0));
        assert_eq!(s0.color.as_deref(), Some("#000000"));
        assert_eq!(s0.font.as_deref(), Some("함초롬바탕"));
        assert!(!s0.bold);

        let s1 = t.char_style(Some("1"));
        assert_eq!(s1.size_pt, Some(12.0));
        assert_eq!(s1.color.as_deref(), Some("#FF0000"));
        assert!(s1.bold);
        assert!(s1.italic);
        assert!(s1.underline);
    }

    #[test]
    fn strikeout_reads_shape_not_type() {
        // 실측 회귀 테스트. 실제 문서는 `shape`만 쓴다:
        //   <hh:strikeout shape="NONE" color="#000000"/>
        // `type`을 읽고 없으면 켜는 기본값을 주면 모든 글자에 취소선이 붙는다
        // (실제로 5개 문서에서 152건 오적용됐다).
        let xml = r##"<hh:charProperties>
            <hh:charPr id="0"><hh:strikeout shape="NONE" color="#000000"/></hh:charPr>
            <hh:charPr id="1"><hh:strikeout shape="SOLID" color="#000000"/></hh:charPr>
            <hh:charPr id="2"><hh:strikeout/></hh:charPr>
            <hh:charPr id="3"><hh:strikeout type="SINGLE"/></hh:charPr>
        </hh:charProperties>"##;
        let t = StyleTable::parse(xml).expect("parses");
        assert!(!t.char_style(Some("0")).strike, "shape=NONE must be off");
        assert!(t.char_style(Some("1")).strike, "shape=SOLID must be on");
        assert!(
            !t.char_style(Some("2")).strike,
            "no attribute at all must default to off"
        );
        assert!(t.char_style(Some("3")).strike, "legacy type= must still work");
    }

    #[test]
    fn font_ref_resolves_through_the_font_table() {
        // 실측: `hh:fontRef/@hangul` 은 이름이 아니라 폰트 표의 인덱스다.
        let xml = r##"<hh:head>
            <hh:fontfaces itemCnt="2">
              <hh:fontface lang="HANGUL" fontCnt="2">
                <hh:font id="0" face="한컴바탕" type="TTF"/>
                <hh:font id="3" face="맑은 고딕" type="TTF"/>
              </hh:fontface>
              <hh:fontface lang="LATIN" fontCnt="1">
                <hh:font id="0" face="Times New Roman" type="TTF"/>
                <hh:font id="9" face="Arial" type="TTF"/>
              </hh:fontface>
            </hh:fontfaces>
            <hh:charProperties>
              <hh:charPr id="0"><hh:fontRef hangul="0" latin="0"/></hh:charPr>
              <hh:charPr id="1"><hh:fontRef hangul="3" latin="3"/></hh:charPr>
              <hh:charPr id="2"><hh:fontRef hangul="9" latin="9"/></hh:charPr>
              <hh:charPr id="3"><hh:fontRef hangul="99" latin="99"/></hh:charPr>
            </hh:charProperties>
        </hh:head>"##;
        let t = StyleTable::parse(xml).expect("parses");
        // HANGUL 표가 이긴다.
        assert_eq!(t.char_style(Some("0")).font.as_deref(), Some("한컴바탕"));
        assert_eq!(t.char_style(Some("1")).font.as_deref(), Some("맑은 고딕"));
        // HANGUL에 없는 id는 다른 언어 표로 채운다.
        assert_eq!(t.char_style(Some("2")).font.as_deref(), Some("Arial"));
        // 표에 없는 숫자는 버린다. `font: "99"` 를 내보내면 안 된다.
        assert_eq!(t.char_style(Some("3")).font, None);
    }

    #[test]
    fn font_ref_accepts_a_literal_name_when_not_an_index() {
        // 일부 생성기는 이름을 직접 쓴다. 그 경우도 받아준다.
        let xml = r##"<hh:charProperties>
            <hh:charPr id="0"><hh:fontRef hangul="함초롬바탕"/></hh:charPr>
        </hh:charProperties>"##;
        let t = StyleTable::parse(xml).expect("parses");
        assert_eq!(t.char_style(Some("0")).font.as_deref(), Some("함초롬바탕"));
    }

    #[test]
    fn underline_type_none_means_no_underline() {
        let xml = r##"<hh:charProperties>
            <hh:charPr id="0"><hh:underline type="NONE"/></hh:charPr>
        </hh:charProperties>"##;
        let t = StyleTable::parse(xml).expect("parses");
        assert!(!t.char_style(Some("0")).underline);
    }

    #[test]
    fn parses_para_properties() {
        let xml = r##"<hh:paraProperties>
            <hh:paraPr id="0">
              <hh:align horizontal="CENTER" vertical="BASELINE"/>
              <hh:margin>
                <hh:intent value="1000"/>
                <hh:left value="2000"/>
                <hh:prev value="500"/>
                <hh:next value="600"/>
              </hh:margin>
              <hh:lineSpacing type="PERCENT" value="160"/>
            </hh:paraPr>
        </hh:paraProperties>"##;

        let t = StyleTable::parse(xml).expect("parses");
        let p = t.para_style(Some("0"));
        assert_eq!(p.align, Some(Align::Center));
        assert_eq!(p.indent_first_twip, Some(200)); // 1000 hwpunit / 5
        assert_eq!(p.indent_left_twip, Some(400));
        assert_eq!(p.space_before_twip, Some(100));
        assert_eq!(p.space_after_twip, Some(120));
        assert_eq!(p.line_spacing_ratio, Some(1.6));
    }

    #[test]
    fn parses_hanging_indent_from_negative_intent() {
        // 실측 형식: margin 자식이 hc: 네임스페이스이고 hp:switch로 두 번 나온다.
        let xml = r##"<hh:paraProperties>
          <hh:paraPr id="0">
            <hh:align horizontal="JUSTIFY" vertical="BASELINE"/>
            <hp:switch>
              <hp:case hp:required-namespace="http://www.hancom.co.kr/hwpml/2016/HwpUnitChar">
                <hh:margin>
                  <hc:intent value="-8570" unit="HWPUNIT"/>
                  <hc:left value="8570" unit="HWPUNIT"/>
                </hh:margin>
              </hp:case>
              <hp:default>
                <hh:margin>
                  <hc:intent value="-8570" unit="HWPUNIT"/>
                  <hc:left value="8570" unit="HWPUNIT"/>
                </hh:margin>
              </hp:default>
            </hp:switch>
          </hh:paraPr>
        </hh:paraProperties>"##;
        let t = StyleTable::parse(xml).expect("parses");
        let p = t.para_style(Some("0"));
        // -8570 HWPUNIT / 5 = -1714 twip → 내어쓰기 1714
        assert_eq!(p.indent_hanging_twip, Some(1714));
        assert_eq!(p.indent_first_twip, None);
        assert_eq!(p.indent_left_twip, Some(1714));
    }

    #[test]
    fn unknown_id_yields_default_style() {
        let t = StyleTable::default();
        assert!(t.char_style(Some("999")).is_plain());
        assert!(t.para_style(None).is_plain());
    }

    #[test]
    fn tolerates_malformed_header() {
        // 잘라먹힌 XML이어도 패닉 없이 지금까지 모은 것을 돌려준다.
        let xml = r##"<hh:charProperties><hh:charPr id="0" height="900"><hh:bold/>"##;
        let t = StyleTable::parse(xml).expect("must not fail hard");
        let s = t.char_style(Some("0"));
        assert_eq!(s.size_pt, Some(9.0));
        assert!(s.bold);
    }
}
