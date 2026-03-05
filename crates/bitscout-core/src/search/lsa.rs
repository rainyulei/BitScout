//! LSA (Latent Semantic Analysis) semantic scoring.
//!
//! Extracts semantic relationships from term-document co-occurrence via
//! truncated SVD (power iteration). "login" and "auth" appearing in the
//! same files → their vectors converge → semantic similarity emerges.
//!
//! Pipeline: tokenize → TF-IDF sparse matrix → truncated SVD → cosine scoring.

use std::collections::HashMap;
use std::path::PathBuf;

use super::simd;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Fixed PRNG seed for reproducible SVD initialization.
const SEED: u64 = 0xB175_C007_2026_0002;

/// Maximum power iteration steps per singular component.
const DEFAULT_MAX_ITER: usize = 15;

// ---------------------------------------------------------------------------
// xorshift128+ PRNG (same as rp.rs, duplicated to keep modules independent)
// ---------------------------------------------------------------------------

struct Xorshift128Plus {
    s0: u64,
    s1: u64,
}

impl Xorshift128Plus {
    fn new(seed: u64) -> Self {
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

    /// Uniform f64 in (0, 1).
    fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Standard normal via Box-Muller.
    fn next_gaussian(&mut self) -> f32 {
        let u1 = self.next_f64().max(1e-15);
        let u2 = self.next_f64();
        let z = (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos();
        z as f32
    }
}

// ---------------------------------------------------------------------------
// Vocabulary
// ---------------------------------------------------------------------------

/// Token-to-index mapping built during corpus indexing.
pub struct Vocabulary {
    map: HashMap<String, u32>,
    next_id: u32,
}

impl Default for Vocabulary {
    fn default() -> Self {
        Self::new()
    }
}

impl Vocabulary {
    pub fn new() -> Self {
        Self {
            map: HashMap::with_capacity(4096),
            next_id: 0,
        }
    }

    pub fn get_or_insert(&mut self, token: &str) -> u32 {
        if let Some(&id) = self.map.get(token) {
            return id;
        }
        let id = self.next_id;
        self.map.insert(token.to_string(), id);
        self.next_id += 1;
        id
    }

    pub fn get(&self, token: &str) -> Option<u32> {
        self.map.get(token).copied()
    }

    pub fn len(&self) -> usize {
        self.next_id as usize
    }

    pub fn is_empty(&self) -> bool {
        self.next_id == 0
    }
}

// ---------------------------------------------------------------------------
// Tokenizer (improved: camelCase + snake_case splitting)
// ---------------------------------------------------------------------------

/// Tokenize text: split on whitespace/punctuation, then split camelCase and
/// snake_case into sub-tokens. All tokens are lowercased.
pub fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    for raw in text.split(|c: char| c.is_whitespace() || c.is_ascii_punctuation()) {
        if raw.is_empty() {
            continue;
        }
        // Split snake_case parts, then split each part on camelCase boundaries
        for snake_part in raw.split('_') {
            if snake_part.is_empty() {
                continue;
            }
            // Split camelCase: "getUserName" → ["get", "User", "Name"]
            let mut start = 0;
            let chars: Vec<char> = snake_part.chars().collect();
            for i in 1..chars.len() {
                if chars[i].is_uppercase() && !chars[i - 1].is_uppercase() {
                    let part: String = chars[start..i].iter().collect();
                    if !part.is_empty() {
                        tokens.push(part.to_lowercase());
                    }
                    start = i;
                }
                // Handle sequences like "HTMLParser" → ["HTML", "Parser"]
                if i + 1 < chars.len()
                    && chars[i].is_uppercase()
                    && chars[i - 1].is_uppercase()
                    && chars[i + 1].is_lowercase()
                {
                    let part: String = chars[start..i].iter().collect();
                    if !part.is_empty() {
                        tokens.push(part.to_lowercase());
                    }
                    start = i;
                }
            }
            let part: String = chars[start..].iter().collect();
            if !part.is_empty() {
                tokens.push(part.to_lowercase());
            }
        }
    }
    tokens
}

// ---------------------------------------------------------------------------
// Sparse Matrix (CSR format)
// ---------------------------------------------------------------------------

/// Compressed Sparse Row matrix. Rows = terms, Columns = documents.
pub struct SparseMatrix {
    pub row_ptr: Vec<usize>,
    pub col_idx: Vec<u32>,
    pub values: Vec<f32>,
    pub nrows: usize,
    pub ncols: usize,
}

impl SparseMatrix {
    /// y = A @ x (matrix × vector). y must be pre-zeroed, len = nrows. x len = ncols.
    pub fn mul_vec(&self, x: &[f32], y: &mut [f32]) {
        debug_assert_eq!(x.len(), self.ncols);
        debug_assert_eq!(y.len(), self.nrows);
        for (row, y_val) in y.iter_mut().enumerate().take(self.nrows) {
            let start = self.row_ptr[row];
            let end = self.row_ptr[row + 1];
            let mut sum = 0.0f32;
            for idx in start..end {
                sum += self.values[idx] * x[self.col_idx[idx] as usize];
            }
            *y_val = sum;
        }
    }

