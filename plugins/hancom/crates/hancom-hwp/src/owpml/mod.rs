//! HWPX(OWPML) 판독.

mod equation;
pub mod model;
mod numbering;
pub mod package;
pub mod section;
pub mod styles;
pub mod xml;

use std::fs::File;
use std::io::{BufReader, Read, Seek};
use std::path::Path;

use crate::error::{PluginError, Result};
use model::{Block, Document, Inline};
use officecli_hancom_core::budget::ResourceBudget;
use package::Package;
use styles::StyleTable;

const MAX_IMAGE_REFERENCES: usize = 512;
const MAX_EMBEDDED_IMAGE_OUTPUT_BYTES: u64 = 64 * 1024 * 1024;

struct ImageBudget {
    references: ResourceBudget,
    output_bytes: ResourceBudget,
}

impl Default for ImageBudget {
    fn default() -> Self {
        Self {
            references: ResourceBudget::new(
                "image reference count",
                u64::try_from(MAX_IMAGE_REFERENCES).expect("constant fits in u64"),
            ),
            output_bytes: ResourceBudget::new(
                "embedded image bytes",
                MAX_EMBEDDED_IMAGE_OUTPUT_BYTES,
            ),
        }
    }
}

impl ImageBudget {
    fn record_reference(&mut self) -> Result<()> {
        self.references.consume(1)
    }

    fn record_output_bytes(&mut self, bytes: usize) -> Result<()> {
        let bytes = u64::try_from(bytes).unwrap_or(u64::MAX);
        self.output_bytes.consume(bytes)
    }
}

/// HWPX 파일을 열어 문서 모델로 만든다.
pub fn read_document(path: &Path) -> Result<Document> {
    let file = File::open(path)
        .map_err(|e| PluginError::corrupt(format!("cannot open {}: {e}", path.display())))?;
    read_document_from(BufReader::new(file))
}

pub fn read_document_from<R: Read + Seek>(reader: R) -> Result<Document> {
    let mut pkg = Package::open(reader)?;

    let styles = match pkg.read_header_xml()? {
        Some(xml) => StyleTable::parse(&xml)?,
        // header.xml이 없으면 서식 없이 진행한다. 텍스트라도 살리는 편이 낫다.
        None => StyleTable::default(),
    };

    let paths: Vec<String> = pkg.section_paths().to_vec();
    let mut sections = Vec::with_capacity(paths.len());
    let mut section_outline_ids = Vec::with_capacity(paths.len());
    for p in &paths {
        let xml = pkg.read_section_xml(p)?;
        let (section, outline_id) = section::parse_section_with_outline(&xml, &styles)?;
        sections.push(section);
        section_outline_ids.push(outline_id);
    }

    let named_styles = styles.materialize_named_styles(&sections, &section_outline_ids)?;
    let numberings = styles.materialize_numberings(&sections, &named_styles)?;
    let mut doc = Document {
        sections,
        numberings,
        styles: named_styles,
    };
    resolve_images(&mut pkg, &mut doc)?;
    Ok(doc)
}

/// 문서 안의 이미지 참조를 BinData 실제 바이트로 채운다.
///
/// 찾지 못한 참조는 `data`가 `None`으로 남고, emitter가 그 이미지를 건너뛴다.
fn resolve_images<R: Read + Seek>(pkg: &mut Package<R>, doc: &mut Document) -> Result<()> {
    let mut budget = ImageBudget::default();
    for section in &mut doc.sections {
        resolve_in_blocks(pkg, &mut section.blocks, &mut budget)?;
        for story in section.headers.iter_mut().chain(&mut section.footers) {
            resolve_in_blocks(pkg, &mut story.blocks, &mut budget)?;
        }
    }
    Ok(())
}

/// 블록 목록을 재귀로 훑는다. 셀 안 중첩표도 따라간다.
fn resolve_in_blocks<R: Read + Seek>(
    pkg: &mut Package<R>,
    blocks: &mut [Block],
    budget: &mut ImageBudget,
) -> Result<()> {
    for block in blocks {
        match block {
            Block::Paragraph(p) => resolve_in_inlines(pkg, &mut p.inlines, budget)?,
            Block::Table(t) => {
                for cell in &mut t.cells {
                    resolve_in_blocks(pkg, &mut cell.blocks, budget)?;
                }
            }
        }
    }
    Ok(())
}

fn resolve_in_inlines<R: Read + Seek>(
    pkg: &mut Package<R>,
    inlines: &mut [Inline],
    budget: &mut ImageBudget,
) -> Result<()> {
    for inline in inlines {
        match inline {
            Inline::Image(img) => {
                budget.record_reference()?;
                if let Some(bytes) = img.data.as_ref() {
                    budget.record_output_bytes(bytes.len())?;
                    continue;
                }
                if let Some((bytes, ctype)) = pkg.read_bin_item(&img.bin_item_id)? {
                    budget.record_output_bytes(bytes.len())?;
                    img.data = Some(bytes);
                    img.content_type = Some(ctype);
                }
            }
            Inline::Note(note) => resolve_in_blocks(pkg, &mut note.blocks, budget)?,
            Inline::Rectangle(rectangle) => {
                if let Some(text) = &mut rectangle.text {
                    resolve_in_blocks(pkg, &mut text.blocks, budget)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}
