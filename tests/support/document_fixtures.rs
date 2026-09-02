use std::io::{Cursor, Read, Write};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

pub(crate) fn rich_docx() -> Vec<u8> {
    let cursor = Cursor::new(Vec::new());
    let mut writer = ZipWriter::new(cursor);
    writer.set_comment("KEEP_ARCHIVE_COMMENT");
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, content) in [
        (
            "[Content_Types].xml",
            r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="officeDocument" Target="word/document.xml"/></Relationships>"#,
        ),
        (
            "word/document.xml",
            r#"<?xml version="1.0" encoding="UTF-8"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body><w:p><w:r><w:t>old</w:t></w:r></w:p><w:p><w:r><w:rPr><w:b/></w:rPr><w:t>bold survives</w:t></w:r></w:p><w:p><w:hyperlink r:id="rId7"><w:r><w:t>linked text</w:t></w:r></w:hyperlink></w:p><w:sectPr/></w:body></w:document>"#,
        ),
        (
            "word/_rels/document.xml.rels",
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId7" Type="hyperlink" Target="https://example.invalid" TargetMode="External"/></Relationships>"#,
        ),
        (
            "docProps/custom.xml",
            "<custom>KEEP_CUSTOM_METADATA</custom>",
        ),
    ] {
        writer.start_file(name, options).unwrap();
        writer.write_all(content.as_bytes()).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

pub(crate) fn zip_entry(bytes: &[u8], name: &str) -> Vec<u8> {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut entry = archive.by_name(name).unwrap();
    let mut content = Vec::new();
    entry.read_to_end(&mut content).unwrap();
    content
}
