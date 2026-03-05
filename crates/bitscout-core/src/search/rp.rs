//! Random Projection (RP) semantic scoring.
//!
//! Uses the Johnson-Lindenstrauss lemma: random projections preserve
//! vector distances with high probability. This enables embedding-free
//! semantic similarity via SIMD-accelerated matrix multiplication.

use std::collections::HashMap;

use super::simd;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum vocabulary size before feature-hashing fallback.
const VOCAB_SIZE: usize = 100_000;

/// Projection dimensionality (output vector size).
const PROJ_DIM: usize = 256;

/// Fixed PRNG seed for reproducible projections.
const SEED: u64 = 0xB175C007_2026_0001;

// ---------------------------------------------------------------------------
// xorshift128+ PRNG
// ---------------------------------------------------------------------------

struct Xorshift128Plus {
    s0: u64,
    s1: u64,
}

impl Xorshift128Plus {
    fn new(seed: u64) -> Self {
        // Split seed into two non-zero states
        let s0 = seed | 1;
        let s1 = (seed.wrapping_mul(6364136223846793005)).wrapping_add(1442695040888963407) | 1;
        Self { s0, s1 }
    }

    fn next_u64(&mut self) -> u64 {
        let mut s1 = self.s0;
        let s0 = self.s1;
        self.s0 = s0;
        s1 ^= s1 << 23;
        s1 ^= s1 >> 17;
        s1 ^= s0;
        s1 ^= s0 >> 26;
        self.s1 = s1;
        s0.wrapping_add(s1)
    }

    /// Generate a uniform f64 in (0, 1).
    fn next_f64(&mut self) -> f64 {
        // Use top 53 bits for full mantissa precision
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Generate a standard normal sample via Box-Muller transform.
    fn next_gaussian(&mut self) -> f32 {
        let u1 = self.next_f64().max(1e-15); // avoid log(0)
        let u2 = self.next_f64();
        let z = (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos();
        z as f32
    }
}

// ---------------------------------------------------------------------------
// Vocabulary
// ---------------------------------------------------------------------------

/// Dynamic token-to-index vocabulary with feature-hashing fallback.
pub struct Vocabulary {
    map: HashMap<String, u32>,
    next_id: u32,
}

impl Vocabulary {
    pub fn new() -> Self {
        Self {
            map: HashMap::with_capacity(4096),
            next_id: 0,
        }
    }

    /// Get or assign an index for a token.
    /// Falls back to feature hashing when vocabulary is full.
    pub fn get_or_insert(&mut self, token: &str) -> u32 {
        if let Some(&id) = self.map.get(token) {
            return id;
        }

        if (self.next_id as usize) < VOCAB_SIZE {
            let id = self.next_id;
            self.map.insert(token.to_string(), id);
            self.next_id += 1;
            id
        } else {
            // Feature hashing fallback
            feature_hash(token)
        }
    }

    /// Look up a token without inserting.
    pub fn get(&self, token: &str) -> Option<u32> {
        self.map.get(token).copied()
    }

    pub fn len(&self) -> usize {
        self.next_id as usize
    }
}

/// FNV-1a hash mod VOCAB_SIZE for feature hashing fallback.
fn feature_hash(token: &str) -> u32 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in token.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    (hash % VOCAB_SIZE as u64) as u32
}

// ---------------------------------------------------------------------------
// Projection Matrix
// ---------------------------------------------------------------------------

/// Gaussian random projection matrix (vocab_size × proj_dim).
///
/// Stored row-major: row `i` is the projection vector for token `i`.
/// Generated lazily on first access with a fixed seed for reproducibility.
pub struct ProjectionMatrix {
    /// Flat storage: rows[i * PROJ_DIM .. (i+1) * PROJ_DIM]
    data: Vec<f32>,
    /// Number of rows currently generated.
    generated_rows: usize,
    /// PRNG state (continues from last generated row).
    rng: Xorshift128Plus,
}

impl ProjectionMatrix {
    pub fn new() -> Self {
        Self {
            data: Vec::new(),
            generated_rows: 0,
            rng: Xorshift128Plus::new(SEED),
        }
    }

