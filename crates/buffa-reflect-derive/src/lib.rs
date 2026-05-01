//! Proc-macro derive for [`buffa_reflect::ReflectMessage`].
//!
//! See the `buffa-reflect` crate documentation for the full reflection API.
//! This crate is exposed transparently through the default `derive` feature
//! of `buffa-reflect`.
//!
//! # Recognized attributes
//!
//! The macro reads `#[buffa_reflect(...)]` attributes on the annotated type.
//! Exactly one of the two descriptor-binding keys must appear:
//!
//! | key                         | value                                                    |
//! | --------------------------- | -------------------------------------------------------- |
//! | `descriptor_pool`           | Rust expression yielding `&buffa_reflect::DescriptorPool`|
//! | `file_descriptor_set_bytes` | Rust expression yielding `&[u8]` (a serialized FDS)      |
//! | `message_name`              | Fully-qualified proto name, e.g. `"acme.api.v1.User"`    |
//!
//! `buffa-reflect-build` injects all three on every generated message; you
//! rarely need to write them by hand.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{Attribute, DeriveInput, Error, Expr, ExprLit, Lit, LitStr, parse_macro_input};

/// Derive `buffa_reflect::ReflectMessage` for the annotated message struct.
///
/// See the crate-level documentation for the recognized attribute shapes.
#[proc_macro_derive(ReflectMessage, attributes(buffa_reflect))]
pub fn derive_reflect_message(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand(&input)
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

fn expand(input: &DeriveInput) -> Result<TokenStream2, Error> {
    let parsed = parse_attributes(&input.attrs)?;
    let ident = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let message_name = parsed.message_name.ok_or_else(|| {
        Error::new_spanned(
            input,
            "missing `#[buffa_reflect(message_name = \"...\")]` attribute",
        )
    })?;
    let message_name_lit = LitStr::new(&message_name.value(), message_name.span());

    let body = match (parsed.pool_expr, parsed.bytes_expr) {
        (None, None) => {
            return Err(Error::new_spanned(
                input,
                "missing `#[buffa_reflect(...)]` binding: provide either \
                 `descriptor_pool = \"...\"` or `file_descriptor_set_bytes = \"...\"`",
            ));
        }
        (Some(_), Some(_)) => {
            return Err(Error::new_spanned(
                input,
                "conflicting `#[buffa_reflect(...)]` bindings: \
                 provide exactly one of `descriptor_pool` / `file_descriptor_set_bytes`",
            ));
        }
        (Some(pool), None) => {
            let pool_expr: Expr = pool.parse()?;
            quote! {
                let __pool: &::buffa_reflect::DescriptorPool = &#pool_expr;
                __pool
                    .get_message_by_name(#message_name_lit)
                    .expect(concat!(
                        "buffa-reflect: descriptor for `",
                        #message_name_lit,
                        "` not found",
                    ))
            }
        }
        (None, Some(bytes)) => {
            let bytes_expr: Expr = bytes.parse()?;
            quote! {
                static __INIT: ::std::sync::OnceLock<::buffa_reflect::DescriptorPool> =
                    ::std::sync::OnceLock::new();
                let __pool = __INIT.get_or_init(|| {
                    ::buffa_reflect::DescriptorPool::decode(#bytes_expr)
                        .expect("buffa-reflect: invalid FileDescriptorSet")
                });
                __pool
                    .get_message_by_name(#message_name_lit)
                    .expect(concat!(
                        "buffa-reflect: descriptor for `",
                        #message_name_lit,
                        "` not found",
                    ))
            }
        }
    };

    Ok(quote! {
        #[automatically_derived]
        impl #impl_generics ::buffa_reflect::ReflectMessage for #ident #ty_generics #where_clause {
            fn descriptor(&self) -> ::buffa_reflect::MessageDescriptor {
                #body
            }
        }
    })
}

#[derive(Default)]
struct ParsedAttrs {
    message_name: Option<LitStr>,
    pool_expr: Option<LitStr>,
    bytes_expr: Option<LitStr>,
}

fn parse_attributes(attrs: &[Attribute]) -> Result<ParsedAttrs, Error> {
    let mut out = ParsedAttrs::default();
    // `buffa-reflect-build` keys per-message attributes by FQN, but the
    // codegen prefix-match also picks up parent-FQN attributes on nested
    // messages. So we may legitimately see multiple `message_name` values
    // here — keep the longest (= most specific FQN).
    let mut all_names: Vec<LitStr> = Vec::new();
    for attr in attrs {
        if !attr.path().is_ident("buffa_reflect") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            let key = meta
                .path
                .get_ident()
                .ok_or_else(|| meta.error("expected a key like `message_name = \"...\"`"))?
                .to_string();
            let value: Expr = meta.value()?.parse()?;
            let lit = match value {
                Expr::Lit(ExprLit {
                    lit: Lit::Str(s), ..
                }) => s,
                other => {
                    return Err(Error::new_spanned(other, "expected a string literal value"));
                }
            };
            match key.as_str() {
                "message_name" => all_names.push(lit),
                "descriptor_pool" => set_once(&mut out.pool_expr, lit, "descriptor_pool")?,
                "file_descriptor_set_bytes" => {
                    set_once(&mut out.bytes_expr, lit, "file_descriptor_set_bytes")?;
                }
                other => {
                    return Err(meta.error(format!(
                        "unknown `buffa_reflect` key `{other}` (expected `message_name`, \
                         `descriptor_pool`, or `file_descriptor_set_bytes`)"
                    )));
                }
            }
            Ok(())
        })?;
    }
    out.message_name = all_names.into_iter().max_by_key(|lit| lit.value().len());
    Ok(out)
}

fn set_once(slot: &mut Option<LitStr>, value: LitStr, key: &str) -> Result<(), Error> {
    if slot.is_some() {
        return Err(Error::new(
            value.span(),
            format!("duplicate `{key}` in `#[buffa_reflect(...)]`"),
        ));
    }
    *slot = Some(value);
    Ok(())
}
