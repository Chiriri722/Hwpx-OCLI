//! HWPX(OWPML) 판독.

pub mod model;
pub mod package;
pub mod section;
pub mod styles;
pub mod xml;

use std::fs::File;
use std::io::{BufReader, Read, Seek};
use std::path::Path;

use crate::error::{PluginError, Result};
use model::{Block, Document, Inline};
use package::Package;
use styles::StyleTable;

/// HWPX 파일을 열어 문서 모델로 만든다.
pub fn read_document(path: &Path) -> Result<Document> {
    let file = File::open(path).map_err(|e| {
        PluginError::corrupt(format!("cannot open {}: {e}", path.display()))
    })?;
    read_document_from(BufReader::new(file))
}

pub fn read_document_from<R: Read + Seek>(reader: R) -> Result<Document> {
    let mut pkg = Package::open(reader)?;

    let styles = match pkg.read_header_xml() {
        Some(xml) => StyleTable::parse(&xml)?,
        // header.xml이 없으면 서식 없이 진행한다. 텍스트라도 살리는 편이 낫다.
        None => StyleTable::default(),
    };

    let paths: Vec<String> = pkg.section_paths().to_vec();
    let mut blocks = Vec::new();
    for p in &paths {
        let xml = pkg.read_section_xml(p)?;
        blocks.extend(section::parse_section(&xml, &styles)?);
    }

    let mut doc = Document { blocks };
    resolve_images(&mut pkg, &mut doc);
    Ok(doc)
}

/// 문서 안의 이미지 참조를 BinData 실제 바이트로 채운다.
///
/// 찾지 못한 참조는 `data`가 `None`으로 남고, emitter가 그 이미지를 건너뛴다.
fn resolve_images<R: Read + Seek>(pkg: &mut Package<R>, doc: &mut Document) {
    resolve_in_blocks(pkg, &mut doc.blocks);
}

/// 블록 목록을 재귀로 훑는다. 셀 안 중첩표도 따라간다.
fn resolve_in_blocks<R: Read + Seek>(pkg: &mut Package<R>, blocks: &mut [Block]) {
    for block in blocks {
        match block {
            Block::Paragraph(p) => resolve_in_inlines(pkg, &mut p.inlines),
            Block::Table(t) => {
                for cell in &mut t.cells {
                    resolve_in_blocks(pkg, &mut cell.blocks);
                }
            }
        }
    }
}

fn resolve_in_inlines<R: Read + Seek>(pkg: &mut Package<R>, inlines: &mut [Inline]) {
    for inline in inlines {
        if let Inline::Image(img) = inline {
            if img.data.is_some() {
                continue;
            }
            if let Some((bytes, ctype)) = pkg.read_bin_item(&img.bin_item_id) {
                img.data = Some(bytes);
                img.content_type = Some(ctype);
            }
        }
    }
}
