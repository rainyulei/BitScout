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

    pub fn score(&self, tf: usize, doc_len: usize, df: usize) -> f64 {
        let n = self.total_docs as f64;
        let df = df as f64;
        let tf = tf as f64;
        let doc_len = doc_len as f64;

        // IDF = ln((N - df + 0.5) / (df + 0.5) + 1.0)
        let idf = ((n - df + 0.5) / (df + 0.5) + 1.0).ln();

        // TF_norm = (tf * (k1 + 1)) / (tf + k1 * (1 - b + b * doc_len / avg_doc_len))
        let tf_norm =
            (tf * (self.k1 + 1.0)) / (tf + self.k1 * (1.0 - self.b + self.b * doc_len / self.avg_doc_len));

        idf * tf_norm
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
}
