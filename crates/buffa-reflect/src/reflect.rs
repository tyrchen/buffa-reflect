//! [`ReflectMessage`] — the runtime hand-off from a generated buffa message
//! type to its descriptor.

use crate::message::MessageDescriptor;

/// Implemented by every generated buffa message that has a descriptor in
/// some [`crate::DescriptorPool`].
///
/// In Phase 1 the trait carries exactly one method — `descriptor()` — which
/// returns a cheap-to-clone handle to the message's [`MessageDescriptor`].
/// Phase 2 will extend the trait with `transcode_to_dynamic` and friends.
pub trait ReflectMessage: ::buffa::Message {
    /// Resolve the [`MessageDescriptor`] for `Self`.
    ///
    /// The pool the descriptor lives in is set up by the
    /// `#[derive(ReflectMessage)]` macro, either:
    /// * the user-supplied `descriptor_pool` expression, or
    /// * a lazily-decoded pool keyed off the embedded
    ///   `file_descriptor_set_bytes`.
    fn descriptor(&self) -> MessageDescriptor;
}
