//! Package manifest catalog for picture binary references.
//!
//! The DOCX projection reader resolves `binaryItemIDRef` leniently (prefix-free
//! tag scanning and a BinData file-stem fallback). The source-aware read model
//! must not guess: it resolves only an exact, unique OPF manifest `id` whose
//! `href` names an existing package part, and otherwise reports why not.

use super::*;
use crate::owpml::package::HPF_ENTRY;

const OPF_NAMESPACE: &[u8] = b"http://www.idpf.org/2007/opf/";
/// Mirrors the package reader's content.hpf limit.
const MAX_MANIFEST_BYTES: u64 = 4 * 1024 * 1024;
/// Mirrors the package reader's manifest item limit.
pub(super) const MAX_MANIFEST_ITEMS: usize = 4096;
const MAX_MANIFEST_DEPTH: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq)]
struct ManifestItem {
    href: Option<String>,
    media_type: Option<String>,
    embedded: Option<String>,
}

#[derive(Clone, Debug, Default)]
enum Manifest {
    #[default]
    Missing,
    /// The lenient package reader accepted it, but the strict namespace-aware
    /// parse did not. Read-only sessions stay usable; nothing is resolved.
    Unreadable,
    Parsed(BTreeMap<String, Vec<ManifestItem>>),
}

/// Outcome of resolving one picture reference.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Resolution {
    Resolved {
        part: String,
        size: u64,
        media_type: Option<String>,
        embedded: Option<bool>,
    },
    External,
    MissingManifest,
    UnreadableManifest,
    MissingItem,
    AmbiguousItem,
    MissingPart {
        embedded: Option<bool>,
    },
}

