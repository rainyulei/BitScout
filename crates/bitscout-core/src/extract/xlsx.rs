use quick_xml::events::Event;
use quick_xml::Reader;
use std::io::Read;

/// Extract text from an XLSX file by parsing xl/sharedStrings.xml.
pub fn extract_xlsx(data: &[u8]) -> Result<String, crate::Error> {
    let cursor = std::io::Cursor::new(data);
    let mut archive = zip::ZipArchive::new(cursor)
        .map_err(|e| crate::Error::Extract(format!("XLSX ZIP open failed: {e}")))?;

    let mut shared_strings = archive.by_name("xl/sharedStrings.xml")
        .map_err(|_| crate::Error::Extract("xl/sharedStrings.xml not found in XLSX".into()))?;

    let mut xml_buf = Vec::new();
    shared_strings.read_to_end(&mut xml_buf)
        .map_err(|e| crate::Error::Extract(format!("XLSX read error: {e}")))?;

    parse_shared_strings(&xml_buf)
}

fn parse_shared_strings(xml: &[u8]) -> Result<String, crate::Error> {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(false);

    let mut output = String::new();
    let mut buf = Vec::new();
    let mut in_t = false;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) if e.local_name().as_ref() == b"t" => {
                in_t = true;
            }
            Ok(Event::Text(ref e)) if in_t => {
                if let Ok(text) = e.unescape() {
                    if !output.is_empty() {
                        output.push('\n');
                    }
                    output.push_str(&text);
                }
            }
            Ok(Event::End(ref e)) if e.local_name().as_ref() == b"t" => {
                in_t = false;
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(crate::Error::Extract(format!("XML parse error: {e}"))),
            _ => {}
        }
        buf.clear();
    }

    if output.is_empty() {
        return Err(crate::Error::Extract("No text content found in XLSX".into()));
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_test_xlsx(strings: &[&str]) -> Vec<u8> {
        let buf = Vec::new();
        let cursor = std::io::Cursor::new(buf);
        let mut zip_writer = zip::ZipWriter::new(cursor);
        let options = zip::write::SimpleFileOptions::default();

        // Build sharedStrings.xml
        let mut xml = String::from(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">"#,
        );
        for s in strings {
            xml.push_str(&format!("<si><t>{}</t></si>", s));
        }
        xml.push_str("</sst>");

        zip_writer.start_file("xl/sharedStrings.xml", options).unwrap();
        zip_writer.write_all(xml.as_bytes()).unwrap();

        zip_writer.finish().unwrap().into_inner()
    }

    #[test]
    fn test_extract_xlsx_strings() {
        let xlsx = make_test_xlsx(&["Revenue", "Expenses", "Q1 2026"]);
        let result = extract_xlsx(&xlsx).unwrap();
        assert!(result.contains("Revenue"));
        assert!(result.contains("Expenses"));
        assert!(result.contains("Q1 2026"));
    }

    #[test]
    fn test_extract_xlsx_no_shared_strings() {
        let buf = Vec::new();
        let cursor = std::io::Cursor::new(buf);
        let mut zip_writer = zip::ZipWriter::new(cursor);
        let options = zip::write::SimpleFileOptions::default();
        zip_writer.start_file("xl/workbook.xml", options).unwrap();
        zip_writer.write_all(b"<workbook/>").unwrap();
        let data = zip_writer.finish().unwrap().into_inner();

        let result = extract_xlsx(&data);
        assert!(result.is_err());
    }
}
