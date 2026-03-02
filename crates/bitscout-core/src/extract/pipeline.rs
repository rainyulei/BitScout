use std::path::Path;

/// Detected file type based on magic bytes and extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    PlainText,
    Gzip,
    Zip,
    Docx,
    Xlsx,
    Pdf,
    Unknown,
}

impl FileType {
    /// Detect file type using magic bytes (priority) and file extension (fallback).
    pub fn detect(filename: &str, header: &[u8]) -> Self {
        // Magic bytes take priority
        if header.len() >= 2 && header[0] == 0x1f && header[1] == 0x8b {
            return Self::Gzip;
        }
        if header.len() >= 4 && header[0..4] == [0x50, 0x4b, 0x03, 0x04] {
            // ZIP-based: differentiate by extension
            let lower = filename.to_lowercase();
            if lower.ends_with(".docx") {
                return Self::Docx;
            }
            if lower.ends_with(".xlsx") {
                return Self::Xlsx;
            }
            return Self::Zip;
        }
        if header.len() >= 5 && &header[0..5] == b"%PDF-" {
            return Self::Pdf;
        }

        // Extension-based detection for text files
        let lower = filename.to_lowercase();
        let text_extensions = [
            ".rs", ".py", ".js", ".ts", ".jsx", ".tsx", ".java", ".c", ".cpp", ".h",
            ".go", ".rb", ".php", ".swift", ".kt", ".scala", ".sh", ".bash", ".zsh",
            ".txt", ".md", ".markdown", ".rst", ".org", ".adoc",
            ".json", ".yaml", ".yml", ".toml", ".xml", ".html", ".htm", ".css",
            ".sql", ".graphql", ".proto", ".csv", ".tsv", ".ini", ".cfg", ".conf",
            ".env", ".gitignore", ".dockerignore", ".editorconfig",
            ".makefile", ".cmake", ".gradle", ".sbt",
            ".r", ".m", ".pl", ".lua", ".vim", ".el", ".clj", ".ex", ".exs",
            ".hs", ".ml", ".fs", ".v", ".sv", ".vhd",
        ];

        // No extension or known text extension → PlainText
        // Check if content looks like valid UTF-8 text
        if text_extensions.iter().any(|ext| lower.ends_with(ext)) {
            return Self::PlainText;
        }

        // Files without extension: check if valid UTF-8
        if std::str::from_utf8(header).is_ok() {
            // Check common text filenames without extensions
            let basename = lower.rsplit('/').next().unwrap_or(&lower);
            let text_filenames = [
                "makefile", "dockerfile", "vagrantfile", "gemfile",
                "rakefile", "procfile", "license", "readme", "changelog",
            ];
            if text_filenames.iter().any(|n| basename == *n) {
                return Self::PlainText;
            }
            // If header is valid UTF-8 and no NUL bytes, assume text
            if !header.contains(&0x00) {
                return Self::PlainText;
            }
        }

        Self::Unknown
    }
}

/// Extract searchable text content from a file.
/// Returns the extracted text, or an error if extraction fails.
pub fn extract_text(path: &Path) -> Result<String, crate::Error> {
    let mmap = crate::extract::text::MmapContent::open(path)?;
    let bytes = mmap.as_bytes();

    if bytes.is_empty() {
        return Ok(String::new());
    }

    let filename = path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let header = &bytes[..bytes.len().min(16)];
    let file_type = FileType::detect(&filename, header);

    match file_type {
        FileType::PlainText => {
            std::str::from_utf8(bytes)
                .map(|s| s.to_string())
                .map_err(|e| crate::Error::Extract(format!("UTF-8 decode error: {e}")))
        }
        FileType::Unknown => {
            Err(crate::Error::Extract("unsupported file type".into()))
        }
        FileType::Gzip => {
            crate::extract::gz::decompress_gz(bytes)
        }
        FileType::Zip => {
            crate::extract::zip_extract::extract_zip(bytes)
        }
        FileType::Docx => {
            crate::extract::docx::extract_docx(bytes)
        }
        FileType::Xlsx => {
            crate::extract::xlsx::extract_xlsx(bytes)
        }
        // Pdf — will be implemented in subsequent tasks
        other => {
            Err(crate::Error::Extract(format!("{other:?} extraction not yet implemented")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_plain_text_by_extension() {
        assert_eq!(FileType::detect("hello.rs", b"fn main() {}"), FileType::PlainText);
        assert_eq!(FileType::detect("readme.md", b"# Title"), FileType::PlainText);
        assert_eq!(FileType::detect("data.json", b"{}"), FileType::PlainText);
    }

    #[test]
    fn test_detect_gzip_by_magic() {
        let gz_magic = &[0x1f, 0x8b, 0x08, 0x00];
        assert_eq!(FileType::detect("file.gz", gz_magic), FileType::Gzip);
        // Magic bytes take precedence even with wrong extension
        assert_eq!(FileType::detect("file.txt", gz_magic), FileType::Gzip);
    }

    #[test]
    fn test_detect_zip_by_magic() {
        let zip_magic = &[0x50, 0x4b, 0x03, 0x04, 0x00];
        assert_eq!(FileType::detect("archive.zip", zip_magic), FileType::Zip);
    }

    #[test]
    fn test_detect_docx_by_extension_and_zip_magic() {
        let zip_magic = &[0x50, 0x4b, 0x03, 0x04, 0x00];
        assert_eq!(FileType::detect("report.docx", zip_magic), FileType::Docx);
        assert_eq!(FileType::detect("data.xlsx", zip_magic), FileType::Xlsx);
    }

    #[test]
    fn test_detect_pdf_by_magic() {
        let pdf_magic = b"%PDF-1.7";
        assert_eq!(FileType::detect("paper.pdf", pdf_magic), FileType::Pdf);
        assert_eq!(FileType::detect("paper.txt", pdf_magic), FileType::Pdf);
    }

    #[test]
    fn test_detect_unknown_binary() {
        let random_bytes = &[0x00, 0x01, 0x02, 0xff];
        assert_eq!(FileType::detect("unknown.bin", random_bytes), FileType::Unknown);
    }

    #[test]
    fn test_extract_text_returns_string_for_plain_text() {
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut tmp = NamedTempFile::with_suffix(".txt").unwrap();
        tmp.write_all(b"hello world").unwrap();
        tmp.flush().unwrap();

        let result = extract_text(tmp.path()).unwrap();
        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_extract_text_from_gzip_file() {
        use flate2::write::GzEncoder;
        use flate2::Compression;
        use std::io::Write;
        use tempfile::NamedTempFile;

        let original = "search me inside gzip";
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(original.as_bytes()).unwrap();
        let compressed = encoder.finish().unwrap();

        let mut tmp = NamedTempFile::with_suffix(".gz").unwrap();
        tmp.write_all(&compressed).unwrap();
        tmp.flush().unwrap();

        let result = extract_text(tmp.path()).unwrap();
        assert_eq!(result, original);
    }
}