    /// Get the projection row for token index `idx`.
    /// Generates rows on demand up to `idx`.
    pub fn row(&mut self, idx: u32) -> &[f32] {
        let idx = idx as usize;
        if idx >= VOCAB_SIZE {
            // Should not happen with proper vocabulary, but handle gracefully
            // Return a zero-filled slice by ensuring we have at least one extra row
            self.ensure_rows(VOCAB_SIZE);
            return &self.data[0..PROJ_DIM]; // degenerate, but safe
        }
        self.ensure_rows(idx + 1);
        &self.data[idx * PROJ_DIM..(idx + 1) * PROJ_DIM]
    }

    fn ensure_rows(&mut self, needed: usize) {
        if self.generated_rows >= needed {
            return;
        }
        let new_elements = (needed - self.generated_rows) * PROJ_DIM;
        self.data.reserve(new_elements);
        // Normalization factor: 1/sqrt(proj_dim) as per JL lemma
        let scale = 1.0 / (PROJ_DIM as f32).sqrt();
        for _ in self.generated_rows..needed {
            for _ in 0..PROJ_DIM {
                self.data.push(self.rng.next_gaussian() * scale);
            }
        }
        self.generated_rows = needed;
    }
}

// ---------------------------------------------------------------------------
// Tokenizer
// ---------------------------------------------------------------------------

/// Simple whitespace + punctuation tokenizer.
/// Splits on whitespace, strips punctuation, lowercases.
fn tokenize(text: &str) -> Vec<&str> {
    text.split(|c: char| c.is_whitespace() || c.is_ascii_punctuation())
        .filter(|s| !s.is_empty())
        .collect()
}

// ---------------------------------------------------------------------------
// RpScorer
// ---------------------------------------------------------------------------

/// Random Projection scorer for semantic similarity.
pub struct RpScorer {
    vocab: Vocabulary,
    matrix: ProjectionMatrix,
}

impl RpScorer {
    pub fn new() -> Self {
        Self {
            vocab: Vocabulary::new(),
            matrix: ProjectionMatrix::new(),
        }
    }

    /// Project a text into the RP space.
    /// Returns a dense vector of length PROJ_DIM.
    pub fn project(&mut self, text: &str) -> Vec<f32> {
        let mut result = vec![0.0f32; PROJ_DIM];
        let tokens = tokenize(text);

        // Count term frequencies
        let mut tf: HashMap<u32, u32> = HashMap::new();
        for token in &tokens {
            let lower = token.to_lowercase();
            let idx = self.vocab.get_or_insert(&lower);
            *tf.entry(idx).or_insert(0) += 1;
        }

        // Weighted accumulation: result += tf * projection_row
        for (&token_idx, &count) in &tf {
            let row = self.matrix.row(token_idx).to_vec(); // copy to avoid borrow conflict
            simd::weighted_accumulate(&mut result, &row, count as f32);
        }

        result
    }

    /// Compute cosine similarity between a projected query and a document text.
    pub fn score(&mut self, query_proj: &[f32], doc_text: &str) -> f32 {
        let doc_proj = self.project(doc_text);
        cosine_similarity(query_proj, &doc_proj)
    }

