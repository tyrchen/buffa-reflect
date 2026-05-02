//! [`DynamicMessage`] — a runtime-typed message backed by a
//! [`MessageDescriptor`].

use std::borrow::Cow;

use buffa::{
    DecodeError, EncodeError,
    bytes::{Buf, BufMut, Bytes, BytesMut},
};

use crate::{
    dynamic::{
        fields::{DynamicMessageFieldSet, ValueOrUnknown},
        message_codec, message_decode,
        unknown::UnknownFieldSet,
        value::{SetFieldError, Value},
    },
    field::FieldDescriptor,
    message::MessageDescriptor,
    pool::DescriptorPool,
};

/// A protobuf message whose schema is known at runtime via a
/// [`MessageDescriptor`].
///
/// `DynamicMessage` is the workhorse type that lets a single binary
/// transcode, inspect, or mutate any proto message in any
/// [`DescriptorPool`]. See the module-level docs for examples.
#[derive(Clone, Debug)]
pub struct DynamicMessage {
    pub(crate) desc: MessageDescriptor,
    pub(crate) fields: DynamicMessageFieldSet,
}

impl PartialEq for DynamicMessage {
    fn eq(&self, other: &Self) -> bool {
        self.desc == other.desc && self.fields == other.fields
    }
}

impl DynamicMessage {
    /// Construct an empty message of the given descriptor.
    #[must_use]
    pub fn new(desc: MessageDescriptor) -> Self {
        Self {
            desc,
            fields: DynamicMessageFieldSet::default(),
        }
    }

    /// The owning descriptor.
    #[must_use]
    pub fn descriptor(&self) -> MessageDescriptor {
        self.desc.clone()
    }

    /// The pool the descriptor was built from. Cloning is `Arc`-cheap.
    #[must_use]
    pub fn parent_pool(&self) -> DescriptorPool {
        self.desc.pool.clone()
    }

    /// True iff no known or unknown field has been recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    // ── decode ──────────────────────────────────────────────────────────

    /// Decode `buf` into a fresh [`DynamicMessage`] of the given
    /// descriptor.
    ///
    /// # Errors
    /// See [`DecodeError`].
    pub fn decode<B: Buf>(desc: MessageDescriptor, mut buf: B) -> Result<Self, DecodeError> {
        let mut msg = Self::new(desc);
        message_decode::merge(&mut msg, &mut buf, buffa::RECURSION_LIMIT)?;
        Ok(msg)
    }

    /// Decode with a custom [`buffa::DecodeOptions`].
    ///
    /// # Errors
    /// See [`DecodeError`].
    pub fn decode_with_options<B: Buf>(
        desc: MessageDescriptor,
        mut buf: B,
        opts: buffa::DecodeOptions,
    ) -> Result<Self, DecodeError> {
        if buf.remaining() > opts.max_message_size() {
            return Err(DecodeError::MessageTooLarge);
        }
        let mut msg = Self::new(desc);
        message_decode::merge(&mut msg, &mut buf, opts.recursion_limit())?;
        Ok(msg)
    }

    /// Merge wire bytes into `self`, accumulating fields atop the
    /// existing ones (matching `Message::merge` semantics).
    ///
    /// # Errors
    /// See [`DecodeError`].
    pub fn merge<B: Buf>(&mut self, mut buf: B) -> Result<(), DecodeError> {
        message_decode::merge(self, &mut buf, buffa::RECURSION_LIMIT)
    }

    // ── encode ──────────────────────────────────────────────────────────

    /// Encoded byte length, including unknown fields.
    #[must_use]
    pub fn encoded_len(&self) -> usize {
        message_codec::encoded_len(self)
    }

