use syn::punctuated::Punctuated;
use syn::{Attribute, Meta, Token};

/// True when the attributes carry `#[test]` or a `cfg` that requires `test`:
/// `cfg(test)`, or `cfg(all(...))` with `test` as a direct argument. A `test`
/// nested deeper, as in `cfg(all(not(test), unix))`, does not count.
pub fn attributes_require_test(attributes: &[Attribute]) -> syn::Result<bool> {
    let is_test_function = attributes.iter().any(|attribute| {
        attribute.path().is_ident("test") && matches!(attribute.meta, Meta::Path(_))
    });
    Ok(is_test_function || cfg_requires_test(attributes)?)
}

/// True when a `cfg` attribute among these requires `test` in the sense of
/// `attributes_require_test`; `#[test]` and `cfg_attr` do not count.
pub fn cfg_requires_test(attributes: &[Attribute]) -> syn::Result<bool> {
    for attribute in attributes {
        if attribute.path().is_ident("cfg")
            && cfg_predicate_requires_test(&attribute.parse_args()?)?
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn cfg_predicate_requires_test(predicate: &Meta) -> syn::Result<bool> {
    match predicate {
        Meta::Path(path) => Ok(path.is_ident("test")),
        Meta::List(list) if list.path.is_ident("all") => {
            let arguments =
                list.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)?;
            Ok(arguments
                .iter()
                .any(|argument| matches!(argument, Meta::Path(path) if path.is_ident("test"))))
        }
        Meta::List(_) | Meta::NameValue(_) => Ok(false),
    }
}