    /// y = A^T @ x (transpose × vector). y must be pre-zeroed, len = ncols. x len = nrows.
    pub fn mul_vec_transpose(&self, x: &[f32], y: &mut [f32]) {
        debug_assert_eq!(x.len(), self.nrows);
        debug_assert_eq!(y.len(), self.ncols);
        for (row, &x_row) in x.iter().enumerate().take(self.nrows) {
            let start = self.row_ptr[row];
            let end = self.row_ptr[row + 1];
            if x_row == 0.0 {
                continue;
            }
            for idx in start..end {
                y[self.col_idx[idx] as usize] += self.values[idx] * x_row;
            }
        }
    }
}

/// Build a term×document TF-IDF sparse matrix from tokenized documents.
///
/// Returns (matrix, vocabulary, idf_vector).
pub fn build_term_doc_matrix(
    docs: &[(PathBuf, String)],
) -> (SparseMatrix, Vocabulary, Vec<f32>) {
    let num_docs = docs.len();
    let mut vocab = Vocabulary::new();

    // Pass 1: tokenize all docs, count df(t) and per-doc tf
    // doc_tokens[doc_idx] = Vec<(term_id, tf)>
    let mut doc_tokens: Vec<Vec<(u32, u32)>> = Vec::with_capacity(num_docs);
    let mut df: HashMap<u32, u32> = HashMap::new();

    for (_path, text) in docs {
        let tokens = tokenize(text);
        let mut tf: HashMap<u32, u32> = HashMap::new();
        for tok in &tokens {
            let id = vocab.get_or_insert(tok);
            *tf.entry(id).or_insert(0) += 1;
        }
        // Record document frequency
        for &term_id in tf.keys() {
            *df.entry(term_id).or_insert(0) += 1;
        }
        let mut tf_vec: Vec<(u32, u32)> = tf.into_iter().collect();
        tf_vec.sort_by_key(|&(id, _)| id);
        doc_tokens.push(tf_vec);
    }

    let vocab_size = vocab.len();
    let n = num_docs as f32;

    // Compute IDF: log(N / df(t)) — smooth with +1 to avoid log(0) for safety
    let mut idf = vec![0.0f32; vocab_size];
    for (&term_id, &doc_freq) in &df {
        idf[term_id as usize] = (n / doc_freq as f32).ln();
    }

    // Pass 2: build CSR matrix (rows = terms, cols = docs)
    // We need to transpose from (doc, term) → (term, doc)
    // First collect all (term_id, doc_id, tfidf_value) triples
    let mut entries: Vec<(u32, u32, f32)> = Vec::new();
    for (doc_id, tf_vec) in doc_tokens.iter().enumerate() {
        for &(term_id, tf) in tf_vec {
            let tfidf = tf as f32 * idf[term_id as usize];
            if tfidf > 0.0 {
                entries.push((term_id, doc_id as u32, tfidf));
            }
        }
    }

    // Sort by (term_id, doc_id) for CSR row order
    entries.sort_by_key(|&(t, d, _)| (t, d));

    // Build CSR
    let mut row_ptr = vec![0usize; vocab_size + 1];
    let mut col_idx = Vec::with_capacity(entries.len());
    let mut values = Vec::with_capacity(entries.len());

    for &(term_id, doc_id, val) in &entries {
        row_ptr[term_id as usize + 1] += 1;
        col_idx.push(doc_id);
        values.push(val);
    }

    // Cumulative sum for row_ptr
    for i in 1..=vocab_size {
        row_ptr[i] += row_ptr[i - 1];
    }

    let matrix = SparseMatrix {
        row_ptr,
        col_idx,
        values,
        nrows: vocab_size,
        ncols: num_docs,
    };

    (matrix, vocab, idf)
}

// ---------------------------------------------------------------------------
// Power Iteration + Truncated SVD
// ---------------------------------------------------------------------------

/// Result of truncated SVD decomposition.
pub struct LsaComponents {
    /// vocab_size × k (word vectors, row-major).
    pub u: Vec<f32>,
    /// k singular values.
    pub singular_values: Vec<f32>,
    /// num_docs × k (document vectors, row-major).
    pub v: Vec<f32>,
    pub k: usize,
    pub vocab_size: usize,
    pub num_docs: usize,
}

/// Compute one singular triplet (u, sigma, v) of matrix A,
/// with deflation against previously extracted components.
///
/// `prev_u` and `prev_v` are columns of previously extracted U and V.
/// `prev_sigma` are their singular values.
fn power_iteration(
    a: &SparseMatrix,
    prev_u: &[f32],   // prev_k * nrows, row-major (each row is a u_i)
    prev_v: &[f32],   // prev_k * ncols, row-major (each row is a v_i)
    prev_sigma: &[f32],
    prev_k: usize,
    max_iter: usize,
    rng: &mut Xorshift128Plus,
) -> (Vec<f32>, f32, Vec<f32>) {
    let nrows = a.nrows;
    let ncols = a.ncols;

    // Random init v
    let mut v = vec![0.0f32; ncols];
    for x in v.iter_mut() {
        *x = rng.next_gaussian();
    }
    // Normalize v
    let norm = simd::norm_sq(&v).sqrt().max(1e-10);
    for x in v.iter_mut() {
        *x /= norm;
    }

    let mut u = vec![0.0f32; nrows];
    let mut sigma = 0.0f32;
    let mut tmp_u = vec![0.0f32; nrows];
    let mut tmp_v = vec![0.0f32; ncols];

    for _ in 0..max_iter {
        // u = A @ v
        for x in tmp_u.iter_mut() {
            *x = 0.0;
        }
        a.mul_vec(&v, &mut tmp_u);

        // Deflation: u -= sum_i sigma_i * (prev_v_i . v) * prev_u_i
        for (i, &sigma_i) in prev_sigma.iter().enumerate().take(prev_k) {
            let vi_start = i * ncols;
            let vi = &prev_v[vi_start..vi_start + ncols];
            let dot = simd::dot_product(vi, &v);
            let coeff = sigma_i * dot;

            let ui_start = i * nrows;
            let ui = &prev_u[ui_start..ui_start + nrows];
            // tmp_u -= coeff * ui
            for (t, &u_val) in tmp_u.iter_mut().zip(ui.iter()) {
                *t -= coeff * u_val;
            }
        }

        // sigma = ||tmp_u||
        sigma = simd::norm_sq(&tmp_u).sqrt();
        if sigma < 1e-10 {
            // Degenerate — zero singular value
            return (vec![0.0; nrows], 0.0, vec![0.0; ncols]);
        }

        // u = tmp_u / sigma
        for (u_val, &t) in u.iter_mut().zip(tmp_u.iter()) {
            *u_val = t / sigma;
        }

        // v = A^T @ u
        for x in tmp_v.iter_mut() {
            *x = 0.0;
        }
        a.mul_vec_transpose(&u, &mut tmp_v);

        // Deflation: tmp_v -= sum_i sigma_i * (prev_u_i . u) * prev_v_i
        for (i, &sigma_i) in prev_sigma.iter().enumerate().take(prev_k) {
            let ui_start = i * nrows;
            let ui = &prev_u[ui_start..ui_start + nrows];
            let dot = simd::dot_product(ui, &u);
            let coeff = sigma_i * dot;

            let vi_start = i * ncols;
            let vi = &prev_v[vi_start..vi_start + ncols];
            for (t, &v_val) in tmp_v.iter_mut().zip(vi.iter()) {
                *t -= coeff * v_val;
            }
        }

        // Normalize v
        let norm_v = simd::norm_sq(&tmp_v).sqrt().max(1e-10);
        for (v_val, &t) in v.iter_mut().zip(tmp_v.iter()) {
            *v_val = t / norm_v;
        }
    }

    (u, sigma, v)
}

/// Truncated SVD via repeated power iteration with deflation.
pub fn truncated_svd(
    a: &SparseMatrix,
    k: usize,
    max_iter: usize,
) -> LsaComponents {
    let nrows = a.nrows;
    let ncols = a.ncols;

    // Clamp k to min dimension
    let k = k.min(nrows).min(ncols);

    let mut all_u = Vec::with_capacity(k * nrows);
    let mut all_sigma = Vec::with_capacity(k);
    let mut all_v = Vec::with_capacity(k * ncols);

    let mut rng = Xorshift128Plus::new(SEED);

    for i in 0..k {
        let (u_i, sigma_i, v_i) = power_iteration(
            a,
            &all_u,
            &all_v,
            &all_sigma,
            i,
            max_iter,
            &mut rng,
        );

        // If we hit a zero singular value, remaining components are zero too
        if sigma_i < 1e-10 {
            // Pad remaining with zeros
            let remaining = k - i;
            all_u.extend(std::iter::repeat_n(0.0f32, remaining * nrows));
            all_sigma.extend(std::iter::repeat_n(0.0f32, remaining));
            all_v.extend(std::iter::repeat_n(0.0f32, remaining * ncols));
            break;
        }

        all_u.extend_from_slice(&u_i);
        all_sigma.push(sigma_i);
        all_v.extend_from_slice(&v_i);
    }

    LsaComponents {
        u: all_u,
        singular_values: all_sigma,
        v: all_v,
        k,
        vocab_size: nrows,
        num_docs: ncols,
    }
}

// ---------------------------------------------------------------------------
// LsaScorer — public API
// ---------------------------------------------------------------------------

/// Cosine similarity (public, for use by engine.rs per-line scoring).
pub fn cosine_similarity_pub(a: &[f32], b: &[f32]) -> f32 {
    cosine_similarity(a, b)
}

/// Cosine similarity using SIMD.
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

/// LSA-based semantic scorer.
///
/// Builds a latent semantic space from the corpus's own term-document
/// co-occurrence patterns, then scores queries against documents.
pub struct LsaScorer {
    components: LsaComponents,
    vocab: Vocabulary,
    idf: Vec<f32>,
    doc_vectors: Vec<Vec<f32>>,
    doc_paths: Vec<PathBuf>,
}

impl LsaScorer {
    /// Build an LSA index from a set of documents.
    pub fn build(docs: &[(PathBuf, String)], k: usize) -> Self {
        let actual_k = k.min(docs.len());

        if docs.is_empty() || actual_k == 0 {
            return Self {
                components: LsaComponents {
                    u: Vec::new(),
                    singular_values: Vec::new(),
                    v: Vec::new(),
                    k: 0,
                    vocab_size: 0,
                    num_docs: 0,
                },
                vocab: Vocabulary::new(),
                idf: Vec::new(),
                doc_vectors: Vec::new(),
                doc_paths: Vec::new(),
            };
        }

        let (matrix, vocab, idf) = build_term_doc_matrix(docs);
        let components = truncated_svd(&matrix, actual_k, DEFAULT_MAX_ITER);

        // Pre-compute document vectors from V matrix
        // V is stored as k rows of ncols each (row-major).
        // doc_vector[d][j] = V[j * ncols + d] → the j-th component of doc d
        let num_docs = docs.len();
        let k_actual = components.k;
        let mut doc_vectors = Vec::with_capacity(num_docs);

        for d in 0..num_docs {
            let mut vec_d = vec![0.0f32; k_actual];
            for (j, val) in vec_d.iter_mut().enumerate().take(k_actual) {
                *val = components.v[j * num_docs + d];
            }
            doc_vectors.push(vec_d);
        }

        let doc_paths: Vec<PathBuf> = docs.iter().map(|(p, _)| p.clone()).collect();

        Self {
            components,
            vocab,
            idf,
            doc_vectors,
            doc_paths,
        }
    }

