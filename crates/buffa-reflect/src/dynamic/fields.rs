//! Internal storage for [`super::DynamicMessage`].
//!
//! BTreeMap keyed by field number (matches prost-reflect's choice — a
//! single iteration order over interleaved known and unknown fields,
//! sparse-friendly, and migration-friendly for consumers moving from
//! prost-reflect).

use std::{collections::BTreeMap, fmt};

use crate::{
    dynamic::{
        unknown::UnknownFieldSet,
        value::{SetFieldError, Value},
    },
    field::{FieldDescriptor, Kind},
    oneof::OneofDescriptor,
};

/// One entry in the field-set storage.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ValueOrUnknown {
    /// A known field with a typed value.
    Value(Value),
    /// Unknown wire tags observed at this field number.
    Unknown(UnknownFieldSet),
    /// Drained-out sentinel. Keeps the BTreeMap entry in place so
    /// drain iterators don't have to mutate the map mid-iteration.
    Taken,
}

/// Storage layer for a [`super::DynamicMessage`].
#[derive(Clone, Default, Debug, PartialEq)]
pub(crate) struct DynamicMessageFieldSet {
    pub(crate) fields: BTreeMap<u32, ValueOrUnknown>,
}

impl DynamicMessageFieldSet {
    /// Look up by field number; returns `None` for `Taken` / `Unknown` slots.
    pub(crate) fn get_value(&self, number: u32) -> Option<&Value> {
        match self.fields.get(&number) {
            Some(ValueOrUnknown::Value(v)) => Some(v),
            _ => None,
        }
    }

    pub(crate) fn get_value_mut(&mut self, number: u32) -> Option<&mut Value> {
        match self.fields.get_mut(&number) {
            Some(ValueOrUnknown::Value(v)) => Some(v),
            _ => None,
        }
    }

    /// Iterate every populated known field in field-number order.
    pub(crate) fn iter_known(&self) -> impl Iterator<Item = (u32, &Value)> + '_ {
        self.fields.iter().filter_map(|(n, v)| match v {
            ValueOrUnknown::Value(val) => Some((*n, val)),
            _ => None,
        })
    }

    /// Iterate every recorded unknown set in field-number order.
    pub(crate) fn iter_unknown(&self) -> impl Iterator<Item = (u32, &UnknownFieldSet)> + '_ {
        self.fields.iter().filter_map(|(n, v)| match v {
            ValueOrUnknown::Unknown(set) => Some((*n, set)),
            _ => None,
        })
    }

    /// `(number, ValueOrUnknown)` in number order — used for canonical
    /// encode (interleaves known + unknown by number).
    pub(crate) fn iter_in_order(&self) -> impl Iterator<Item = (u32, &ValueOrUnknown)> + '_ {
        self.fields.iter().map(|(n, v)| (*n, v))
    }

    /// True iff `number` is mapped to a populated `Value` slot.
    pub(crate) fn has_value(&self, number: u32) -> bool {
        matches!(self.fields.get(&number), Some(ValueOrUnknown::Value(_)))
    }

    /// Set a known value at `field`. Caller must have validated the
    /// shape via [`Value::is_valid_for_field`] (`set_field` does this
    /// behind a `debug_assert!`; `try_set_field` does it eagerly and
    /// surfaces the error).
    pub(crate) fn set(&mut self, field: &FieldDescriptor, value: Value) {
        debug_assert!(
            value.is_valid_for_field(field),
            "DynamicMessageFieldSet::set: value {value:?} not valid for field {}",
            field.full_name(),
        );
        self.clear_oneof_siblings(field);
        self.fields
            .insert(field.number(), ValueOrUnknown::Value(value));
    }

    /// Validating equivalent of [`Self::set`].
    pub(crate) fn try_set(
        &mut self,
        field: &FieldDescriptor,
        value: Value,
    ) -> Result<(), SetFieldError> {
        if !value.is_valid_for_field(field) {
            return Err(SetFieldError::InvalidType {
                field: field.clone(),
                value: Box::new(value),
            });
        }
        self.clear_oneof_siblings(field);
        self.fields
            .insert(field.number(), ValueOrUnknown::Value(value));
        Ok(())
    }

    /// Remove the `Value` (or any sibling oneof member) at `field.number()`.
    pub(crate) fn clear(&mut self, field: &FieldDescriptor) {
        self.clear_oneof_siblings(field);
        self.fields.remove(&field.number());
    }

    /// Clear any sibling oneof member; no-op for non-oneof fields.
    fn clear_oneof_siblings(&mut self, field: &FieldDescriptor) {
        let Some(oneof) = field.containing_oneof() else {
            return;
        };
        let self_number = field.number();
        for sibling in oneof.fields() {
            let n = sibling.number();
            if n == self_number {
                continue;
            }
            self.fields.remove(&n);
        }
    }

    /// Append an unknown wire-tag at `number`. Multiple tags accumulate
    /// in insertion order under the same key.
    pub(crate) fn add_unknown(&mut self, number: u32, field: buffa::UnknownField) {
        match self.fields.entry(number) {
            std::collections::btree_map::Entry::Occupied(mut occ) => match occ.get_mut() {
                ValueOrUnknown::Unknown(set) => set.push(field),
                ValueOrUnknown::Value(_) | ValueOrUnknown::Taken => {
                    let mut set = UnknownFieldSet::new();
                    set.push(field);
                    occ.insert(ValueOrUnknown::Unknown(set));
                }
            },
            std::collections::btree_map::Entry::Vacant(vac) => {
                let mut set = UnknownFieldSet::new();
                set.push(field);
                vac.insert(ValueOrUnknown::Unknown(set));
            }
        }
    }

    /// Drain unknown-field sets out of the map, leaving `Taken`
    /// sentinels behind so insertion-order is preserved across the call.
    pub(crate) fn drain_unknown(&mut self) -> Vec<(u32, UnknownFieldSet)> {
        let numbers: Vec<u32> = self
            .fields
            .iter()
            .filter_map(|(n, v)| match v {
                ValueOrUnknown::Unknown(_) => Some(*n),
                _ => None,
            })
            .collect();
        let mut out = Vec::with_capacity(numbers.len());
        for n in numbers {
            if let Some(slot) = self.fields.get_mut(&n) {
                let taken = std::mem::replace(slot, ValueOrUnknown::Taken);
                if let ValueOrUnknown::Unknown(set) = taken {
                    out.push((n, set));
                }
            }
        }
        // Tidy up the Taken sentinels — the public iterator filters them
        // anyway, but keeping them around forever wastes space.
        self.fields
            .retain(|_, v| !matches!(v, ValueOrUnknown::Taken));
        out
    }

    /// True iff no field (known or unknown) has been recorded.
    pub(crate) fn is_empty(&self) -> bool {
        self.fields
            .values()
            .all(|v| matches!(v, ValueOrUnknown::Taken))
    }
}

