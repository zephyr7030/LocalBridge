use std::io::{Cursor, Read, Write};
use std::path::Path;

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use quick_xml::Reader;
use quick_xml::events::Event as XmlEvent;
use serde::Serialize;
use sha2::{Digest, Sha256};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::filesystem::service::{FilesystemError, FilesystemService};
use crate::workspace::path_authority::WorkspaceResolver;

const MAX_DOCUMENT_BYTES: usize = 16 * 1024 * 1024;
const MAX_INSPECT_BYTES: usize = 1024 * 1024;
const MAX_SEARCH_RESULTS: usize = 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentFormat {
    Text,
    Markdown,
    Docx,
    Pdf,
}

impl DocumentFormat {
    pub fn from_path(path: &str) -> Result<Self, DocumentError> {
        let extension = Path::new(path)
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        if extension.eq_ignore_ascii_case("txt") {
            Ok(Self::Text)
        } else if extension.eq_ignore_ascii_case("md") || extension.eq_ignore_ascii_case("markdown")
        {
            Ok(Self::Markdown)
        } else if extension.eq_ignore_ascii_case("docx") {
            Ok(Self::Docx)
        } else if extension.eq_ignore_ascii_case("pdf") {
            Ok(Self::Pdf)
        } else {
            Err(DocumentError::UnsupportedFormat)
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Markdown => "markdown",
            Self::Docx => "docx",
            Self::Pdf => "pdf",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentBlockKind {
    Paragraph,
    Heading,
    ListItem,
    Blank,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DocumentBlock {
    pub id: String,
    pub kind: DocumentBlockKind,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentIr {
    pub format: DocumentFormat,
    pub blocks: Vec<DocumentBlock>,
    editable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocumentEditOperation {
    Replace { block_id: String, content: String },
    InsertBefore { block_id: String, content: String },
    InsertAfter { block_id: String, content: String },
    Delete { block_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocumentRequest {
    Inspect {
        path: String,
        start_block: usize,
        max_blocks: usize,
        max_bytes: usize,
    },
    Search {
        path: String,
        query: String,
        case_sensitive: bool,
        max_results: usize,
    },
    Create {
        path: String,
        content: String,
        source_format: DocumentFormat,
    },
    Edit {
        path: String,
        expected_sha256: String,
        edits: Vec<DocumentEditOperation>,
    },
    Convert {
        source: String,
        path: String,
    },
    Rebuild {
        path: String,
        content: String,
        source_format: DocumentFormat,
        expected_sha256: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DocumentMatch {
    pub block_id: String,
    pub block_index: usize,
    pub excerpt: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum DocumentResult {
    Inspect {
        path: String,
        format: DocumentFormat,
        sha256: String,
        total_bytes: usize,
        start_block: usize,
        end_block: Option<usize>,
        total_blocks: usize,
        blocks: Vec<DocumentBlock>,
        text: String,
        truncated: bool,
    },
    Search {
        path: String,
        format: DocumentFormat,
        sha256: String,
        matches: Vec<DocumentMatch>,
        total_blocks: usize,
        truncated: bool,
    },
    Create {
        path: String,
        format: DocumentFormat,
        sha256: String,
        bytes: usize,
    },
    Edit {
        path: String,
        format: DocumentFormat,
        sha256: String,
        applied_edits: usize,
    },
    Convert {
        source: String,
        path: String,
        source_format: DocumentFormat,
        format: DocumentFormat,
        source_sha256: String,
        sha256: String,
        bytes: usize,
    },
    Rebuild {
        path: String,
        format: DocumentFormat,
        sha256: String,
        bytes: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentError {
    InvalidArgument,
    NotFound,
    OutsideAuthority,
    FileChanged,
    LimitExceeded,
    UnsupportedFormat,
    UnsupportedContent,
    CorruptDocument,
    Io,
}

#[derive(Debug, Clone)]
pub struct DocumentService {
    filesystem: FilesystemService,
}

impl DocumentService {
    pub fn with_authority(authority: WorkspaceResolver) -> Result<Self, DocumentError> {
        Ok(Self {
            filesystem: FilesystemService::from_authority(authority)
                .map_err(map_filesystem_error)?,
        })
    }

    pub fn execute(&self, request: DocumentRequest) -> Result<DocumentResult, DocumentError> {
        match request {
            DocumentRequest::Inspect {
                path,
                start_block,
                max_blocks,
                max_bytes,
            } => self.inspect(&path, start_block, max_blocks, max_bytes),
            DocumentRequest::Search {
                path,
                query,
                case_sensitive,
                max_results,
            } => self.search(&path, &query, case_sensitive, max_results),
            DocumentRequest::Create {
                path,
                content,
                source_format,
            } => self.create(&path, &content, source_format),
            DocumentRequest::Edit {
                path,
                expected_sha256,
                edits,
            } => self.edit(&path, &expected_sha256, edits),
            DocumentRequest::Convert { source, path } => self.convert(&source, &path),
            DocumentRequest::Rebuild {
                path,
                content,
                source_format,
                expected_sha256,
            } => self.rebuild(&path, &content, source_format, &expected_sha256),
        }
    }

    fn inspect(
        &self,
        path: &str,
        start_block: usize,
        max_blocks: usize,
        max_bytes: usize,
    ) -> Result<DocumentResult, DocumentError> {
        if start_block == 0
            || max_blocks == 0
            || max_blocks > 10_000
            || max_bytes == 0
            || max_bytes > MAX_INSPECT_BYTES
        {
            return Err(DocumentError::InvalidArgument);
        }
        let (bytes, sha256, ir) = self.load(path)?;
        let total_blocks = ir.blocks.len();
        if total_blocks > 0 && start_block > total_blocks {
            return Err(DocumentError::InvalidArgument);
        }
        let start_index = start_block.saturating_sub(1).min(total_blocks);
        let mut blocks = Vec::new();
        let mut returned_bytes = 0usize;
        let mut limit_hit = false;
        for block in ir.blocks.iter().skip(start_index).take(max_blocks) {
            let additional = block.text.len();
            if returned_bytes.saturating_add(additional) > max_bytes {
                limit_hit = true;
                break;
            }
            returned_bytes = returned_bytes.saturating_add(additional);
            blocks.push(block.clone());
        }
        let end_block = (!blocks.is_empty()).then_some(start_index + blocks.len());
        let more_by_count = start_index.saturating_add(blocks.len()) < total_blocks;
        let text = blocks
            .iter()
            .map(|block| block.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        Ok(DocumentResult::Inspect {
            path: self.display_path(path)?,
            format: ir.format,
            sha256,
            total_bytes: bytes.len(),
            start_block,
            end_block,
            total_blocks,
            blocks,
            text,
            truncated: limit_hit || more_by_count,
        })
    }

    fn search(
        &self,
        path: &str,
        query: &str,
        case_sensitive: bool,
        max_results: usize,
    ) -> Result<DocumentResult, DocumentError> {
        if query.is_empty() || max_results == 0 || max_results > MAX_SEARCH_RESULTS {
            return Err(DocumentError::InvalidArgument);
        }
        let (_, sha256, ir) = self.load(path)?;
        let needle = if case_sensitive {
            query.to_string()
        } else {
            query.to_lowercase()
        };
        let mut matches = Vec::new();
        let mut truncated = false;
        for (index, block) in ir.blocks.iter().enumerate() {
            let candidate = if case_sensitive {
                block.text.clone()
            } else {
                block.text.to_lowercase()
            };
            if candidate.contains(&needle) {
                if matches.len() == max_results {
                    truncated = true;
                    break;
                }
                matches.push(DocumentMatch {
                    block_id: block.id.clone(),
                    block_index: index + 1,
                    excerpt: block.text.clone(),
                });
            }
        }
        Ok(DocumentResult::Search {
            path: self.display_path(path)?,
            format: ir.format,
            sha256,
            matches,
            total_blocks: ir.blocks.len(),
            truncated,
        })
    }

    fn create(
        &self,
        path: &str,
        content: &str,
        source_format: DocumentFormat,
    ) -> Result<DocumentResult, DocumentError> {
        require_text_source(source_format)?;
        let format = DocumentFormat::from_path(path)?;
        if format == DocumentFormat::Pdf {
            return Err(DocumentError::UnsupportedFormat);
        }
        let ir = parse_textual(content, source_format);
        let bytes = render(&ir, format)?;
        self.filesystem
            .create_file_for_edit(path, &bytes)
            .map_err(map_filesystem_error)?;
        Ok(DocumentResult::Create {
            path: self.display_path(path)?,
            format,
            sha256: sha256_hex(&bytes),
            bytes: bytes.len(),
        })
    }

    fn edit(
        &self,
        path: &str,
        expected_sha256: &str,
        edits: Vec<DocumentEditOperation>,
    ) -> Result<DocumentResult, DocumentError> {
        if edits.is_empty() || !valid_sha256(expected_sha256) {
            return Err(DocumentError::InvalidArgument);
        }
        let (_bytes, sha256, mut ir) = self.load(path)?;
        if !sha256.eq_ignore_ascii_case(expected_sha256) {
            return Err(DocumentError::FileChanged);
        }
        if !ir.editable {
            return Err(if ir.format == DocumentFormat::Pdf {
                DocumentError::UnsupportedFormat
            } else {
                DocumentError::UnsupportedContent
            });
        }
        let edit_count = edits.len();
        for edit in edits {
            apply_edit(&mut ir, edit)?;
        }
        renumber_blocks(&mut ir.blocks);
        let updated = render(&ir, ir.format)?;
        self.filesystem
            .replace_file_if_sha256(path, expected_sha256, &updated)
            .map_err(map_filesystem_error)?;
        Ok(DocumentResult::Edit {
            path: self.display_path(path)?,
            format: ir.format,
            sha256: sha256_hex(&updated),
            applied_edits: edit_count,
        })
    }

    fn convert(&self, source: &str, path: &str) -> Result<DocumentResult, DocumentError> {
        let (_, source_sha256, ir) = self.load(source)?;
        let format = DocumentFormat::from_path(path)?;
        if format == DocumentFormat::Pdf {
            return Err(DocumentError::UnsupportedFormat);
        }
        let bytes = render(&ir, format)?;
        self.filesystem
            .create_file_for_edit(path, &bytes)
            .map_err(map_filesystem_error)?;
        Ok(DocumentResult::Convert {
            source: self.display_path(source)?,
            path: self.display_path(path)?,
            source_format: ir.format,
            format,
            source_sha256,
            sha256: sha256_hex(&bytes),
            bytes: bytes.len(),
        })
    }

    fn rebuild(
        &self,
        path: &str,
        content: &str,
        source_format: DocumentFormat,
        expected_sha256: &str,
    ) -> Result<DocumentResult, DocumentError> {
        require_text_source(source_format)?;
        if !valid_sha256(expected_sha256) {
            return Err(DocumentError::InvalidArgument);
        }
        let format = DocumentFormat::from_path(path)?;
        if format == DocumentFormat::Pdf {
            return Err(DocumentError::UnsupportedFormat);
        }
        let ir = parse_textual(content, source_format);
        let bytes = render(&ir, format)?;
        self.filesystem
            .replace_file_if_sha256(path, expected_sha256, &bytes)
            .map_err(map_filesystem_error)?;
        Ok(DocumentResult::Rebuild {
            path: self.display_path(path)?,
            format,
            sha256: sha256_hex(&bytes),
            bytes: bytes.len(),
        })
    }

    fn load(&self, path: &str) -> Result<(Vec<u8>, String, DocumentIr), DocumentError> {
        let format = DocumentFormat::from_path(path)?;
        let bytes = self
            .filesystem
            .read_bytes_bounded(path, MAX_DOCUMENT_BYTES)
            .map_err(map_filesystem_error)?;
        let sha256 = sha256_hex(&bytes);
        let ir = parse(&bytes, format)?;
        Ok((bytes, sha256, ir))
    }

    fn display_path(&self, path: &str) -> Result<String, DocumentError> {
        self.filesystem
            .hash(path)
            .map(|value| value.path)
            .map_err(map_filesystem_error)
    }
}

fn require_text_source(format: DocumentFormat) -> Result<(), DocumentError> {
    if matches!(format, DocumentFormat::Text | DocumentFormat::Markdown) {
        Ok(())
    } else {
        Err(DocumentError::InvalidArgument)
    }
}

fn parse(bytes: &[u8], format: DocumentFormat) -> Result<DocumentIr, DocumentError> {
    match format {
        DocumentFormat::Text | DocumentFormat::Markdown => {
            let text = std::str::from_utf8(bytes).map_err(|_| DocumentError::UnsupportedContent)?;
            Ok(parse_textual(text, format))
        }
        DocumentFormat::Docx => parse_docx(bytes),
        DocumentFormat::Pdf => parse_pdf(bytes),
    }
}

fn parse_textual(content: &str, format: DocumentFormat) -> DocumentIr {
    let normalized = normalize_newlines(content);
    let mut blocks = if format == DocumentFormat::Markdown {
        markdown_blocks(&normalized)
    } else {
        text_line_blocks(&normalized)
    };
    renumber_blocks(&mut blocks);
    DocumentIr {
        format,
        blocks,
        editable: true,
    }
}

fn text_line_blocks(content: &str) -> Vec<DocumentBlock> {
    if content.is_empty() {
        return Vec::new();
    }
    content
        .split('\n')
        .map(|line| {
            let kind = if line.is_empty() {
                DocumentBlockKind::Blank
            } else {
                DocumentBlockKind::Paragraph
            };
            DocumentBlock {
                id: String::new(),
                kind,
                text: line.to_string(),
                level: None,
            }
        })
        .collect()
}

fn markdown_blocks(content: &str) -> Vec<DocumentBlock> {
    let mut blocks = Vec::new();
    let mut active = None::<DocumentBlock>;
    for event in Parser::new_ext(content, Options::all()) {
        match event {
            Event::Start(Tag::Heading { level, .. }) if active.is_none() => {
                active = Some(DocumentBlock {
                    id: String::new(),
                    kind: DocumentBlockKind::Heading,
                    text: String::new(),
                    level: Some(heading_level(level)),
                });
            }
            Event::Start(Tag::Item) if active.is_none() => {
                active = Some(DocumentBlock {
                    id: String::new(),
                    kind: DocumentBlockKind::ListItem,
                    text: String::new(),
                    level: None,
                });
            }
            Event::Start(Tag::Paragraph) if active.is_none() => {
                active = Some(new_paragraph(String::new()));
            }
            Event::Text(text) | Event::Code(text) => {
                if let Some(block) = active.as_mut() {
                    block.text.push_str(&text);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some(block) = active.as_mut() {
                    block.text.push('\n');
                }
            }
            Event::End(TagEnd::Heading(_)) => finish_markdown_block(&mut active, &mut blocks),
            Event::End(TagEnd::Paragraph)
                if active
                    .as_ref()
                    .is_some_and(|block| block.kind == DocumentBlockKind::Paragraph) =>
            {
                finish_markdown_block(&mut active, &mut blocks);
            }
            Event::End(TagEnd::Item)
                if active
                    .as_ref()
                    .is_some_and(|block| block.kind == DocumentBlockKind::ListItem) =>
            {
                finish_markdown_block(&mut active, &mut blocks);
            }
            _ => {}
        }
    }
    finish_markdown_block(&mut active, &mut blocks);
    blocks
}

fn finish_markdown_block(active: &mut Option<DocumentBlock>, blocks: &mut Vec<DocumentBlock>) {
    if let Some(block) = active.take() {
        blocks.push(block);
    }
}

const fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn parse_pdf(bytes: &[u8]) -> Result<DocumentIr, DocumentError> {
    let text =
        pdf_extract::extract_text_from_mem(bytes).map_err(|_| DocumentError::CorruptDocument)?;
    if text.len() > MAX_DOCUMENT_BYTES {
        return Err(DocumentError::LimitExceeded);
    }
    let mut ir = parse_textual(&text, DocumentFormat::Text);
    ir.format = DocumentFormat::Pdf;
    ir.editable = false;
    Ok(ir)
}

fn parse_docx(bytes: &[u8]) -> Result<DocumentIr, DocumentError> {
    let cursor = Cursor::new(bytes);
    let mut archive = ZipArchive::new(cursor).map_err(|_| DocumentError::CorruptDocument)?;
    if archive.len() > 10_000 {
        return Err(DocumentError::LimitExceeded);
    }
    let mut editable = true;
    for index in 0..archive.len() {
        let name = archive
            .by_index(index)
            .map_err(|_| DocumentError::CorruptDocument)?
            .name()
            .to_ascii_lowercase();
        if name.starts_with("word/media/")
            || name.starts_with("word/header")
            || name.starts_with("word/footer")
            || name == "word/footnotes.xml"
            || name == "word/endnotes.xml"
            || name == "word/comments.xml"
        {
            editable = false;
        }
    }
    let mut xml = String::new();
    let mut document_xml = archive
        .by_name("word/document.xml")
        .map_err(|_| DocumentError::CorruptDocument)?;
    if document_xml.size() > MAX_DOCUMENT_BYTES as u64 {
        return Err(DocumentError::LimitExceeded);
    }
    document_xml
        .read_to_string(&mut xml)
        .map_err(|_| DocumentError::CorruptDocument)?;
    if xml.contains("<w:tbl") || xml.contains("<w:drawing") || xml.contains("<w:object") {
        editable = false;
    }

    let mut reader = Reader::from_str(&xml);
    reader.config_mut().trim_text(false);
    let mut blocks = Vec::new();
    let mut paragraph = None::<String>;
    let mut kind = DocumentBlockKind::Paragraph;
    let mut level = None;
    let mut in_text = false;
    loop {
        match reader.read_event() {
            Ok(XmlEvent::Start(event)) => match event.local_name().as_ref() {
                b"p" => {
                    paragraph = Some(String::new());
                    kind = DocumentBlockKind::Paragraph;
                    level = None;
                }
                b"t" => in_text = true,
                b"pStyle" => {
                    for attribute in event.attributes().flatten() {
                        if attribute.key.local_name().as_ref() == b"val" {
                            let value = String::from_utf8_lossy(attribute.value.as_ref());
                            if let Some(number) = value.strip_prefix("Heading") {
                                kind = DocumentBlockKind::Heading;
                                level = number
                                    .parse::<u8>()
                                    .ok()
                                    .filter(|value| (1..=6).contains(value));
                            } else if value == "ListParagraph" {
                                kind = DocumentBlockKind::ListItem;
                            }
                        }
                    }
                }
                b"tab" => {
                    if let Some(value) = paragraph.as_mut() {
                        value.push('\t');
                    }
                }
                b"br" => {
                    if let Some(value) = paragraph.as_mut() {
                        value.push('\n');
                    }
                }
                _ => {}
            },
            Ok(XmlEvent::Empty(event)) => match event.local_name().as_ref() {
                b"pStyle" => {
                    for attribute in event.attributes().flatten() {
                        if attribute.key.local_name().as_ref() == b"val" {
                            let value = String::from_utf8_lossy(attribute.value.as_ref());
                            if let Some(number) = value.strip_prefix("Heading") {
                                kind = DocumentBlockKind::Heading;
                                level = number
                                    .parse::<u8>()
                                    .ok()
                                    .filter(|value| (1..=6).contains(value));
                            } else if value == "ListParagraph" {
                                kind = DocumentBlockKind::ListItem;
                            }
                        }
                    }
                }
                b"tab" => {
                    if let Some(value) = paragraph.as_mut() {
                        value.push('\t');
                    }
                }
                b"br" => {
                    if let Some(value) = paragraph.as_mut() {
                        value.push('\n');
                    }
                }
                _ => {}
            },
            Ok(XmlEvent::Text(event)) if in_text => {
                let value = event.decode().map_err(|_| DocumentError::CorruptDocument)?;
                if let Some(paragraph) = paragraph.as_mut() {
                    paragraph.push_str(&value);
                }
            }
            Ok(XmlEvent::End(event)) => match event.local_name().as_ref() {
                b"t" => in_text = false,
                b"p" => {
                    let text = paragraph.take().unwrap_or_default();
                    blocks.push(DocumentBlock {
                        id: String::new(),
                        kind: if text.is_empty() {
                            DocumentBlockKind::Blank
                        } else {
                            kind
                        },
                        text,
                        level,
                    });
                }
                _ => {}
            },
            Ok(XmlEvent::Eof) => break,
            Err(_) => return Err(DocumentError::CorruptDocument),
            _ => {}
        }
    }
    renumber_blocks(&mut blocks);
    Ok(DocumentIr {
        format: DocumentFormat::Docx,
        blocks,
        editable,
    })
}

fn render(ir: &DocumentIr, target: DocumentFormat) -> Result<Vec<u8>, DocumentError> {
    match target {
        DocumentFormat::Text => Ok(render_text(ir).into_bytes()),
        DocumentFormat::Markdown => Ok(render_markdown(ir).into_bytes()),
        DocumentFormat::Docx => render_docx(ir),
        DocumentFormat::Pdf => Err(DocumentError::UnsupportedFormat),
    }
}

fn render_text(ir: &DocumentIr) -> String {
    ir.blocks
        .iter()
        .map(|block| block.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_markdown(ir: &DocumentIr) -> String {
    ir.blocks
        .iter()
        .map(|block| match block.kind {
            DocumentBlockKind::Heading => {
                format!(
                    "{} {}",
                    "#".repeat(block.level.unwrap_or(1) as usize),
                    block.text
                )
            }
            DocumentBlockKind::ListItem => format!("- {}", block.text),
            DocumentBlockKind::Paragraph | DocumentBlockKind::Blank => block.text.clone(),
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn render_docx(ir: &DocumentIr) -> Result<Vec<u8>, DocumentError> {
    let mut document = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>"#,
    );
    for block in &ir.blocks {
        document.push_str("<w:p>");
        match block.kind {
            DocumentBlockKind::Heading => document.push_str(&format!(
                "<w:pPr><w:pStyle w:val=\"Heading{}\"/></w:pPr>",
                block.level.unwrap_or(1)
            )),
            DocumentBlockKind::ListItem => {
                document.push_str("<w:pPr><w:pStyle w:val=\"ListParagraph\"/><w:numPr><w:ilvl w:val=\"0\"/><w:numId w:val=\"1\"/></w:numPr></w:pPr>")
            }
            DocumentBlockKind::Paragraph | DocumentBlockKind::Blank => {}
        }
        document.push_str("<w:r><w:t xml:space=\"preserve\">");
        document.push_str(&xml_escape(&block.text));
        document.push_str("</w:t></w:r></w:p>");
    }
    document.push_str("<w:sectPr/></w:body></w:document>");

    let cursor = Cursor::new(Vec::new());
    let mut writer = ZipWriter::new(cursor);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, content) in [
        (
            "[Content_Types].xml",
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/><Override PartName="/word/numbering.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        ),
        (
            "word/_rels/document.xml.rels",
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering" Target="numbering.xml"/></Relationships>"#,
        ),
        (
            "word/styles.xml",
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style><w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:pPr><w:outlineLvl w:val="0"/></w:pPr></w:style><w:style w:type="paragraph" w:styleId="Heading2"><w:name w:val="heading 2"/><w:pPr><w:outlineLvl w:val="1"/></w:pPr></w:style><w:style w:type="paragraph" w:styleId="Heading3"><w:name w:val="heading 3"/><w:pPr><w:outlineLvl w:val="2"/></w:pPr></w:style><w:style w:type="paragraph" w:styleId="Heading4"><w:name w:val="heading 4"/><w:pPr><w:outlineLvl w:val="3"/></w:pPr></w:style><w:style w:type="paragraph" w:styleId="Heading5"><w:name w:val="heading 5"/><w:pPr><w:outlineLvl w:val="4"/></w:pPr></w:style><w:style w:type="paragraph" w:styleId="Heading6"><w:name w:val="heading 6"/><w:pPr><w:outlineLvl w:val="5"/></w:pPr></w:style><w:style w:type="paragraph" w:styleId="ListParagraph"><w:name w:val="List Paragraph"/></w:style></w:styles>"#,
        ),
        (
            "word/numbering.xml",
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="0"><w:numFmt w:val="bullet"/><w:lvlText w:val="•"/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="720" w:hanging="360"/></w:pPr></w:lvl></w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num></w:numbering>"#,
        ),
        ("word/document.xml", document.as_str()),
    ] {
        writer
            .start_file(name, options)
            .map_err(|_| DocumentError::Io)?;
        writer
            .write_all(content.as_bytes())
            .map_err(|_| DocumentError::Io)?;
    }
    writer
        .finish()
        .map(|cursor| cursor.into_inner())
        .map_err(|_| DocumentError::Io)
}

fn apply_edit(ir: &mut DocumentIr, edit: DocumentEditOperation) -> Result<(), DocumentError> {
    let (block_id, action) = match edit {
        DocumentEditOperation::Replace { block_id, content } => (block_id, (0u8, Some(content))),
        DocumentEditOperation::InsertBefore { block_id, content } => {
            (block_id, (1u8, Some(content)))
        }
        DocumentEditOperation::InsertAfter { block_id, content } => {
            (block_id, (2u8, Some(content)))
        }
        DocumentEditOperation::Delete { block_id } => (block_id, (3u8, None)),
    };
    if action
        .1
        .as_deref()
        .is_some_and(|value| value.contains(['\r', '\n']))
    {
        return Err(DocumentError::InvalidArgument);
    }
    let index = ir
        .blocks
        .iter()
        .position(|block| block.id == block_id)
        .ok_or(DocumentError::InvalidArgument)?;
    match action {
        (0, Some(content)) => ir.blocks[index].text = content,
        (1, Some(content)) => ir.blocks.insert(index, new_paragraph(content)),
        (2, Some(content)) => ir.blocks.insert(index + 1, new_paragraph(content)),
        (3, None) => {
            ir.blocks.remove(index);
        }
        _ => return Err(DocumentError::InvalidArgument),
    }
    Ok(())
}

fn new_paragraph(text: String) -> DocumentBlock {
    DocumentBlock {
        id: String::new(),
        kind: if text.is_empty() {
            DocumentBlockKind::Blank
        } else {
            DocumentBlockKind::Paragraph
        },
        text,
        level: None,
    }
}

fn renumber_blocks(blocks: &mut [DocumentBlock]) {
    for (index, block) in blocks.iter_mut().enumerate() {
        block.id = format!("block-{}", index + 1);
    }
}

fn normalize_newlines(value: &str) -> String {
    value.replace("\r\n", "\n").replace('\r', "\n")
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn map_filesystem_error(error: FilesystemError) -> DocumentError {
    match error {
        FilesystemError::InvalidArgument => DocumentError::InvalidArgument,
        FilesystemError::NotFound => DocumentError::NotFound,
        FilesystemError::OutsideAuthority => DocumentError::OutsideAuthority,
        FilesystemError::AlreadyExists => DocumentError::FileChanged,
        FilesystemError::FileChanged => DocumentError::FileChanged,
        FilesystemError::LimitExceeded => DocumentError::LimitExceeded,
        FilesystemError::Unsupported => DocumentError::UnsupportedContent,
        FilesystemError::Cancelled | FilesystemError::Io => DocumentError::Io,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixture() -> (std::path::PathBuf, DocumentService) {
        let root = std::env::temp_dir().join(format!(
            "localbridge-document-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let authority = WorkspaceResolver::active_workspace(&root).unwrap();
        (root, DocumentService::with_authority(authority).unwrap())
    }

    fn simple_pdf(text: &str) -> Vec<u8> {
        let escaped = text
            .replace('\\', "\\\\")
            .replace('(', "\\(")
            .replace(')', "\\)");
        let stream = format!("BT /F1 12 Tf 72 720 Td ({escaped}) Tj ET");
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".to_string(),
            format!("<< /Length {} >>\nstream\n{stream}\nendstream", stream.len()),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        ];
        let mut pdf = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (index, object) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
        }
        let xref = pdf.len();
        pdf.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
        pdf.extend_from_slice(b"0000000000 65535 f \n");
        for offset in offsets {
            pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        pdf
    }

    #[test]
    fn text_edit_is_hash_guarded_and_atomic() {
        let (root, service) = fixture();
        fs::write(root.join("note.md"), "# Title\nold\n").unwrap();
        let inspected = service
            .execute(DocumentRequest::Inspect {
                path: "note.md".into(),
                start_block: 1,
                max_blocks: 20,
                max_bytes: 1024,
            })
            .unwrap();
        let sha256 = match inspected {
            DocumentResult::Inspect { sha256, .. } => sha256,
            _ => unreachable!(),
        };
        let edited = service
            .execute(DocumentRequest::Edit {
                path: "note.md".into(),
                expected_sha256: sha256.clone(),
                edits: vec![DocumentEditOperation::Replace {
                    block_id: "block-2".into(),
                    content: "new".into(),
                }],
            })
            .unwrap();
        assert!(matches!(edited, DocumentResult::Edit { .. }));
        assert_eq!(
            fs::read_to_string(root.join("note.md")).unwrap(),
            "# Title\n\nnew"
        );
        assert_eq!(
            service
                .execute(DocumentRequest::Edit {
                    path: "note.md".into(),
                    expected_sha256: sha256.clone(),
                    edits: vec![DocumentEditOperation::Delete {
                        block_id: "block-1".into(),
                    }],
                })
                .unwrap_err(),
            DocumentError::FileChanged
        );
        assert_eq!(
            service
                .execute(DocumentRequest::Rebuild {
                    path: "note.md".into(),
                    content: "replacement".into(),
                    source_format: DocumentFormat::Markdown,
                    expected_sha256: sha256,
                })
                .unwrap_err(),
            DocumentError::FileChanged
        );
        assert_eq!(
            service
                .execute(DocumentRequest::Create {
                    path: "note.md".into(),
                    content: "replacement".into(),
                    source_format: DocumentFormat::Markdown,
                })
                .unwrap_err(),
            DocumentError::FileChanged
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn docx_round_trip_and_search_use_document_ir() {
        let (root, service) = fixture();
        let created = service
            .execute(DocumentRequest::Create {
                path: "note.docx".into(),
                content: "# Title\nneedle\n".into(),
                source_format: DocumentFormat::Markdown,
            })
            .unwrap();
        let sha256 = match created {
            DocumentResult::Create { sha256, .. } => sha256,
            _ => unreachable!(),
        };
        let search = service
            .execute(DocumentRequest::Search {
                path: "note.docx".into(),
                query: "needle".into(),
                case_sensitive: true,
                max_results: 10,
            })
            .unwrap();
        assert!(matches!(
            search,
            DocumentResult::Search { matches, .. } if matches.len() == 1
        ));
        service
            .execute(DocumentRequest::Edit {
                path: "note.docx".into(),
                expected_sha256: sha256,
                edits: vec![DocumentEditOperation::Replace {
                    block_id: "block-2".into(),
                    content: "edited".into(),
                }],
            })
            .unwrap();
        service
            .execute(DocumentRequest::Convert {
                source: "note.docx".into(),
                path: "note.md".into(),
            })
            .unwrap();
        let markdown = fs::read_to_string(root.join("note.md")).unwrap();
        assert!(markdown.contains("# Title"));
        assert!(markdown.contains("edited"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn pdf_is_read_only_searchable_and_convertible_to_text() {
        let (root, service) = fixture();
        fs::write(root.join("note.pdf"), simple_pdf("PDF_SEARCH_NEEDLE")).unwrap();
        let search = service
            .execute(DocumentRequest::Search {
                path: "note.pdf".into(),
                query: "PDF_SEARCH_NEEDLE".into(),
                case_sensitive: true,
                max_results: 10,
            })
            .unwrap();
        assert!(matches!(
            search,
            DocumentResult::Search { matches, .. } if matches.len() == 1
        ));
        service
            .execute(DocumentRequest::Convert {
                source: "note.pdf".into(),
                path: "note.txt".into(),
            })
            .unwrap();
        assert!(
            fs::read_to_string(root.join("note.txt"))
                .unwrap()
                .contains("PDF_SEARCH_NEEDLE")
        );
        let sha256 = service.filesystem.hash("note.pdf").unwrap().sha256;
        assert_eq!(
            service
                .execute(DocumentRequest::Edit {
                    path: "note.pdf".into(),
                    expected_sha256: sha256,
                    edits: vec![DocumentEditOperation::Delete {
                        block_id: "block-1".into(),
                    }],
                })
                .unwrap_err(),
            DocumentError::UnsupportedFormat
        );
        fs::remove_dir_all(root).unwrap();
    }
}
