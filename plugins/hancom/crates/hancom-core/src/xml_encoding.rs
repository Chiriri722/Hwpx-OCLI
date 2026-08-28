//! Bounded XML byte decoding shared by Hancom container readers.

use crate::error::{PluginError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XmlEncoding {
    Utf8,
    Utf16Le,
    Utf16Be,
}

pub struct DecodedXml {
    pub text: String,
    pub encoding: XmlEncoding,
}

/// Decode one complete XML byte stream. A BOM selects UTF-8/UTF-16LE/UTF-16BE;
/// BOM-less XML is required to be UTF-8.
pub fn decode_xml(bytes: &[u8], max_bytes: u64) -> Result<DecodedXml> {
    let input_len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if input_len > max_bytes {
        return Err(limit_error(format!(
            "XML input has {input_len} bytes (maximum {max_bytes})"
        )));
    }

    let (encoding, offset) = bom_encoding(bytes).unwrap_or((XmlEncoding::Utf8, 0));
    let text = match encoding {
        XmlEncoding::Utf8 => std::str::from_utf8(&bytes[offset..])
            .map_err(|error| PluginError::corrupt(format!("XML is not valid UTF-8: {error}")))?
            .to_owned(),
        XmlEncoding::Utf16Le | XmlEncoding::Utf16Be => decode_utf16(&bytes[offset..], encoding)?,
    };

    let decoded_len = u64::try_from(text.len()).unwrap_or(u64::MAX);
    if decoded_len > max_bytes {
        return Err(limit_error(format!(
            "decoded XML has {decoded_len} UTF-8 bytes (maximum {max_bytes})"
        )));
    }
    Ok(DecodedXml { text, encoding })
}

/// Decode a bounded prefix used only for root-element detection.
pub fn decode_xml_prefix(bytes: &[u8], max_bytes: u64) -> Result<DecodedXml> {
    let bounded = &bytes[..bytes
        .len()
        .min(usize::try_from(max_bytes).unwrap_or(usize::MAX))];
    let (encoding, offset) = bom_encoding(bounded).unwrap_or((XmlEncoding::Utf8, 0));
    let payload = &bounded[offset..];
    let text = match encoding {
        XmlEncoding::Utf8 => match std::str::from_utf8(payload) {
            Ok(text) => text.to_owned(),
            Err(error) if error.error_len().is_none() => {
                std::str::from_utf8(&payload[..error.valid_up_to()])
                    .expect("valid UTF-8 prefix")
                    .to_owned()
            }
            Err(error) => {
                return Err(PluginError::corrupt(format!(
                    "XML prefix is not valid UTF-8: {error}"
                )));
            }
        },
        XmlEncoding::Utf16Le | XmlEncoding::Utf16Be => {
            let even_len = payload.len() - payload.len() % 2;
            decode_utf16_prefix(&payload[..even_len], encoding)?
        }
    };
    Ok(DecodedXml { text, encoding })
}

pub fn bom_encoding(bytes: &[u8]) -> Option<(XmlEncoding, usize)> {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        Some((XmlEncoding::Utf8, 3))
    } else if bytes.starts_with(&[0xFF, 0xFE]) {
        Some((XmlEncoding::Utf16Le, 2))
    } else if bytes.starts_with(&[0xFE, 0xFF]) {
        Some((XmlEncoding::Utf16Be, 2))
    } else {
        None
    }
}

fn decode_utf16(bytes: &[u8], encoding: XmlEncoding) -> Result<String> {
    if !bytes.len().is_multiple_of(2) {
        return Err(PluginError::corrupt(
            "UTF-16 XML has an incomplete final code unit",
        ));
    }
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| match encoding {
            XmlEncoding::Utf16Le => u16::from_le_bytes([pair[0], pair[1]]),
            XmlEncoding::Utf16Be => u16::from_be_bytes([pair[0], pair[1]]),
            XmlEncoding::Utf8 => unreachable!("UTF-8 has no u16 code units"),
        })
        .collect();
    String::from_utf16(&units)
        .map_err(|error| PluginError::corrupt(format!("XML is not valid UTF-16: {error}")))
}

fn decode_utf16_prefix(bytes: &[u8], encoding: XmlEncoding) -> Result<String> {
    let mut units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| match encoding {
            XmlEncoding::Utf16Le => u16::from_le_bytes([pair[0], pair[1]]),
            XmlEncoding::Utf16Be => u16::from_be_bytes([pair[0], pair[1]]),
            XmlEncoding::Utf8 => unreachable!("UTF-8 has no u16 code units"),
        })
        .collect();
    if units
        .last()
        .is_some_and(|unit| (0xD800..=0xDBFF).contains(unit))
    {
        units.pop();
    }
    String::from_utf16(&units)
        .map_err(|error| PluginError::corrupt(format!("XML prefix is not valid UTF-16: {error}")))
}

fn limit_error(message: impl Into<String>) -> PluginError {
    PluginError::corrupt(format!("resource limit exceeded: {}", message.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_utf8_and_both_utf16_byte_orders() {
        let source = "<HWPML>한글</HWPML>";
        let utf8 = [b"\xEF\xBB\xBF".as_slice(), source.as_bytes()].concat();
        assert_eq!(decode_xml(&utf8, 1024).expect("UTF-8").text, source);

        for encoding in [XmlEncoding::Utf16Le, XmlEncoding::Utf16Be] {
            let mut bytes = match encoding {
                XmlEncoding::Utf16Le => vec![0xFF, 0xFE],
                XmlEncoding::Utf16Be => vec![0xFE, 0xFF],
                XmlEncoding::Utf8 => unreachable!(),
            };
            for unit in source.encode_utf16() {
                let pair = match encoding {
                    XmlEncoding::Utf16Le => unit.to_le_bytes(),
                    XmlEncoding::Utf16Be => unit.to_be_bytes(),
                    XmlEncoding::Utf8 => unreachable!(),
                };
                bytes.extend_from_slice(&pair);
            }
            let decoded = decode_xml(&bytes, 1024).expect("UTF-16");
            assert_eq!(decoded.text, source);
            assert_eq!(decoded.encoding, encoding);
        }
    }

    #[test]
    fn rejects_odd_or_invalid_utf16() {
        assert!(decode_xml(&[0xFF, 0xFE, 0x3C], 32).is_err());
        assert!(decode_xml(&[0xFF, 0xFE, 0x00, 0xD8], 32).is_err());
    }

    #[test]
    fn prefix_decoder_tolerates_a_trailing_high_surrogate() {
        let mut bytes = vec![0xFF, 0xFE];
        for unit in "<HWPML>".encode_utf16().chain([0xD800]) {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }

        let decoded = decode_xml_prefix(&bytes, bytes.len() as u64)
            .expect("a bounded prefix may end between a surrogate pair");
        assert_eq!(decoded.text, "<HWPML>");
    }
}
