use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{Attribute, ForeignItem, ImplItem, Item, TraitItem};

use crate::test_code::attributes_require_test;

pub const MAXIMUM_COUNTED_LINES: usize = 400;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceLength {
    TestFile,
    Counted(usize),
}

pub fn measure_source(source: &str) -> syn::Result<SourceLength> {
    let file = syn::parse_file(source)?;
    if attributes_require_test(&file.attrs)? {
        return Ok(SourceLength::TestFile);
    }

    let mut test_lines = TestItemLines::default();
    test_lines.visit_file(&file);
    if let Some(error) = test_lines.first_error {
        return Err(error);
    }

    let total_lines = source.lines().count();
    Ok(SourceLength::Counted(
        total_lines.saturating_sub(test_lines.lines),
    ))
}

/// Sums the line spans of test items. A test item is not descended into, so a
/// test item nested in another is not subtracted twice.
#[derive(Default)]
struct TestItemLines {
    lines: usize,
    first_error: Option<syn::Error>,
}

impl TestItemLines {
    fn is_test_item(&mut self, attributes: &[Attribute], node: &impl Spanned) -> bool {
        match attributes_require_test(attributes) {
            Ok(true) => {
                let span = node.span();
                self.lines += span.end().line.saturating_sub(span.start().line) + 1;
                true
            }
            Ok(false) => false,
            Err(error) => {
                self.first_error.get_or_insert(error);
                false
            }
        }
    }
}

impl<'ast> Visit<'ast> for TestItemLines {
    fn visit_item(&mut self, item: &'ast Item) {
        if !self.is_test_item(item_attributes(item), item) {
            visit::visit_item(self, item);
        }
    }

    fn visit_impl_item(&mut self, item: &'ast ImplItem) {
        if !self.is_test_item(impl_item_attributes(item), item) {
            visit::visit_impl_item(self, item);
        }
    }

    fn visit_trait_item(&mut self, item: &'ast TraitItem) {
        if !self.is_test_item(trait_item_attributes(item), item) {
            visit::visit_trait_item(self, item);
        }
    }

    fn visit_foreign_item(&mut self, item: &'ast ForeignItem) {
        if !self.is_test_item(foreign_item_attributes(item), item) {
            visit::visit_foreign_item(self, item);
        }
    }
}

// The item enums are non-exhaustive. A variant syn adds later has no
// attributes here, so its lines are counted, which errs toward a longer file.
fn item_attributes(item: &Item) -> &[Attribute] {
    match item {
        Item::Const(item) => &item.attrs,
        Item::Enum(item) => &item.attrs,
        Item::ExternCrate(item) => &item.attrs,
        Item::Fn(item) => &item.attrs,
        Item::ForeignMod(item) => &item.attrs,
        Item::Impl(item) => &item.attrs,
        Item::Macro(item) => &item.attrs,
        Item::Mod(item) => &item.attrs,
        Item::Static(item) => &item.attrs,
        Item::Struct(item) => &item.attrs,
        Item::Trait(item) => &item.attrs,
        Item::TraitAlias(item) => &item.attrs,
        Item::Type(item) => &item.attrs,
        Item::Union(item) => &item.attrs,
        Item::Use(item) => &item.attrs,
        _ => &[],
    }
}

fn impl_item_attributes(item: &ImplItem) -> &[Attribute] {
    match item {
        ImplItem::Const(item) => &item.attrs,
        ImplItem::Fn(item) => &item.attrs,
        ImplItem::Type(item) => &item.attrs,
        ImplItem::Macro(item) => &item.attrs,
        _ => &[],
    }
}

fn trait_item_attributes(item: &TraitItem) -> &[Attribute] {
    match item {
        TraitItem::Const(item) => &item.attrs,
        TraitItem::Fn(item) => &item.attrs,
        TraitItem::Type(item) => &item.attrs,
        TraitItem::Macro(item) => &item.attrs,
        _ => &[],
    }
}

fn foreign_item_attributes(item: &ForeignItem) -> &[Attribute] {
    match item {
        ForeignItem::Fn(item) => &item.attrs,
        ForeignItem::Static(item) => &item.attrs,
        ForeignItem::Type(item) => &item.attrs,
        ForeignItem::Macro(item) => &item.attrs,
        _ => &[],
    }
}
