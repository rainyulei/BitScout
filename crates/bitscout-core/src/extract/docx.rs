use quick_xml::events::Event;
use quick_xml::Reader;
use std::io::Read;

/// Extract text from a DOCX file (ZIP containing word/document.xml).
/// Parses XML and concatenates all <w:t> text nodes, separating paragraphs with newlines.
pub fn extract_docx(data: &[u8]) -> Result<String, crate::Error> {
    let cursor = std::io::Cursor::new(data);
    let mut archive = zip::ZipArchive::new(cursor)
        .map_err(|e| crate::Error::Extract(format!("DOCX ZIP open failed: {e}")))?;

    // Find word/document.xml
    let mut doc_xml = archive.by_name("word/document.xml")
        .map_err(|_| crate::Error::Extract("word/document.xml not found in DOCX".into()))?;

    let mut xml_buf = Vec::new();
    doc_xml.read_to_end(&mut xml_buf)
        .map_err(|e| crate::Error::Extract(format!("DOCX read error: {e}")))?;

    parse_docx_xml(&xml_buf)
}

/// Parse DOCX XML and extract text from <w:t> elements.
fn parse_docx_xml(xml: &[u8]) -> Result<String, crate::Error> {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(false);

    let mut output = String::new();
    let mut buf = Vec::new();
    let mut in_t = false; // Inside a <w:t> element
    let mut in_p = false; // Inside a <w:p> element
    let mut paragraph_text = String::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                let local_name = e.local_name();
                match local_name.as_ref() {
                    b"p" => {
                        in_p = true;
                        paragraph_text.clear();
                    }
                    b"t" if in_p => {
                        in_t = true;
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(ref e)) if in_t => {
                if let Ok(text) = e.unescape() {
                    paragraph_text.push_str(&text);
                }
            }
            Ok(Event::End(ref e)) => {
                let local_name = e.local_name();
                match local_name.as_ref() {
                    b"t" => {
                        in_t = false;
                    }
                    b"p" => {
                        in_p = false;
                        if !paragraph_text.is_empty() {
                            if !output.is_empty() {
                                output.push('\n');
                            }
                            output.push_str(&paragraph_text);
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(crate::Error::Extract(format!("XML parse error: {e}")));
            }
            _ => {}
        }
        buf.clear();
    }

    if output.is_empty() {
        return Err(crate::Error::Extract("No text content found in DOCX".into()));
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Build a minimal .docx file (ZIP containing word/document.xml).
    fn make_test_docx(paragraphs: &[&str]) -> Vec<u8> {
        let buf = Vec::new();
        let cursor = std::io::Cursor::new(buf);
        let mut zip_writer = zip::ZipWriter::new(cursor);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

        // Build document.xml
        let mut xml = String::from(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:body>"#,
        );
        for para in paragraphs {
            xml.push_str(&format!(
                r#"<w:p><w:r><w:t>{}</w:t></w:r></w:p>"#,
                para
            ));
        }
        xml.push_str("</w:body></w:document>");

        zip_writer.start_file("word/document.xml", options).unwrap();
        zip_writer.write_all(xml.as_bytes()).unwrap();

        // Add [Content_Types].xml (required for valid DOCX)
        zip_writer.start_file("[Content_Types].xml", options).unwrap();
        zip_writer.write_all(br#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"></Types>"#).unwrap();

        zip_writer.finish().unwrap().into_inner()
    }

    #[test]
    fn test_extract_docx_single_paragraph() {
        let docx = make_test_docx(&["Hello from DOCX"]);
        let result = extract_docx(&docx).unwrap();
        assert!(result.contains("Hello from DOCX"), "got: {result}");
    }

    #[test]
    fn test_extract_docx_multiple_paragraphs() {
        let docx = make_test_docx(&["First paragraph", "Second paragraph", "Third"]);
        let result = extract_docx(&docx).unwrap();
        assert!(result.contains("First paragraph"));
        assert!(result.contains("Second paragraph"));
        assert!(result.contains("Third"));
    }

    #[test]
    fn test_extract_docx_split_runs() {
        // Simulate text split across multiple <w:r><w:t> within a single <w:p>
        let buf = Vec::new();
        let cursor = std::io::Cursor::new(buf);
        let mut zip_writer = zip::ZipWriter::new(cursor);
        let options = zip::write::SimpleFileOptions::default();

        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:body>
<w:p><w:r><w:t>Hello </w:t></w:r><w:r><w:rPr><w:b/></w:rPr><w:t>World</w:t></w:r></w:p>
</w:body></w:document>"#;

        zip_writer.start_file("word/document.xml", options).unwrap();
        zip_writer.write_all(xml.as_bytes()).unwrap();
        let docx_data = zip_writer.finish().unwrap().into_inner();

        let result = extract_docx(&docx_data).unwrap();
        assert!(result.contains("Hello World") || result.contains("Hello  World"),
            "Expected concatenated text, got: {result}");
    }

    #[test]
    fn test_extract_docx_no_document_xml() {
        let buf = Vec::new();
        let cursor = std::io::Cursor::new(buf);
        let mut zip_writer = zip::ZipWriter::new(cursor);
        let options = zip::write::SimpleFileOptions::default();
        zip_writer.start_file("other.xml", options).unwrap();
        zip_writer.write_all(b"<data/>").unwrap();
        let data = zip_writer.finish().unwrap().into_inner();

        let result = extract_docx(&data);
        assert!(result.is_err());
    }
}
