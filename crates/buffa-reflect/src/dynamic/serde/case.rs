//! Snake-case ↔ lowerCamelCase conversions used to match proto field
//! names against JSON keys.

/// Convert `snake_case` (or `Title_Case`) to `lowerCamelCase`.
pub(crate) fn snake_to_lower_camel(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut upper_next = false;
    for (i, ch) in input.chars().enumerate() {
        if ch == '_' {
            upper_next = true;
            continue;
        }
        if upper_next {
            for u in ch.to_uppercase() {
                out.push(u);
            }
            upper_next = false;
        } else if i == 0 {
            for l in ch.to_lowercase() {
                out.push(l);
            }
        } else {
            out.push(ch);
        }
    }
    out
}

/// Lower-camel-case to snake_case (used for `FieldMask` JSON paths).
pub(crate) fn lower_camel_to_snake(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 4);
    for ch in input.chars() {
        if ch.is_ascii_uppercase() {
            out.push('_');
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}