impl Resolution {
    pub(super) fn status(&self) -> &'static str {
        match self {
            Self::Resolved { .. } => "resolved",
            Self::External => "external",
            Self::MissingManifest => "missing_manifest",
            Self::UnreadableManifest => "unreadable_manifest",
            Self::MissingItem => "missing_item",
            Self::AmbiguousItem => "ambiguous_item",
            Self::MissingPart { .. } => "missing_part",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(super) struct BinaryCatalog {
    manifest: Manifest,
    parts: BTreeMap<String, u64>,
}

impl BinaryCatalog {
    /// Read the ZIP directory and `Contents/content.hpf` of a package.
    pub(super) fn load(path: &Path) -> Result<Self> {
        let mut archive = ZipArchive::new(BufReader::new(File::open(path)?))?;
        let mut parts = BTreeMap::new();
        for index in 0..archive.len() {
            let entry = archive.by_index_raw(index)?;
            if !entry.is_dir() {
                parts.insert(entry.name().to_owned(), entry.size());
            }
        }
        let manifest = match archive.by_name(HPF_ENTRY) {
            Ok(mut entry) => {
                if entry.size() > MAX_MANIFEST_BYTES {
                    return Err(PluginError::unsupported_feature(format!(
                        "{HPF_ENTRY} exceeds {MAX_MANIFEST_BYTES} bytes"
                    )));
                }
                let mut bytes = Vec::new();
                entry
                    .by_ref()
                    .take(MAX_MANIFEST_BYTES.saturating_add(1))
                    .read_to_end(&mut bytes)?;
                if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_MANIFEST_BYTES {
                    return Err(PluginError::unsupported_feature(format!(
                        "{HPF_ENTRY} exceeded {MAX_MANIFEST_BYTES} bytes while reading"
                    )));
                }
                Some(bytes)
            }
            Err(zip::result::ZipError::FileNotFound) => None,
            Err(error) => return Err(error.into()),
        };
        Ok(Self::from_parts(manifest.as_deref(), parts))
    }

    /// Build a catalog from manifest bytes and the package part sizes.
    pub(super) fn from_parts(manifest: Option<&[u8]>, parts: BTreeMap<String, u64>) -> Self {
        let manifest = match manifest {
            None => Manifest::Missing,
            Some(bytes) => match parse_manifest(bytes) {
                Ok(items) => Manifest::Parsed(items),
                Err(_) => Manifest::Unreadable,
            },
        };
        Self { manifest, parts }
    }

    pub(super) fn resolve(&self, id: &str) -> Resolution {
        let items = match &self.manifest {
            Manifest::Missing => return Resolution::MissingManifest,
            Manifest::Unreadable => return Resolution::UnreadableManifest,
            Manifest::Parsed(items) => items,
        };
        let item = match items.get(id).map(Vec::as_slice) {
            None | Some([]) => return Resolution::MissingItem,
            Some([item]) => item,
            Some(_) => return Resolution::AmbiguousItem,
        };
        let embedded = match item.embedded.as_deref() {
            Some("1") => Some(true),
            Some("0") => Some(false),
            _ => None,
        };
        if embedded == Some(false) {
            return Resolution::External;
        }
        match item
            .href
            .as_ref()
            .and_then(|href| self.parts.get_key_value(href))
        {
            Some((part, size)) => Resolution::Resolved {
                part: part.clone(),
                size: *size,
                media_type: item.media_type.clone(),
                embedded,
            },
            None => Resolution::MissingPart { embedded },
        }
    }
}

fn parse_manifest(xml: &[u8]) -> Result<BTreeMap<String, Vec<ManifestItem>>> {
    let mut reader = NsReader::from_reader(xml);
    reader.config_mut().trim_text(false);
    let mut buffer = Vec::new();
    // Each open element records whether it is an OPF `manifest`.
    let mut stack = Vec::<bool>::new();
    let mut items = BTreeMap::<String, Vec<ManifestItem>>::new();
    let mut count = 0usize;
    loop {
        let event = reader.read_event_into(&mut buffer)?;
        match &event {
            Event::Start(element) | Event::Empty(element) => {
                if stack.len() >= MAX_MANIFEST_DEPTH {
                    return Err(PluginError::unsupported_feature(
                        "HWPX package manifest exceeds the XML depth budget",
                    ));
                }
                let (namespace, name) = reader.resolver().resolve_element(element.name());
                let opf = match namespace {
                    ResolveResult::Bound(ns) => ns.as_ref() == OPF_NAMESPACE,
                    ResolveResult::Unknown(prefix) => {
                        return Err(PluginError::corrupt(format!(
                            "HWPX package manifest uses undeclared prefix {:?}",
                            String::from_utf8_lossy(&prefix)
                        )));
                    }
                    ResolveResult::Unbound => false,
                };
                let in_manifest = stack.last().copied().unwrap_or(false);
                if opf && in_manifest && name.as_ref() == b"item" {
                    count += 1;
                    if count > MAX_MANIFEST_ITEMS {
                        return Err(PluginError::unsupported_feature(format!(
                            "HWPX package manifest exceeds {MAX_MANIFEST_ITEMS} items"
                        )));
                    }
                    if let Some(id) = exact_attribute(element, b"id")? {
                        items.entry(id).or_default().push(ManifestItem {
                            href: exact_attribute(element, b"href")?,
                            media_type: exact_attribute(element, b"media-type")?,
                            embedded: exact_attribute(element, b"isEmbeded")?,
                        });
                    }
                }
                if matches!(event, Event::Start(_)) {
                    stack.push(opf && name.as_ref() == b"manifest");
                }
            }
            Event::End(_) => {
                stack.pop().ok_or_else(|| {
                    PluginError::corrupt("HWPX package manifest closing-element underflow")
                })?;
            }
            Event::DocType(_) => {
                return Err(PluginError::corrupt(
                    "HWPX package manifest must not contain a document type declaration",
                ));
            }
            Event::Eof => {
                if !stack.is_empty() {
                    return Err(PluginError::corrupt(
                        "HWPX package manifest ended before all elements were closed",
                    ));
                }
                break;
            }
            _ => {}
        }
        buffer.clear();
    }
    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HPF: &str = r#"<opf:package xmlns:opf="http://www.idpf.org/2007/opf/"><opf:manifest>
<opf:item id="image1" href="BinData/image1.png" media-type="image/png" isEmbeded="1"/>
<opf:item id="link" href="D:\pictures\" media-type="image/" isEmbeded="0"/>
<opf:item id="dup" href="BinData/a.png"/><opf:item id="dup" href="BinData/b.png"/>
<opf:item id="gone" href="BinData/gone.png" isEmbeded="1"/>
<x:item xmlns:x="urn:foreign" id="foreign" href="BinData/image1.png"/>
</opf:manifest><opf:item id="outside" href="BinData/image1.png"/></opf:package>"#;

    fn catalog() -> BinaryCatalog {
        let parts = BTreeMap::from([
            ("BinData/image1.png".to_owned(), 42),
            ("BinData/a.png".to_owned(), 1),
        ]);
        BinaryCatalog::from_parts(Some(HPF.as_bytes()), parts)
    }

    #[test]
    fn resolves_only_unique_embedded_manifest_parts() {
        let catalog = catalog();
        assert_eq!(
            catalog.resolve("image1"),
            Resolution::Resolved {
                part: "BinData/image1.png".into(),
                size: 42,
                media_type: Some("image/png".into()),
                embedded: Some(true),
            }
        );
        assert_eq!(catalog.resolve("link"), Resolution::External);
        assert_eq!(catalog.resolve("dup"), Resolution::AmbiguousItem);
        assert_eq!(
            catalog.resolve("gone"),
            Resolution::MissingPart {
                embedded: Some(true)
            }
        );
        // Neither a foreign-namespace item nor one outside opf:manifest counts,
        // and a BinData file stem is never used as a fallback.
        for id in ["foreign", "outside", "a", "image2"] {
            assert_eq!(catalog.resolve(id), Resolution::MissingItem, "{id}");
        }
        let without = BinaryCatalog::from_parts(None, BTreeMap::new());
        assert_eq!(without.resolve("image1"), Resolution::MissingManifest);
        let unreadable = BinaryCatalog::from_parts(Some(b"<q:package/>"), BTreeMap::new());
        assert_eq!(unreadable.resolve("image1"), Resolution::UnreadableManifest);
    }

    #[test]
    fn manifest_parser_fails_closed_and_enforces_the_item_budget() {
        for bad in [
            r#"<!DOCTYPE x><opf:package xmlns:opf="http://www.idpf.org/2007/opf/"/>"#,
            r#"<opf:package xmlns:opf="http://www.idpf.org/2007/opf/"><opf:manifest><opf:item id="a" id="b"/></opf:manifest></opf:package>"#,
            r#"<q:package><q:manifest/></q:package>"#,
            r#"<opf:package xmlns:opf="http://www.idpf.org/2007/opf/"><opf:manifest>"#,
        ] {
            assert!(parse_manifest(bad.as_bytes()).is_err(), "{bad}");
        }
        for (count, valid) in [(MAX_MANIFEST_ITEMS, true), (MAX_MANIFEST_ITEMS + 1, false)] {
            let items = (0..count)
                .map(|index| format!(r#"<opf:item id="i{index}" href="x"/>"#))
                .collect::<String>();
            let xml = format!(
                r#"<opf:package xmlns:opf="http://www.idpf.org/2007/opf/"><opf:manifest>{items}</opf:manifest></opf:package>"#
            );
            assert_eq!(parse_manifest(xml.as_bytes()).is_ok(), valid, "{count}");
        }
    }
}