    /// Encode to `buf`. Iterates fields in number order; unknown fields
    /// re-emit at their original positions.
    ///
    /// # Errors
    /// Bubbles up encoder failures. With well-formed in-memory state
    /// the only realistic source of failure is `BufMut::remaining_mut`
    /// running short.
    pub fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), EncodeError> {
        message_codec::encode(self, buf)
    }

    /// Encode to a fresh `Vec<u8>`.
    #[must_use]
    pub fn encode_to_vec(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(self.encoded_len());
        // BufMut for Vec never errors as long as the heap is OK.
        let _ = self.encode(&mut buf);
        buf
    }

    /// Encode to a fresh [`Bytes`].
    #[must_use]
    pub fn encode_to_bytes(&self) -> Bytes {
        let mut buf = BytesMut::with_capacity(self.encoded_len());
        let _ = self.encode(&mut buf);
        buf.freeze()
    }

    // ── transcode ───────────────────────────────────────────────────────

    /// Merge a typed `T`'s wire bytes into `self`.
    ///
    /// # Errors
    /// See [`DecodeError`]. Errors imply `T`'s schema and `self.descriptor()`
    /// are incompatible.
    pub fn transcode_from<T: buffa::Message>(&mut self, value: &T) -> Result<(), DecodeError> {
        self.merge(value.encode_to_vec().as_slice())
    }

    /// Decode `self` as the typed `T`.
    ///
    /// # Errors
    /// See [`DecodeError`]. Errors imply `T`'s schema and the dynamic
    /// message's descriptor are incompatible.
    pub fn transcode_to<T: buffa::Message + Default>(&self) -> Result<T, DecodeError> {
        T::decode_from_slice(self.encode_to_vec().as_slice())
    }

    /// Specialisation of `transcode_to_dynamic` — returns `self.clone()`
    /// without the wire round-trip a typed `ReflectMessage` would pay.
    #[must_use]
    pub fn transcode_to_dynamic(&self) -> Self {
        self.clone()
    }

    // ── inspection / iteration ──────────────────────────────────────────

    /// Iterate every populated known field in field-number order.
    pub fn fields(&self) -> impl Iterator<Item = (FieldDescriptor, &Value)> + '_ {
        let desc = self.desc.clone();
        self.fields
            .iter_known()
            .filter_map(move |(n, v)| desc.get_field_by_number(n).map(|fd| (fd, v)))
    }

    /// Iterate fields with explicit knobs over default-omission and
    /// declaration vs number order. See the design doc §6 for the
    /// rationale.
    pub fn iter_with_options<'a>(
        &'a self,
        include_default: bool,
        index_order: bool,
    ) -> Box<dyn Iterator<Item = (FieldDescriptor, Cow<'a, Value>)> + 'a> {
        if index_order {
            Box::new(self.iter_index_order(include_default))
        } else {
            Box::new(self.iter_number_order(include_default))
        }
    }

    fn iter_number_order<'a>(
        &'a self,
        include_default: bool,
    ) -> impl Iterator<Item = (FieldDescriptor, Cow<'a, Value>)> + 'a {
        let descriptors = collect_descriptors_in_number_order(&self.desc, include_default);
        descriptors
            .into_iter()
            .filter_map(move |fd| match self.fields.get_value(fd.number()) {
                Some(v) => Some((fd, Cow::Borrowed(v))),
                None if include_default => {
                    let v = Value::default_value_for_field(&fd);
                    Some((fd, Cow::Owned(v)))
                }
                None => None,
            })
    }

    fn iter_index_order<'a>(
        &'a self,
        include_default: bool,
    ) -> impl Iterator<Item = (FieldDescriptor, Cow<'a, Value>)> + 'a {
        self.desc
            .fields()
            .filter_map(move |fd| match self.fields.get_value(fd.number()) {
                Some(v) => Some((fd, Cow::Borrowed(v))),
                None if include_default => {
                    let v = Value::default_value_for_field(&fd);
                    Some((fd, Cow::Owned(v)))
                }
                None => None,
            })
    }

    /// True iff a known value exists at `field`'s number, or for
    /// non-presence-tracking fields, the value is non-default.
    #[must_use]
    pub fn has_field(&self, field: &FieldDescriptor) -> bool {
        match self.fields.get_value(field.number()) {
            Some(v) => field.supports_presence() || !v.is_default(&field.kind()),
            None => false,
        }
    }

    /// Read `field` — borrows the populated value or synthesises a
    /// default.
    #[must_use]
    pub fn get_field(&self, field: &FieldDescriptor) -> Cow<'_, Value> {
        match self.fields.get_value(field.number()) {
            Some(v) => Cow::Borrowed(v),
            None => Cow::Owned(Value::default_value_for_field(field)),
        }
    }

    /// Mutable accessor — clears any active oneof sibling first, then
    /// inserts a default value if the slot was empty.
    pub fn get_field_mut(&mut self, field: &FieldDescriptor) -> &mut Value {
        if !self.fields.has_value(field.number()) {
            let default = Value::default_value_for_field(field);
            self.fields.set(field, default);
        } else if let Some(oneof) = field.containing_oneof() {
            let self_number = field.number();
            for sibling in oneof.fields() {
                if sibling.number() != self_number {
                    self.fields.fields.remove(&sibling.number());
                }
            }
        }
        self.fields
            .get_value_mut(field.number())
            .expect("just-inserted value")
    }

    /// `has_field` keyed by proto name.
    #[must_use]
    pub fn has_field_by_name(&self, name: &str) -> bool {
        match self.desc.get_field_by_name(name) {
            Some(fd) => self.has_field(&fd),
            None => false,
        }
    }

    /// `get_field` keyed by proto name.
    #[must_use]
    pub fn get_field_by_name(&self, name: &str) -> Option<Cow<'_, Value>> {
        self.desc
            .get_field_by_name(name)
            .map(|fd| self.get_field(&fd))
    }

    /// `get_field_mut` keyed by proto name.
    pub fn get_field_by_name_mut(&mut self, name: &str) -> Option<&mut Value> {
        let fd = self.desc.get_field_by_name(name)?;
        Some(self.get_field_mut(&fd))
    }

    /// `has_field` keyed by tag number.
    #[must_use]
    pub fn has_field_by_number(&self, number: u32) -> bool {
        match self.desc.get_field_by_number(number) {
            Some(fd) => self.has_field(&fd),
            None => false,
        }
    }

    /// `get_field` keyed by tag number.
    #[must_use]
    pub fn get_field_by_number(&self, number: u32) -> Option<Cow<'_, Value>> {
        self.desc
            .get_field_by_number(number)
            .map(|fd| self.get_field(&fd))
    }

    /// `get_field_mut` keyed by tag number.
    pub fn get_field_by_number_mut(&mut self, number: u32) -> Option<&mut Value> {
        let fd = self.desc.get_field_by_number(number)?;
        Some(self.get_field_mut(&fd))
    }

    // ── mutation: dual API ──────────────────────────────────────────────

    /// Set `field` to `value`. Validates with `debug_assert!` (zero
    /// cost in release builds); panics on type mismatch in debug. Use
    /// [`Self::try_set_field`] when the value's shape is data-driven.
    ///
    /// # Panics
    ///
    /// Panics in debug builds when `value` doesn't match `field`'s
    /// declared shape. In release builds the bad value is stored and
    /// will surface as an encode-time mismatch.
    pub fn set_field(&mut self, field: &FieldDescriptor, value: Value) {
        self.fields.set(field, value);
    }

    /// Validating equivalent of [`Self::set_field`]: returns
    /// [`SetFieldError::InvalidType`] when `value` doesn't match
    /// `field`.
    ///
    /// # Errors
    ///
    /// See [`SetFieldError`].
    pub fn try_set_field(
        &mut self,
        field: &FieldDescriptor,
        value: Value,
    ) -> Result<(), SetFieldError> {
        self.fields.try_set(field, value)
    }

    /// `set_field` keyed by tag number.
    ///
    /// # Panics
    ///
    /// Panics if `number` is unknown to the descriptor (in addition to
    /// the type-mismatch panic inherited from [`Self::set_field`]).
    pub fn set_field_by_number(&mut self, number: u32, value: Value) {
        let fd = self
            .desc
            .get_field_by_number(number)
            .expect("set_field_by_number: unknown field number");
        self.fields.set(&fd, value);
    }

    /// Validating `_by_number` form.
    ///
    /// # Errors
    ///
    /// Returns [`SetFieldError::NotFound`] when `number` is unknown.
    pub fn try_set_field_by_number(
        &mut self,
        number: u32,
        value: Value,
    ) -> Result<(), SetFieldError> {
        let fd = self
            .desc
            .get_field_by_number(number)
            .ok_or(SetFieldError::NotFound)?;
        self.fields.try_set(&fd, value)
    }

    /// `set_field` keyed by proto name.
    ///
    /// # Panics
    ///
    /// Panics if `name` is unknown to the descriptor (in addition to
    /// the type-mismatch panic inherited from [`Self::set_field`]).
    pub fn set_field_by_name(&mut self, name: &str, value: Value) {
        let fd = self
            .desc
            .get_field_by_name(name)
            .expect("set_field_by_name: unknown field name");
        self.fields.set(&fd, value);
    }

    /// Validating `_by_name` form.
    ///
    /// # Errors
    ///
    /// Returns [`SetFieldError::NotFound`] when `name` is unknown.
    pub fn try_set_field_by_name(&mut self, name: &str, value: Value) -> Result<(), SetFieldError> {
        let fd = self
            .desc
            .get_field_by_name(name)
            .ok_or(SetFieldError::NotFound)?;
        self.fields.try_set(&fd, value)
    }

    /// Clear `field` (and any sibling oneof member).
    pub fn clear_field(&mut self, field: &FieldDescriptor) {
        self.fields.clear(field);
    }

    /// Clear by name; no-op when the name is unknown.
    pub fn clear_field_by_name(&mut self, name: &str) {
        if let Some(fd) = self.desc.get_field_by_name(name) {
            self.fields.clear(&fd);
        }
    }

    /// Clear by number; no-op when the number is unknown.
    pub fn clear_field_by_number(&mut self, number: u32) {
        if let Some(fd) = self.desc.get_field_by_number(number) {
            self.fields.clear(&fd);
        }
    }

    /// Iterate every recorded unknown-field set in number order.
    pub fn unknown_fields(&self) -> impl Iterator<Item = (u32, &UnknownFieldSet)> + '_ {
        self.fields.iter_unknown()
    }

    /// Move every unknown-field set out of `self`. The returned vector
    /// is in number order.
    pub fn drain_unknown_fields(&mut self) -> impl Iterator<Item = (u32, UnknownFieldSet)> {
        self.fields.drain_unknown().into_iter()
    }

    pub(crate) fn fields_set_ref(&self) -> &DynamicMessageFieldSet {
        &self.fields
    }

    pub(crate) fn fields_set_mut(&mut self) -> &mut DynamicMessageFieldSet {
        &mut self.fields
    }

    /// Iterate over the raw entries in number order — used by the codec
    /// to interleave known + unknown encoding.
    pub(crate) fn iter_storage(&self) -> impl Iterator<Item = (u32, &ValueOrUnknown)> + '_ {
        self.fields.iter_in_order()
    }
}

fn collect_descriptors_in_number_order(
    desc: &MessageDescriptor,
    _include_default: bool,
) -> Vec<FieldDescriptor> {
    let mut fields: Vec<FieldDescriptor> = desc.fields().collect();
    fields.sort_by_key(|f| f.number());
    fields
}
