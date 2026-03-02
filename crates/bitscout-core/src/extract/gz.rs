use flate2::read::GzDecoder;
use std::io::Read;

/// Decompress gzip-compressed bytes and return the content as a UTF-8 string.
pub fn decompress_gz(data: &[u8]) -> Result<String, crate::Error> {
    let mut decoder = GzDecoder::new(data);
    let mut output = String::new();
    decoder
        .read_to_string(&mut output)
        .map_err(|e| crate::Error::Extract(format!("gzip decompression failed: {e}")))?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Write;

    #[test]
    fn test_decompress_gz_bytes() {
        let original = b"fn main() { println!(\"hello from gzip\"); }";
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(original).unwrap();
        let compressed = encoder.finish().unwrap();

        let result = decompress_gz(&compressed).unwrap();
        assert_eq!(result, String::from_utf8_lossy(original));
    }

    #[test]
    fn test_decompress_gz_multiline() {
        let original = b"line one\nline two\nline three\n";
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(original).unwrap();
        let compressed = encoder.finish().unwrap();

        let result = decompress_gz(&compressed).unwrap();
        assert!(result.contains("line two"));
    }

    #[test]
    fn test_decompress_invalid_gz() {
        let not_gz = b"this is not gzip data";
        let result = decompress_gz(not_gz);
        assert!(result.is_err());
    }
}
