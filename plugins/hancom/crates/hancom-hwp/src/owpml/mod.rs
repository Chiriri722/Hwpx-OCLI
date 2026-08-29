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
const MAX_CHART_REFERENCES: usize = 512;
const MAX_EMBEDDED_CHART_OUTPUT_BYTES: u64 = 64 * 1024 * 1024;

struct ImageBudget {
    references: ResourceBudget,
    output_bytes: ResourceBudget,
}

struct ChartBudget {
    references: ResourceBudget,
    output_bytes: ResourceBudget,
}

impl Default for ChartBudget {
    fn default() -> Self {
        Self {
            references: ResourceBudget::new(
                "chart reference count",
                u64::try_from(MAX_CHART_REFERENCES).expect("constant fits in u64"),
            ),
            output_bytes: ResourceBudget::new(
                "embedded chart XML bytes",
                MAX_EMBEDDED_CHART_OUTPUT_BYTES,
            ),
        }
    }
}

impl ChartBudget {
    fn record_reference(&mut self) -> Result<()> {
        self.references.consume(1)
    }

    fn record_output_bytes(&mut self, bytes: usize) -> Result<()> {
        self.output_bytes
            .consume(u64::try_from(bytes).unwrap_or(u64::MAX))
    }
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
    resolve_embedded_resources(&mut pkg, &mut doc)?;
    Ok(doc)
}

/// 문서 안의 BinData 이미지와 자가 완결형 차트 참조를 실제 payload로 채운다.
///
/// 찾지 못한 이미지는 `data`가 `None`으로 남고 emitter가 건너뛴다. 차트 참조는
/// 편집 가능한 DOCX 개체를 보존하는 데 필수이므로 누락되면 즉시 실패한다.
fn resolve_embedded_resources<R: Read + Seek>(
    pkg: &mut Package<R>,
    doc: &mut Document,
) -> Result<()> {
    let mut image_budget = ImageBudget::default();
    let mut chart_budget = ChartBudget::default();
    for section in &mut doc.sections {
        resolve_in_blocks(
            pkg,
            &mut section.blocks,
            &mut image_budget,
            &mut chart_budget,
        )?;
        for story in section.headers.iter_mut().chain(&mut section.footers) {
            resolve_in_blocks(pkg, &mut story.blocks, &mut image_budget, &mut chart_budget)?;
        }
    }
    Ok(())
}

/// 블록 목록을 재귀로 훑는다. 셀 안 중첩표도 따라간다.
fn resolve_in_blocks<R: Read + Seek>(
    pkg: &mut Package<R>,
    blocks: &mut [Block],
    image_budget: &mut ImageBudget,
    chart_budget: &mut ChartBudget,
) -> Result<()> {
    for block in blocks {
        match block {
            Block::Paragraph(p) => {
                resolve_in_inlines(pkg, &mut p.inlines, image_budget, chart_budget)?
            }
            Block::Table(t) => {
                for cell in &mut t.cells {
                    resolve_in_blocks(pkg, &mut cell.blocks, image_budget, chart_budget)?;
                }
            }
        }
    }
    Ok(())
}

fn resolve_in_inlines<R: Read + Seek>(
    pkg: &mut Package<R>,
    inlines: &mut [Inline],
    image_budget: &mut ImageBudget,
    chart_budget: &mut ChartBudget,
) -> Result<()> {
    for inline in inlines {
        match inline {
            Inline::Image(img) => {
                image_budget.record_reference()?;
                if let Some(bytes) = img.data.as_ref() {
                    image_budget.record_output_bytes(bytes.len())?;
                    continue;
                }
                if let Some((bytes, ctype)) = pkg.read_bin_item(&img.bin_item_id)? {
                    image_budget.record_output_bytes(bytes.len())?;
                    img.data = Some(bytes);
                    img.content_type = Some(ctype);
                }
            }
            Inline::Chart(chart) => {
                chart_budget.record_reference()?;
                if let Some(xml) = chart.xml.as_ref() {
                    chart_budget.record_output_bytes(xml.len())?;
                    continue;
                }
                let xml = pkg.read_chart_part(&chart.chart_id_ref)?;
                chart_budget.record_output_bytes(xml.len())?;
                chart.xml = Some(xml);
            }
            Inline::Note(note) => {
                resolve_in_blocks(pkg, &mut note.blocks, image_budget, chart_budget)?
            }
            Inline::Rectangle(rectangle) => {
                if let Some(text) = &mut rectangle.text {
                    resolve_in_blocks(pkg, &mut text.blocks, image_budget, chart_budget)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}
