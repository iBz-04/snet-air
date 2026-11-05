use std::collections::BTreeSet;

use toml::{Table, Value};

use crate::errors::RulesError;
use crate::forbidden_terms::ForbiddenTerms;
use crate::report::{Finding, Rule};

pub const WORKSPACE_MANIFEST_PATH: &str = "engine/Cargo.toml";
pub const CRATE_NAME_PREFIX: &str = "snet-";
pub const WORKSPACE_LICENSE: &str = "MPL-2.0";

#[derive(Debug, Clone)]
pub struct ManifestSource {
    pub relative_path: String,
    pub text: String,
}

/// Checks the workspace manifest and every other `Cargo.toml`: each workspace crate sits at `engine/crates/<name>/Cargo.toml`,
/// is named `snet-<role>`, is a workspace member and inherits publish,
/// licence and lints; and each member is such a crate.
pub fn check_manifests(
    workspace: &ManifestSource,
    crate_manifests: &[ManifestSource],
    terms: &ForbiddenTerms,
) -> Result<Vec<Finding>, RulesError> {
    let mut findings = Vec::new();
    let workspace_table = parse_manifest(workspace, terms)?;
    let workspace_path = terms.display_path(&workspace.relative_path);
    let mut report = |path: &str, detail: String| {
        findings.push(Finding {
            path: path.to_string(),
            line: 0,
            rule: Rule::Manifest,
            detail,
        })
    };

    let workspace_package = workspace_table
        .get("workspace")
        .and_then(|workspace| workspace.get("package"));
    if workspace_package.and_then(|package| package.get("publish")) != Some(&Value::Boolean(false))
    {
        report(
            &workspace_path,
            "workspace.package.publish must be false".to_string(),
        );
    }
    if workspace_package.and_then(|package| package.get("license"))
        != Some(&Value::String(WORKSPACE_LICENSE.to_string()))
    {
        report(
            &workspace_path,
            format!("workspace.package.license must be \"{WORKSPACE_LICENSE}\""),
        );
    }

    let members = workspace_members(&workspace_table);
    let members = match members {
        Some(members) => members,
        None => {
            report(
                &workspace_path,
                "workspace.members must be a list of paths".to_string(),
            );
            BTreeSet::new()
        }
    };

    let mut members_with_manifests = BTreeSet::new();
    for manifest in crate_manifests {
        let table = parse_manifest(manifest, terms)?;
        let path = terms.display_path(&manifest.relative_path);
        let Some(package) = table.get("package").and_then(Value::as_table) else {
            report(
                &path,
                "every manifest besides the workspace's must describe one crate".to_string(),
            );
            continue;
        };
        let Some(name) = package.get("name").and_then(Value::as_str) else {
            report(&path, "package.name is missing".to_string());
            continue;
        };

        if !is_crate_name_allowed(name) {
            report(
                &path,
                format!("crate names are {CRATE_NAME_PREFIX}<role> in lowercase kebab-case"),
            );
        }
        if manifest.relative_path != format!("engine/crates/{name}/Cargo.toml") {
            report(
                &path,
                "a crate's manifest sits at engine/crates/<its name>/Cargo.toml".to_string(),
            );
        }
        let member_path = format!("crates/{name}");
        if members.contains(&member_path) {
            members_with_manifests.insert(member_path);
        } else {
            report(&path, "the crate is not a workspace member".to_string());
        }
        for field in ["publish", "license"] {
            if !inherits_from_workspace(package.get(field)) {
                report(
                    &path,
                    format!("package.{field} must be {{ workspace = true }}"),
                );
            }
        }
        if !inherits_from_workspace(table.get("lints")) {
            report(&path, "[lints] must be workspace = true".to_string());
        }
    }

    for member in members.difference(&members_with_manifests) {
        report(
            &workspace_path,
            format!(
                "member {} has no matching snet crate manifest in the repository",
                terms.display_path(member)
            ),
        );
    }

    Ok(findings)
}

fn parse_manifest(manifest: &ManifestSource, terms: &ForbiddenTerms) -> Result<Table, RulesError> {
    manifest
        .text
        .parse::<Table>()
        .map_err(|error| RulesError::ManifestUnparsable {
            path: terms.display_path(&manifest.relative_path),
            message: error.message().to_string(),
        })
}

fn workspace_members(workspace_table: &Table) -> Option<BTreeSet<String>> {
    let members = workspace_table
        .get("workspace")?
        .get("members")?
        .as_array()?;
    members
        .iter()
        .map(|member| member.as_str().map(str::to_string))
        .collect()
}

fn inherits_from_workspace(value: Option<&Value>) -> bool {
    value
        .and_then(Value::as_table)
        .and_then(|table| table.get("workspace"))
        == Some(&Value::Boolean(true))
}

fn is_crate_name_allowed(name: &str) -> bool {
    let Some(role) = name.strip_prefix(CRATE_NAME_PREFIX) else {
        return false;
    };
    !role.is_empty()
        && !role.starts_with('-')
        && !role.ends_with('-')
        && role
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}
