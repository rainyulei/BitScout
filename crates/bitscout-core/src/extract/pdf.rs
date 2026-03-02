/// Extract text from PDF bytes using pdf-extract.
pub fn extract_pdf(data: &[u8]) -> Result<String, crate::Error> {
    if data.is_empty() {
        return Err(crate::Error::Extract("empty PDF data".into()));
    }

    let text = pdf_extract::extract_text_from_mem(data)
        .map_err(|e| crate::Error::Extract(format!("PDF extraction failed: {e}")))?;

    if text.trim().is_empty() {
        return Err(crate::Error::Extract("PDF contains no extractable text".into()));
    }

    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_pdf_returns_error_for_invalid_data() {
        let result = extract_pdf(b"not a pdf");
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_pdf_returns_error_for_empty_data() {
        let result = extract_pdf(b"");
        assert!(result.is_err());
    }

    // NOTE: Testing with real PDFs requires creating valid PDF bytes programmatically,
    // which is complex. Real PDF extraction is verified in E2E tests with sample files.
    // Here we verify the error handling path and that the module compiles correctly.
}
