//! End-to-end tests for binary file format search support.
//! These tests verify that SearchEngine can find text inside gzip, zip, and docx files.

use bitscout_core::search::engine::{SearchEngine, SearchOptions};
use flate2::write::GzEncoder;
use flate2::Compression;
use std::fs;
use std::io::Write;
use tempfile::TempDir;

#[test]
fn test_search_finds_text_in_gzip() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    let code = b"fn authenticate_user(token: &str) -> bool { true }\n";
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(code).unwrap();
    let compressed = encoder.finish().unwrap();
    fs::write(root.join("auth.rs.gz"), &compressed).unwrap();

    let engine = SearchEngine::new(root).unwrap();
    let results = engine
        .search("authenticate_user", &SearchOptions::default())
        .unwrap();
    assert_eq!(results.len(), 1);
}

#[test]
fn test_search_finds_text_in_zip() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    let buf = Vec::new();
    let cursor = std::io::Cursor::new(buf);
    let mut zip_writer = zip::ZipWriter::new(cursor);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    zip_writer.start_file("src/main.rs", options).unwrap();
    zip_writer
        .write_all(b"fn unique_function_name() {}\n")
        .unwrap();
    let zip_data = zip_writer.finish().unwrap().into_inner();

    fs::write(root.join("source.zip"), &zip_data).unwrap();

    let engine = SearchEngine::new(root).unwrap();
    let results = engine
        .search("unique_function_name", &SearchOptions::default())
        .unwrap();
    assert_eq!(results.len(), 1);
}

#[test]
fn test_search_finds_text_in_docx() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    // Build minimal DOCX (a ZIP containing word/document.xml)
    let buf = Vec::new();
    let cursor = std::io::Cursor::new(buf);
    let mut zip_writer = zip::ZipWriter::new(cursor);
    let options = zip::write::SimpleFileOptions::default();

    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:body><w:p><w:r><w:t>confidential_report_keyword</w:t></w:r></w:p></w:body>
</w:document>"#;

    zip_writer.start_file("word/document.xml", options).unwrap();
    zip_writer.write_all(xml.as_bytes()).unwrap();
    let docx_data = zip_writer.finish().unwrap().into_inner();

    fs::write(root.join("report.docx"), &docx_data).unwrap();

    let engine = SearchEngine::new(root).unwrap();
    let results = engine
        .search("confidential_report_keyword", &SearchOptions::default())
        .unwrap();
    assert_eq!(results.len(), 1);
}

#[test]
fn test_search_mixed_file_types() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    // Plain text
    fs::write(root.join("code.rs"), "fn search_target() {}\n").unwrap();

    // Gzip with same keyword
    let gz_content = b"// Also has search_target in gzip\n";
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(gz_content).unwrap();
    let compressed = encoder.finish().unwrap();
    fs::write(root.join("backup.gz"), &compressed).unwrap();

    let engine = SearchEngine::new(root).unwrap();
    let results = engine
        .search("search_target", &SearchOptions::default())
        .unwrap();
    assert_eq!(
        results.len(),
        2,
        "should find in both plain text and gzip: {:?}",
        results
    );
}