    /// Project a query string into LSA space.
    ///
    /// Formula: q_lsa = Σ^{-1} · U^T · q_tfidf
    pub fn project_query(&self, query: &str) -> Vec<f32> {
        let k = self.components.k;
        if k == 0 {
            return Vec::new();
        }

        let vocab_size = self.components.vocab_size;

        // Build TF-IDF query vector (sparse → dense)
        let tokens = tokenize(query);
        let mut tf: HashMap<u32, u32> = HashMap::new();
        for tok in &tokens {
            if let Some(id) = self.vocab.get(tok) {
                *tf.entry(id).or_insert(0) += 1;
            }
        }

        let mut q_tfidf = vec![0.0f32; vocab_size];
        for (&term_id, &count) in &tf {
            q_tfidf[term_id as usize] = count as f32 * self.idf[term_id as usize];
        }

        // q_lsa[j] = (1/sigma_j) * sum_i(U[i,j] * q_tfidf[i])
        // U is stored as k rows of vocab_size each. U[j] starts at j * vocab_size.
        let mut q_lsa = vec![0.0f32; k];
        for (j, q_val) in q_lsa.iter_mut().enumerate().take(k) {
            let sigma = self.components.singular_values[j];
            if sigma < 1e-10 {
                continue;
            }
            let u_j = &self.components.u[j * vocab_size..(j + 1) * vocab_size];
            let dot = simd::dot_product(u_j, &q_tfidf);
            *q_val = dot / sigma;
        }

        q_lsa
    }

