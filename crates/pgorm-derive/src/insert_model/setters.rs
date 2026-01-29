//! Setter methods generation.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use crate::common::syn_types::option_inner;

pub(super) fn generate_with_setters(
    fields: &syn::punctuated::Punctuated<syn::Field, syn::Token![,]>,
) -> TokenStream {
    let mut setters = Vec::new();

    for field in fields.iter() {
        let field_ident = match &field.ident {
            Some(ident) => ident.clone(),
            None => continue,
        };
        let field_ty = &field.ty;
        let setter_name = format_ident!("with_{}", field_ident);

        // Check if the field type is Option<T>
        if let Some(inner_ty) = option_inner(field_ty) {
            // For Option<T>: generate both with_<field>(T) and with_<field>_opt(Option<T>)
            let setter_opt_name = format_ident!("with_{}_opt", field_ident);

            setters.push(quote! {
                /// Builder-style setter: sets the field to `Some(v)`.
                #[inline]
                pub fn #setter_name(mut self, v: #inner_ty) -> Self {
                    self.#field_ident = ::std::option::Option::Some(v);
                    self
                }

                /// Builder-style setter: sets the field to the given Option value.
                #[inline]
                pub fn #setter_opt_name(mut self, v: ::std::option::Option<#inner_ty>) -> Self {
                    self.#field_ident = v;
                    self
                }
            });
        } else {
            // For non-Option types: generate with_<field>(T)
            setters.push(quote! {
                /// Builder-style setter.
                #[inline]
                pub fn #setter_name(mut self, v: #field_ty) -> Self {
                    self.#field_ident = v;
                    self
                }
            });
        }
    }

    quote! {
        #(#setters)*
    }
}
