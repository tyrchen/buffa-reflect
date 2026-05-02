//! [`ReflectMessage`] — the runtime hand-off from a generated buffa message
//! type to its descriptor.

use crate::message::MessageDescriptor;

/// Implemented by every generated buffa message that has a descriptor in
/// some [`crate::DescriptorPool`].
///
/// Carries `descriptor()` plus, when the `dynamic` feature is on, a
/// default-impl `transcode_to_dynamic()` that wire-encodes `self` and
/// decodes the bytes against the descriptor.
pub trait ReflectMessage: ::buffa::Message {
    /// Resolve the [`MessageDescriptor`] for `Self`.
    ///
    /// The pool the descriptor lives in is set up by the
    /// `#[derive(ReflectMessage)]` macro, either:
    /// * the user-supplied `descriptor_pool` expression, or
    /// * a lazily-decoded pool keyed off the embedded `file_descriptor_set_bytes`.
    fn descriptor(&self) -> MessageDescriptor;

    /// Round-trip `self` through wire bytes into a [`crate::DynamicMessage`].
    ///
    /// The default implementation pays one wire round-trip; the impl
    /// for [`crate::DynamicMessage`] short-circuits to `self.clone()`.
    ///
    /// # Panics
    ///
    /// Panics if `self.encode_to_vec()` produces bytes that fail to
    /// decode against `self.descriptor()` — this would indicate a bug
    /// in the generated [`descriptor()`](Self::descriptor) wiring or in
    /// `self`'s `Message` impl.
    #[cfg(feature = "dynamic")]
    fn transcode_to_dynamic(&self) -> crate::DynamicMessage
    where
        Self: Sized,
    {
        let descriptor = self.descriptor();
        let bytes = ::buffa::Message::encode_to_vec(self);
        crate::DynamicMessage::decode(descriptor, bytes.as_slice())
            .expect("self-encoded message must decode against its own descriptor")
    }
}

// `DynamicMessage` is intentionally not a [`ReflectMessage`]: the
// supertrait bound (`buffa::Message`) requires a static
// `DefaultInstance` keyed by Rust type, which a runtime-typed
// `DynamicMessage` cannot satisfy without a fake descriptor.
//
// Generic code that needs uniform handling between typed and dynamic
// messages can call [`crate::DynamicMessage::descriptor`] and
// [`crate::DynamicMessage::transcode_to`] / `transcode_to_dynamic`
// directly — both are method-resolved with the same names a
// `ReflectMessage` impl would surface.
