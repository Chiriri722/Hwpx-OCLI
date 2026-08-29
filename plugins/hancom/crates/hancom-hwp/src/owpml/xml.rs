//! quick-xml 공용 헬퍼.
//!
//! OWPML은 네임스페이스 접두사(`hp:`, `hh:`, `hs:`)를 쓰지만 문서마다 접두사가
//! 다를 수 있다. 따라서 접두사를 무시하고 local name으로만 비교한다.

use quick_xml::events::BytesStart;

use crate::error::Result;

/// `hp:run` → `run`. 접두사를 떼고 소유 String으로 돌려준다.
pub fn local_name(raw: &[u8]) -> String {
    let s = String::from_utf8_lossy(raw);
    match s.rsplit_once(':') {
        Some((_, local)) => local.to_string(),
        None => s.into_owned(),
    }
}

/// 속성값을 local name으로 찾는다. 접두사가 붙어 있어도(`hp:id`) 매칭된다.
///
/// 값의 XML 엔티티는 해제한다. `alt="A &amp;amp; B"` → `A & B`.
/// 원시 바이트를 그대로 쓰면 엔티티가 문자열에 남는다.
pub fn attr(e: &BytesStart<'_>, name: &str) -> Option<String> {
    for a in e.attributes().with_checks(false) {
        let a = a.ok()?;
        if local_name(a.key.as_ref()) == name {
            // HWPX 문서 선언은 `version="1.0"`이다.
            return Some(
                match a.normalized_value(quick_xml::XmlVersion::Explicit1_0) {
                    Ok(v) => v.into_owned(),
                    // 해제 실패 시에도 값을 버리지 않는다.
                    Err(_) => String::from_utf8_lossy(a.value.as_ref()).into_owned(),
                },
            );
        }
    }
    None
}

/// 속성을 usize로 읽는다.
pub fn attr_usize(e: &BytesStart<'_>, name: &str) -> Option<usize> {
    attr(e, name)?.trim().parse().ok()
}

/// 속성을 i64로 읽는다. 소수 표기도 받아준다.
pub fn attr_i64(e: &BytesStart<'_>, name: &str) -> Option<i64> {
    let raw = attr(e, name)?;
    let t = raw.trim();
    t.parse::<i64>()
        .ok()
        .or_else(|| t.parse::<f64>().ok().map(|f| f.round() as i64))
}

/// `&amp;` 또는 `&#48;`처럼 quick-xml이 별도 이벤트로 내는 엔티티를
/// 내용 손실 없이 문자열로 바꾼다. 알 수 없는 이름은 원문 형태로 보존한다.
pub fn resolve_entity(reference: &quick_xml::events::BytesRef<'_>) -> Result<String> {
    if let Ok(Some(character)) = reference.resolve_char_ref() {
        return Ok(character.to_string());
    }
    let name = reference.decode()?;
    Ok(match quick_xml::escape::resolve_predefined_entity(&name) {
        Some(value) => value.to_string(),
        None => format!("&{name};"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use quick_xml::events::Event;
    use quick_xml::Reader;

    fn first_start(xml: &str) -> BytesStart<'static> {
        let mut r = Reader::from_str(xml);
        r.config_mut().expand_empty_elements = true;
        let mut buf = Vec::new();
        loop {
            match r.read_event_into(&mut buf).expect("valid xml") {
                Event::Start(e) => return e.into_owned(),
                Event::Eof => panic!("no start element"),
                _ => {}
            }
            buf.clear();
        }
    }

    #[test]
    fn strips_namespace_prefix() {
        assert_eq!(local_name(b"hp:run"), "run");
        assert_eq!(local_name(b"run"), "run");
        assert_eq!(local_name(b"hs:sec"), "sec");
    }

    #[test]
    fn finds_attribute_regardless_of_prefix() {
        let e = first_start(r#"<hp:p id="7" hp:paraPrIDRef="3"/>"#);
        assert_eq!(attr(&e, "id").as_deref(), Some("7"));
        assert_eq!(attr(&e, "paraPrIDRef").as_deref(), Some("3"));
        assert_eq!(attr(&e, "nope"), None);
    }

    #[test]
    fn parses_numeric_attributes() {
        let e = first_start(r#"<hp:cellSz width="4000" height="1000.0"/>"#);
        assert_eq!(attr_i64(&e, "width"), Some(4000));
        assert_eq!(attr_i64(&e, "height"), Some(1000));
        assert_eq!(attr_usize(&e, "width"), Some(4000));
    }

    #[test]
    fn preserves_korean_attribute_values() {
        let e = first_start(r#"<hh:fontRef hangul="함초롬바탕"/>"#);
        assert_eq!(attr(&e, "hangul").as_deref(), Some("함초롬바탕"));
    }

    #[test]
    fn unescapes_entities_in_attribute_values() {
        // 회귀 테스트: 엔티티를 해제하지 않으면 `&amp;`가 문자열에 남는다.
        let e = first_start(r#"<hp:img alt="A &amp; B &lt;C&gt;"/>"#);
        assert_eq!(attr(&e, "alt").as_deref(), Some("A & B <C>"));
    }
}
