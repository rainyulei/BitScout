use std::io::Read;

/// Extract all text content from a ZIP archive.
/// Each text file's content is concatenated with a header line showing the entry path.
/// Binary entries (containing NUL bytes) are skipped.
pub fn extract_zip(data: &[u8]) -> Result<String, crate::Error> {
    let cursor = std::io::Cursor::new(data);
    let mut archive = zip::ZipArchive::new(cursor)
        .map_err(|e| crate::Error::Extract(format!("ZIP open failed: {e}")))?;

    let mut output = String::new();

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| crate::Error::Extract(format!("ZIP entry {i} error: {e}")))?;

        // Skip directories
        if entry.is_dir() {
            continue;
        }

        // Read entry content
        let mut buf = Vec::new();
        if entry.read_to_end(&mut buf).is_err() {
            continue;
        }

        // Skip binary content (contains NUL bytes)
        if buf.contains(&0x00) {
            continue;
        }

        // Try to decode as UTF-8
        if let Ok(text) = std::str::from_utf8(&buf) {
            if !output.is_empty() {
                output.push('\n');
            }
            // Add file path header so search results can reference the entry
            output.push_str(&format!("--- {} ---\n", entry.name()));
            output.push_str(text);
        }
    }

    if output.is_empty() {
        return Err(crate::Error::Extract("ZIP contains no text content".into()));
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_test_zip(files: &[(&str, &[u8])]) -> Vec<u8> {
        let buf = Vec::new();
        let cursor = std::io::Cursor::new(buf);
        let mut zip_writer = zip::ZipWriter::new(cursor);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

        for (name, content) in files {
            zip_writer.start_file(*name, options).unwrap();
            zip_writer.write_all(content).unwrap();
        }

        zip_writer.finish().unwrap().into_inner()
    }

    #[test]
    fn test_extract_zip_single_text_file() {
        let zip_data = make_test_zip(&[("hello.txt", b"hello world")]);
        let result = extract_zip(&zip_data).unwrap();
        assert!(result.contains("hello world"));
    }

    #[test]
    fn test_extract_zip_multiple_files() {
        let zip_data = make_test_zip(&[
            ("src/main.rs", b"fn main() {}"),
            ("src/lib.rs", b"pub fn greet() {}"),
            ("README.md", b"# My Project"),
        ]);
        let result = extract_zip(&zip_data).unwrap();
        assert!(result.contains("fn main()"));
        assert!(result.contains("pub fn greet()"));
        assert!(result.contains("# My Project"));
    }

    #[test]
    fn test_extract_zip_skips_binary_entries() {
        let zip_data = make_test_zip(&[
            ("code.rs", b"fn search_me() {}"),
            ("image.png", &[0x89, 0x50, 0x4e, 0x47, 0x00, 0xff]),
        ]);
        let result = extract_zip(&zip_data).unwrap();
        assert!(result.contains("fn search_me()"));
        // Binary content should not appear as text
    }

    #[test]
    fn test_extract_zip_skips_directories() {
        let zip_data = make_test_zip(&[("src/main.rs", b"fn main() {}")]);
        let result = extract_zip(&zip_data).unwrap();
        assert!(result.contains("fn main()"));
    }

    #[test]
    fn test_extract_invalid_zip() {
        let result = extract_zip(b"not a zip file");
        assert!(result.is_err());
    }
}