    /// Rank all documents by cosine similarity to a query vector.
    /// Returns (score, path_index) sorted descending.
    pub fn rank_documents(&self, query_vec: &[f32]) -> Vec<(f32, usize)> {
        let mut scores: Vec<(f32, usize)> = self
            .doc_vectors
            .iter()
            .enumerate()
            .map(|(idx, doc_vec)| (cosine_similarity(query_vec, doc_vec), idx))
            .collect();
        scores.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scores
    }

    /// Get the document path for a given index.
    pub fn doc_path(&self, idx: usize) -> &PathBuf {
        &self.doc_paths[idx]
    }

    /// Number of documents in the index.
    pub fn num_docs(&self) -> usize {
        self.doc_paths.len()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenize_basic() {
        let tokens = tokenize("Hello, world! This is a test.");
        assert_eq!(tokens, vec!["hello", "world", "this", "is", "a", "test"]);
    }

    #[test]
    fn test_tokenize_camel_case() {
        let tokens = tokenize("getUserName");
        assert_eq!(tokens, vec!["get", "user", "name"]);
    }

    #[test]
    fn test_tokenize_snake_case() {
        let tokens = tokenize("get_user_name");
        assert_eq!(tokens, vec!["get", "user", "name"]);
    }

    #[test]
    fn test_tokenize_mixed() {
        let tokens = tokenize("parseHTMLDocument");
        // "parse" + "HTML" + "Document" → ["parse", "html", "document"]
        assert_eq!(tokens, vec!["parse", "html", "document"]);
    }

    #[test]
    fn test_sparse_matrix_mul_vec() {
        // Simple 2×3 matrix:
        // [[1, 0, 2],
        //  [0, 3, 0]]
        let matrix = SparseMatrix {
            row_ptr: vec![0, 2, 3],
            col_idx: vec![0, 2, 1],
            values: vec![1.0, 2.0, 3.0],
            nrows: 2,
            ncols: 3,
        };

        let x = vec![1.0, 2.0, 3.0];
        let mut y = vec![0.0; 2];
        matrix.mul_vec(&x, &mut y);
        assert!((y[0] - 7.0).abs() < 1e-6); // 1*1 + 2*3 = 7
        assert!((y[1] - 6.0).abs() < 1e-6); // 3*2 = 6
    }

    #[test]
    fn test_sparse_matrix_mul_vec_transpose() {
        // Same 2×3 matrix, transpose multiply
        let matrix = SparseMatrix {
            row_ptr: vec![0, 2, 3],
            col_idx: vec![0, 2, 1],
            values: vec![1.0, 2.0, 3.0],
            nrows: 2,
            ncols: 3,
        };

        let x = vec![1.0, 2.0]; // nrows
        let mut y = vec![0.0; 3]; // ncols
        matrix.mul_vec_transpose(&x, &mut y);
        assert!((y[0] - 1.0).abs() < 1e-6); // 1*1
        assert!((y[1] - 6.0).abs() < 1e-6); // 3*2
        assert!((y[2] - 2.0).abs() < 1e-6); // 2*1
    }

    #[test]
    fn test_tfidf_weighting() {
        let docs = vec![
            (PathBuf::from("a.rs"), "the the the common common rare".to_string()),
            (PathBuf::from("b.rs"), "the the the common common common".to_string()),
            (PathBuf::from("c.rs"), "the the the common common common".to_string()),
        ];

        let (matrix, vocab, idf) = build_term_doc_matrix(&docs);

        // "the" appears in all 3 docs → IDF = ln(3/3) = 0
        let the_id = vocab.get("the").unwrap();
        assert!((idf[the_id as usize]).abs() < 1e-6, "IDF of 'the' should be ~0");

        // "rare" appears in 1 doc → IDF = ln(3/1) = ln(3) ≈ 1.099
        let rare_id = vocab.get("rare").unwrap();
        assert!(
            (idf[rare_id as usize] - 3.0f32.ln()).abs() < 1e-4,
            "IDF of 'rare' should be ln(3)"
        );

        assert_eq!(matrix.nrows, vocab.len());
        assert_eq!(matrix.ncols, 3);
    }

    #[test]
    fn test_power_iteration_dominant_singular_value() {
        // Simple rank-1 matrix: [[3, 0], [0, 0], [4, 0]]
        // Singular value should be 5 (sqrt(9+16))
        let matrix = SparseMatrix {
            row_ptr: vec![0, 1, 1, 2],
            col_idx: vec![0, 0],
            values: vec![3.0, 4.0],
            nrows: 3,
            ncols: 2,
        };

        let mut rng = Xorshift128Plus::new(SEED);
        let (u, sigma, v) = power_iteration(
            &matrix, &[], &[], &[], 0, 50, &mut rng,
        );

        assert!(
            (sigma - 5.0).abs() < 0.01,
            "Dominant singular value should be 5, got {}",
            sigma
        );

        // u should be unit vector close to [3/5, 0, 4/5]
        let u_norm = simd::norm_sq(&u).sqrt();
        assert!((u_norm - 1.0).abs() < 1e-4, "u should be unit vector");

        // v should be unit vector close to [1, 0]
        let v_norm = simd::norm_sq(&v).sqrt();
        assert!((v_norm - 1.0).abs() < 1e-4, "v should be unit vector");
    }

    #[test]
    fn test_truncated_svd_reconstruction() {
        // Create a small matrix and verify low-rank approximation
        let docs = vec![
            (PathBuf::from("a.rs"), "login auth user password".to_string()),
            (PathBuf::from("b.rs"), "login auth session token".to_string()),
            (PathBuf::from("c.rs"), "database query insert table".to_string()),
            (PathBuf::from("d.rs"), "database schema migrate index".to_string()),
        ];

        let (matrix, _vocab, _idf) = build_term_doc_matrix(&docs);
        let components = truncated_svd(&matrix, 2, 20);

        // Reconstruct: A_approx = U @ diag(S) @ V^T
        // Check that reconstruction error is bounded
        let k = components.k;
        let nrows = matrix.nrows;
        let ncols = matrix.ncols;

        // Compute ||A - USV^T||_F
        let mut total_err = 0.0f64;
        for row in 0..nrows {
            let start = matrix.row_ptr[row];
            let end = matrix.row_ptr[row + 1];
            // Build dense row of A
            let mut a_row = vec![0.0f32; ncols];
            for idx in start..end {
                a_row[matrix.col_idx[idx] as usize] = matrix.values[idx];
            }
            // Compute reconstructed row
            for col in 0..ncols {
                let mut approx = 0.0f32;
                for j in 0..k {
                    let u_ij = components.u[j * nrows + row];
                    let v_jc = components.v[j * ncols + col];
                    approx += components.singular_values[j] * u_ij * v_jc;
                }
                let diff = a_row[col] - approx;
                total_err += (diff * diff) as f64;
            }
        }

        let frob_err = total_err.sqrt();
        eprintln!("SVD reconstruction Frobenius error: {:.4}", frob_err);

        // With k=2 on a small 4-doc matrix, error should be reasonable
        // (not zero since we only keep 2 components, but much less than ||A||_F)
        assert!(frob_err < 10.0, "Reconstruction error too large: {}", frob_err);
    }

    #[test]
    fn test_lsa_cooccurrence_similarity() {
        // "login" and "auth" co-occur in same files → should be similar
        let docs = vec![
            (PathBuf::from("a.rs"), "login auth user credentials verify".to_string()),
            (PathBuf::from("b.rs"), "login auth session token validate".to_string()),
            (PathBuf::from("c.rs"), "login auth password hash bcrypt".to_string()),
            (PathBuf::from("d.rs"), "database query insert select update".to_string()),
            (PathBuf::from("e.rs"), "database schema migrate column index".to_string()),
            (PathBuf::from("f.rs"), "database pool connection transaction".to_string()),
        ];

        let scorer = LsaScorer::build(&docs, 4);

        let login_vec = scorer.project_query("login");
        let auth_vec = scorer.project_query("auth");
        let db_vec = scorer.project_query("database");

        let login_auth_sim = cosine_similarity(&login_vec, &auth_vec);
        let login_db_sim = cosine_similarity(&login_vec, &db_vec);

        eprintln!("login-auth similarity: {:.4}", login_auth_sim);
        eprintln!("login-database similarity: {:.4}", login_db_sim);

        assert!(
            login_auth_sim > login_db_sim,
            "login-auth ({:.4}) should be more similar than login-db ({:.4})",
            login_auth_sim, login_db_sim
        );
    }

    #[test]
    fn test_query_foldin() {
        let docs = vec![
            (PathBuf::from("auth.rs"), "login auth user password session token verify".to_string()),
            (PathBuf::from("db.rs"), "database query select insert table row column".to_string()),
            (PathBuf::from("net.rs"), "http server socket bind listen accept request".to_string()),
        ];

        let scorer = LsaScorer::build(&docs, 3);
        let q = scorer.project_query("authenticate user login");
        let rankings = scorer.rank_documents(&q);

        // auth.rs should rank first
        let top_path = scorer.doc_path(rankings[0].1);
        assert!(
            top_path.to_string_lossy().contains("auth"),
            "auth.rs should rank first, got: {}",
            top_path.display()
        );
    }

    #[test]
    fn test_deterministic() {
        let docs = vec![
            (PathBuf::from("a.rs"), "login auth user password".to_string()),
            (PathBuf::from("b.rs"), "database query table".to_string()),
        ];

        let scorer1 = LsaScorer::build(&docs, 2);
        let scorer2 = LsaScorer::build(&docs, 2);

        let q1 = scorer1.project_query("login");
        let q2 = scorer2.project_query("login");

        assert_eq!(q1.len(), q2.len());
        for (a, b) in q1.iter().zip(q2.iter()) {
            assert!(
                (a - b).abs() < 1e-5,
                "LSA should be deterministic: {} vs {}",
                a, b
            );
        }

        let r1 = scorer1.rank_documents(&q1);
        let r2 = scorer2.rank_documents(&q2);
        assert_eq!(r1.len(), r2.len());
        for ((s1, i1), (s2, i2)) in r1.iter().zip(r2.iter()) {
            assert_eq!(i1, i2);
            assert!((s1 - s2).abs() < 1e-6);
        }
    }
}
