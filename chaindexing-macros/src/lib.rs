//! # Chaindexing Macros
//!
//! This crate provides procedural macros for the Chaindexing library.
//!
//! **Note**: These macros are re-exported by the main `chaindexing` crate.
//! Users should import them from `chaindexing` instead of depending on this crate directly:
//!
//! ```text
//! use chaindexing::state_migrations;
//! // or
//! use chaindexing::prelude::*;
//! ```

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, Expr, ExprArray, Lit};

/// Validates SQL migration strings at compile time and re-emits them as a
/// `&[&'static str]` slice literal.
///
/// # Example
/// ```
/// use chaindexing_macros::state_migrations;
/// const MIGRATIONS: &[&str] = state_migrations!([
///     r#"CREATE TABLE IF NOT EXISTS foo (id BIGSERIAL PRIMARY KEY)"#,
///     r#"CREATE INDEX IF NOT EXISTS idx_id ON foo(id)"#
/// ]);
/// ```
#[proc_macro]
pub fn state_migrations(input: TokenStream) -> TokenStream {
    // Expect an array literal like ["SQL1", "SQL2", ...]
    let ExprArray { elems, .. } = parse_macro_input!(input as ExprArray);

    let dialect = sqlparser::dialect::PostgreSqlDialect {};

    for expr in &elems {
        let lit_str = match expr {
            Expr::Lit(expr_lit) => match &expr_lit.lit {
                Lit::Str(s) => s,
                _ => {
                    return syn::Error::new_spanned(
                        expr,
                        "state_migrations! expects string literals",
                    )
                    .to_compile_error()
                    .into();
                }
            },
            _ => {
                return syn::Error::new_spanned(expr, "state_migrations! expects string literals")
                    .to_compile_error()
                    .into();
            }
        };

        if let Err(e) = sqlparser::parser::Parser::parse_sql(&dialect, &lit_str.value()) {
            let msg = format!("SQL parse error: {e}");
            return quote! { compile_error!(#msg); }.into();
        }
    }

    // All good – turn the string literals into a slice reference.
    let elems_iter = elems.iter();
    quote! {
        &[ #( #elems_iter ),* ]
    }
    .into()
}