/// Common surface implemented by both [`FieldDescriptor`] and (one day)
/// `ExtensionDescriptor`. The storage layer dispatches through this so
/// extensions land additively.
#[allow(dead_code, reason = "extension support follow-up will add second impl")]
pub(crate) trait FieldDescriptorLike: fmt::Debug {
    fn number(&self) -> u32;
    fn kind(&self) -> Kind;
    fn is_list(&self) -> bool;
    fn is_map(&self) -> bool;
    fn is_packed(&self) -> bool;
    fn is_packable(&self) -> bool;
    fn supports_presence(&self) -> bool;
    fn containing_oneof(&self) -> Option<OneofDescriptor>;
    fn default_value(&self) -> Value;
    fn is_default_value(&self, value: &Value) -> bool;
    fn is_valid(&self, value: &Value) -> bool;
}

impl FieldDescriptorLike for FieldDescriptor {
    fn number(&self) -> u32 {
        FieldDescriptor::number(self)
    }
    fn kind(&self) -> Kind {
        FieldDescriptor::kind(self)
    }
    fn is_list(&self) -> bool {
        FieldDescriptor::is_list(self)
    }
    fn is_map(&self) -> bool {
        FieldDescriptor::is_map(self)
    }
    fn is_packed(&self) -> bool {
        FieldDescriptor::is_packed(self)
    }
    fn is_packable(&self) -> bool {
        FieldDescriptor::is_packable(self)
    }
    fn supports_presence(&self) -> bool {
        FieldDescriptor::supports_presence(self)
    }
    fn containing_oneof(&self) -> Option<OneofDescriptor> {
        FieldDescriptor::containing_oneof(self)
    }
    fn default_value(&self) -> Value {
        Value::default_value_for_field(self)
    }
    fn is_default_value(&self, value: &Value) -> bool {
        if self.is_list() || self.is_map() {
            return matches!(value, Value::List(l) if l.is_empty())
                || matches!(value, Value::Map(m) if m.is_empty());
        }
        value.is_default(&FieldDescriptor::kind(self))
    }
    fn is_valid(&self, value: &Value) -> bool {
        Value::is_valid_for_field(value, self)
    }
}
