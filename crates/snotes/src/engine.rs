//! Adapter around rnote-engine. Every call into the engine goes through this
//! module, because the upstream API is not stable.

use rnote_engine::Engine;

/// Owns the rnote engine instance behind SnoteS' own interface.
pub struct InkEngine {
    inner: Engine,
}

impl InkEngine {
    pub fn new() -> Self {
        Self {
            inner: Engine::default(),
        }
    }

    /// Current document size in engine units, as (width, height).
    pub fn document_size(&self) -> (f64, f64) {
        (self.inner.document.width, self.inner.document.height)
    }
}

impl Default for InkEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_engine_has_a_document() {
        let (w, h) = InkEngine::new().document_size();
        assert!(w > 0.0 && h > 0.0);
    }
}
