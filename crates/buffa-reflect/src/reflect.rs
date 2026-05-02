//! [`ReflectMessage`] — the runtime hand-off from a generated buffa message
//! type to its descriptor.

use crate::message::MessageDescriptor;

/// Implemented by every type that has a descriptor in some
/// [`crate::DescriptorPool`] — generated typed messages via
/// `#[derive(ReflectMessage)]` and runtime-typed
/// [`crate::DynamicMessage`].
///
/// Note: this trait deliberately does **not** require
/// [`buffa::Message`] as a super-trait so that
/// [`crate::DynamicMessage`] (which has no static
/// `DefaultInstance`) can implement it. Where wire-encoding via
/// `Self::encode_to_vec` is needed, individual methods name
/// `Self: ::buffa::Message` as a where-clause.
pub trait ReflectMessage {
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
        Self: ::buffa::Message + Sized,
    {
        let descriptor = self.descriptor();
        let bytes = ::buffa::Message::encode_to_vec(self);
        crate::DynamicMessage::decode(descriptor, bytes.as_slice())
            .expect("self-encoded message must decode against its own descriptor")
    }
}

/// Reflection over a buffa view type. Implemented by every generated
/// `*View<'a>` so the zero-copy decode path can introspect the
/// message without allocating an owned form.
///
/// `descriptor()` returns the same [`MessageDescriptor`] the owned
/// [`ReflectMessage`] returns for the same proto.
pub trait ReflectMessageView<'a>: ::buffa::view::MessageView<'a> {
    /// Resolve the [`MessageDescriptor`] for this view's proto type.
    fn descriptor(&self) -> MessageDescriptor;
}

#[cfg(feature = "dynamic")]
impl ReflectMessage for crate::DynamicMessage {
    fn descriptor(&self) -> MessageDescriptor {
        crate::DynamicMessage::descriptor(self)
    }

    // `transcode_to_dynamic` is intentionally not overridden here —
    // the trait's default impl carries `where Self: buffa::Message`,
    // which `DynamicMessage` cannot satisfy. Callers reach the
    // short-circuit via the inherent
    // [`crate::DynamicMessage::transcode_to_dynamic`], which has the
    // same name and resolves first under Rust's method-resolution rules.
}
