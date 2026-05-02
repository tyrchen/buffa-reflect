//! Unknown-field preservation for [`super::DynamicMessage`].
//!
//! Wraps `buffa::UnknownField` / `buffa::UnknownFields` so consumers of
//! `buffa-reflect` don't need to import the upstream types when they
//! only want round-trip fidelity.

use buffa::bytes::BufMut;
pub use buffa::unknown_fields::UnknownField;

/// All unknown wire-tags observed for a single field number.
///
/// Wraps [`buffa::UnknownFields`]; we re-expose the upstream type
/// behind a thin newtype so the storage layer's `ValueOrUnknown`
/// variant has a stable name even if upstream renames things.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UnknownFieldSet {
    inner: buffa::UnknownFields,
}

impl UnknownFieldSet {
    /// Construct an empty set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// True iff no unknown fields were recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Number of recorded entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Append one entry.
    pub fn push(&mut self, field: UnknownField) {
        self.inner.push(field);
    }

    /// Iterate over the recorded entries in insertion order.
    pub fn iter(&self) -> core::slice::Iter<'_, UnknownField> {
        self.inner.iter()
    }

    /// Encoded byte length of every entry tagged at `_number`.
    ///
    /// (The `_number` argument is unused — entries already carry their
    /// own number on `UnknownField::number`. Kept for symmetry with
    /// `encode`.)
    #[must_use]
    pub fn encoded_len(&self, _number: u32) -> usize {
        self.inner.encoded_len()
    }

    /// Re-emit every entry to `buf` at its original position.
    pub fn encode(&self, _number: u32, buf: &mut impl BufMut) {
        self.inner.write_to(buf);
    }
}

impl<'a> IntoIterator for &'a UnknownFieldSet {
    type Item = &'a UnknownField;
    type IntoIter = core::slice::Iter<'a, UnknownField>;

    fn into_iter(self) -> Self::IntoIter {
        self.inner.iter()
    }
}
