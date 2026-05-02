//! Protobuf textproto encode / decode for [`crate::DynamicMessage`].
//!
//! Mirrors `protoc --decode` output and `protoc --encode` input.
//! API names match prost-reflect for cross-ecosystem familiarity.

mod format;
mod parse;

pub use crate::dynamic::text_format::{
    format::FormatOptions,
    parse::{ParseError, ParseErrorKind},
};
use crate::{dynamic::message::DynamicMessage, message::MessageDescriptor};

impl DynamicMessage {
    /// Render as canonical textproto using the default
    /// [`FormatOptions`].
    #[must_use]
    pub fn to_text_format(&self) -> String {
        self.to_text_format_with_options(&FormatOptions::default())
    }

    /// Render with explicit [`FormatOptions`].
    #[must_use]
    pub fn to_text_format_with_options(&self, options: &FormatOptions) -> String {
        let mut buf = String::new();
        format::format_message(self, options, &mut buf, 0);
        buf
    }

    /// Parse textproto into a `DynamicMessage` of the given descriptor.
    ///
    /// # Errors
    ///
    /// See [`ParseError`].
    pub fn parse_text_format(
        descriptor: MessageDescriptor,
        input: &str,
    ) -> Result<Self, ParseError> {
        parse::parse(descriptor, input)
    }
}
