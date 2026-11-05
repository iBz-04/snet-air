use std::collections::BTreeSet;

use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{Item, ItemMod};

use crate::test_code::cfg_requires_test;

/// Where a Rust file sits, which decides whether the 400-line limit applies
///.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Placement {
    /// Under a crate's top-level `tests/` directory, which Cargo builds only
    /// as tests.
    CrateTests,
    /// A `tests.rs` under a crate's `src`, exempt only when one of these files
    /// declares it with a `cfg` that requires `test`.
    SourceTestsFile { declaring_files: Vec<String> },
    /// Any other file.
    Ordinary,
}

/// The directories holding a crate manifest, from the paths of those
/// manifests (`engine/crates/snet-x/Cargo.toml` gives `engine/crates/snet-x`).
pub fn crate_directories<'a>(
    crate_manifest_paths: impl IntoIterator<Item = &'a str>,
) -> BTreeSet<String> {
    crate_manifest_paths
        .into_iter()
        .filter_map(|path| path.strip_suffix("/Cargo.toml"))
        .map(str::to_string)
        .collect()
}

/// The part of the path inside its crate, from the innermost crate holding it.
fn within_crate<'p>(
    relative_path: &'p str,
    crate_directories: &BTreeSet<String>,
) -> Option<(&'p str, &'p str)> {
    crate_directories
        .iter()
        .filter_map(|directory| {
            relative_path
                .strip_prefix(directory.as_str())
                .and_then(|rest| rest.strip_prefix('/'))
                .map(|rest| (&relative_path[..directory.len()], rest))
        })
        .max_by_key(|(directory, _)| directory.len())
}

pub fn placement(relative_path: &str, crate_directories: &BTreeSet<String>) -> Placement {
    let Some((directory, inside)) = within_crate(relative_path, crate_directories) else {
        return Placement::Ordinary;
    };
    if inside.starts_with("tests/") {
        return Placement::CrateTests;
    }
    let Some(in_source) = inside.strip_prefix("src/") else {
        return Placement::Ordinary;
    };
    if in_source == "tests.rs" {
        return Placement::SourceTestsFile {
            declaring_files: vec![
                format!("{directory}/src/lib.rs"),
                format!("{directory}/src/main.rs"),
            ],
        };
    }
    match in_source.strip_suffix("/tests.rs") {
        Some(parent) => Placement::SourceTestsFile {
            declaring_files: vec![
                format!("{directory}/src/{parent}.rs"),
                format!("{directory}/src/{parent}/mod.rs"),
            ],
        },
        None => Placement::Ordinary,
    }
}

/// True for a file under a crate's `src`, where `#[path]` is checked.
pub fn is_crate_source(relative_path: &str, crate_directories: &BTreeSet<String>) -> bool {
    within_crate(relative_path, crate_directories)
        .is_some_and(|(_, inside)| inside.starts_with("src/"))
}

/// True when the file declares `mod tests;` at its top level with a `cfg`
/// that requires `test` on the declaration itself.
pub fn declares_test_module(source: &str) -> syn::Result<bool> {
    let file = syn::parse_file(source)?;
    for item in &file.items {
        if let Item::Mod(module) = item
            && module.ident == "tests"
            && module.content.is_none()
            && cfg_requires_test(&module.attrs)?
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The lines of every module declaration that carries `#[path]`, at any
/// depth.
pub fn module_path_attribute_lines(source: &str) -> syn::Result<Vec<usize>> {
    let file = syn::parse_file(source)?;
    let mut finder = PathAttributes::default();
    finder.visit_file(&file);
    Ok(finder.lines)
}

#[derive(Default)]
struct PathAttributes {
    lines: Vec<usize>,
}

impl<'ast> Visit<'ast> for PathAttributes {
    fn visit_item_mod(&mut self, module: &'ast ItemMod) {
        for attribute in &module.attrs {
            if attribute.path().is_ident("path") {
                self.lines.push(attribute.span().start().line);
            }
        }
        visit::visit_item_mod(self, module);
    }
}
