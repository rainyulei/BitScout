/// BM25 scoring mode
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Bm25Mode {
    #[default]
    Off,
    /// TF-normalized only (no IDF), streaming compatible
    Tf,
    /// Full BM25 with IDF, requires buffering
    Full,
}

pub struct Bm25Scorer {
    total_docs: usize,
    avg_doc_len: f64,
    k1: f64,
    b: f64,
}

impl Bm25Scorer {
    pub fn new(total_docs: usize, avg_doc_len: f64) -> Self {
        Self {
            total_docs,
            avg_doc_len,
            k1: 1.2,
            b: 0.75,
        }
    }

    /// TF-normalized score without IDF. Streaming-compatible.
    /// Returns score in range (0, k1+1) = (0, 2.2) with default params.
    pub fn tf_score(&self, tf: usize, doc_len: usize) -> f64 {
        let tf = tf as f64;
        let doc_len = doc_len as f64;
        (tf * (self.k1 + 1.0)) / (tf + self.k1 * (1.0 - self.b + self.b * doc_len / self.avg_doc_len))
    }

    /// Full BM25 score with IDF component.
    pub fn score(&self, tf: usize, doc_len: usize, df: usize) -> f64 {
        let n = self.total_docs as f64;
        let df = df as f64;

        // IDF = ln((N - df + 0.5) / (df + 0.5) + 1.0)
        let idf = ((n - df + 0.5) / (df + 0.5) + 1.0).ln();

        idf * self.tf_score(tf, doc_len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bm25_score_higher_for_relevant_doc() {
        let scorer = Bm25Scorer::new(100, 50.0);
        let high = scorer.score(3, 40, 10);
        let low = scorer.score(1, 100, 10);
        assert!(high > low, "3 hits in 40 tokens ({high}) should score higher than 1 hit in 100 tokens ({low})");
    }

    #[test]
    fn test_bm25_rare_term_scores_higher() {
        let scorer = Bm25Scorer::new(100, 50.0);
        let rare = scorer.score(2, 50, 1);
        let common = scorer.score(2, 50, 90);
        assert!(rare > common, "df=1 ({rare}) should score higher than df=90 ({common})");
    }

    #[test]
    fn test_tf_score_density() {
        let scorer = Bm25Scorer::new(100, 50.0);
        let dense = scorer.tf_score(3, 40);
        let sparse = scorer.tf_score(1, 100);
        assert!(dense > sparse, "dense ({dense}) should > sparse ({sparse})");
    }

    #[test]
    fn test_tf_score_range() {
        let scorer = Bm25Scorer::new(100, 50.0);
        let score = scorer.tf_score(5, 50);
        // TF-normalized score should be in (0, k1+1) range = (0, 2.2)
        assert!(score > 0.0);
        assert!(score < 2.2);
    }

    #[test]
    fn test_full_score_uses_tf_score() {
        // Full BM25 = IDF * tf_score, so refactoring should keep results identical
        let scorer = Bm25Scorer::new(100, 50.0);
        let full = scorer.score(3, 40, 10);
        let tf = scorer.tf_score(3, 40);
        // IDF for df=10, N=100
        let idf = ((100.0 - 10.0 + 0.5) / (10.0 + 0.5) + 1.0_f64).ln();
        let expected = idf * tf;
        assert!((full - expected).abs() < 1e-10, "full ({full}) should equal idf*tf ({expected})");
    }
}