    /// Project dimension (for external use).
    pub fn proj_dim() -> usize {
        PROJ_DIM
    }
}

/// Cosine similarity using SIMD-accelerated dot product and norm.
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot = simd::dot_product(a, b);
    let norm_a = simd::norm_sq(a).sqrt();
    let norm_b = simd::norm_sq(b).sqrt();
    let denom = norm_a * norm_b;
    if denom < 1e-10 {
        0.0
    } else {
        dot / denom
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_xorshift_deterministic() {
        let mut rng1 = Xorshift128Plus::new(42);
        let mut rng2 = Xorshift128Plus::new(42);
        for _ in 0..100 {
            assert_eq!(rng1.next_u64(), rng2.next_u64());
        }
    }

    #[test]
    fn test_gaussian_distribution() {
        let mut rng = Xorshift128Plus::new(12345);
        let n = 10_000;
        let samples: Vec<f32> = (0..n).map(|_| rng.next_gaussian()).collect();
        let mean = samples.iter().sum::<f32>() / n as f32;
        let variance = samples.iter().map(|x| (x - mean).powi(2)).sum::<f32>() / n as f32;

        // Mean should be close to 0, variance close to 1
        assert!(mean.abs() < 0.05, "mean = {}", mean);
        assert!((variance - 1.0).abs() < 0.1, "variance = {}", variance);
    }

    #[test]
    fn test_vocabulary_basic() {
        let mut vocab = Vocabulary::new();
        let id1 = vocab.get_or_insert("hello");
        let id2 = vocab.get_or_insert("world");
        let id3 = vocab.get_or_insert("hello");
        assert_eq!(id1, id3); // same token => same id
        assert_ne!(id1, id2); // different tokens => different ids
    }

    #[test]
    fn test_feature_hash_consistency() {
        let h1 = feature_hash("overflow_token");
        let h2 = feature_hash("overflow_token");
        assert_eq!(h1, h2);
        assert!((h1 as usize) < VOCAB_SIZE);
    }

    #[test]
    fn test_projection_matrix_deterministic() {
        let mut m1 = ProjectionMatrix::new();
        let mut m2 = ProjectionMatrix::new();
        let row1 = m1.row(5).to_vec();
        let row2 = m2.row(5).to_vec();
        assert_eq!(row1, row2);
    }

    #[test]
    fn test_projection_matrix_row_length() {
        let mut m = ProjectionMatrix::new();
        let row = m.row(0);
        assert_eq!(row.len(), PROJ_DIM);
    }

    #[test]
    fn test_tokenize() {
        let tokens = tokenize("Hello, world! This is a test.");
        assert_eq!(tokens, vec!["Hello", "world", "This", "is", "a", "test"]);
    }

    #[test]
    fn test_project_produces_nonzero_vector() {
        let mut scorer = RpScorer::new();
        let proj = scorer.project("hello world");
        assert_eq!(proj.len(), PROJ_DIM);
        // Should not be all zeros
        assert!(proj.iter().any(|&x| x != 0.0));
    }

    #[test]
    fn test_cosine_self_similarity() {
        let mut scorer = RpScorer::new();
        let proj = scorer.project("authentication token validation");
        let sim = cosine_similarity(&proj, &proj);
        assert!((sim - 1.0).abs() < 1e-5, "self-similarity should be ~1.0, got {}", sim);
    }

    #[test]
    fn test_similar_texts_higher_score_than_dissimilar() {
        let mut scorer = RpScorer::new();

        let q = scorer.project("user authentication login flow");
        let similar = scorer.project("authenticate user credentials verify login");
        let dissimilar = scorer.project("database migration schema upgrade rollback");

        let sim_score = cosine_similarity(&q, &similar);
        let dis_score = cosine_similarity(&q, &dissimilar);

        assert!(
            sim_score > dis_score,
            "similar ({}) should score higher than dissimilar ({})",
            sim_score, dis_score
        );
    }

    #[test]
    fn test_score_method() {
        let mut scorer = RpScorer::new();
        let q = scorer.project("error handling");
        let score = scorer.score(&q, "error handling and recovery");
        assert!(score > 0.0, "related text should have positive score");
    }

    #[test]
    fn test_empty_text_returns_zero_vector() {
        let mut scorer = RpScorer::new();
        let proj = scorer.project("");
        assert!(proj.iter().all(|&x| x == 0.0));
    }

    #[test]
    fn test_cosine_with_zero_vector() {
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![0.0, 0.0, 0.0];
        assert_eq!(cosine_similarity(&a, &b), 0.0);
    }
}
