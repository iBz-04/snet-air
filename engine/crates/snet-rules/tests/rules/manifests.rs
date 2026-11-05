use crate::support::{SAMPLE_CRATE_MANIFEST, TestResult, WORKSPACE_MANIFEST, terms_listing};
use snet_rules::errors::RulesError;
use snet_rules::manifests::{ManifestSource, check_manifests};
use snet_rules::report::Finding;

fn source(relative_path: &str, text: &str) -> ManifestSource {
    ManifestSource {
        relative_path: relative_path.to_string(),
        text: text.to_string(),
    }
}

fn check(workspace_text: &str, crates: &[(&str, &str)]) -> TestResult<Vec<Finding>> {
    let terms = terms_listing(&["plantedname"])?;
    let crate_sources: Vec<ManifestSource> = crates
        .iter()
        .map(|(path, text)| source(path, text))
        .collect();
    Ok(check_manifests(
        &source("engine/Cargo.toml", workspace_text),
        &crate_sources,
        &terms,
    )?)
}

fn details(findings: &[Finding]) -> Vec<String> {
    findings
        .iter()
        .map(|finding| finding.detail.clone())
        .collect()
}

const SAMPLE_PATH: &str = "engine/crates/snet-sample/Cargo.toml";

#[test]
fn a_conforming_workspace_passes() {
    let findings = check(WORKSPACE_MANIFEST, &[(SAMPLE_PATH, SAMPLE_CRATE_MANIFEST)]).unwrap();
    assert!(findings.is_empty(), "{findings:?}");
}

#[test]
fn a_crate_without_the_prefix_is_reported() {
    let workspace = WORKSPACE_MANIFEST.replace("crates/snet-sample", "crates/sample");
    let manifest = SAMPLE_CRATE_MANIFEST.replace("snet-sample", "sample");
    let findings = check(
        &workspace,
        &[("engine/crates/sample/Cargo.toml", &manifest)],
    )
    .unwrap();
    assert_eq!(
        details(&findings),
        vec!["crate names are snet-<role> in lowercase kebab-case"]
    );
}

#[test]
fn a_crate_in_the_wrong_directory_is_reported() {
    let workspace = WORKSPACE_MANIFEST.replace("crates/snet-sample", "tools/snet-sample");
    let findings = check(
        &workspace,
        &[("engine/tools/snet-sample/Cargo.toml", SAMPLE_CRATE_MANIFEST)],
    )
    .unwrap();
    let details = details(&findings);
    assert!(
        details.contains(
            &"a crate's manifest sits at engine/crates/<its name>/Cargo.toml".to_string()
        )
    );
    assert!(details.contains(&"the crate is not a workspace member".to_string()));
}

#[test]
fn a_crate_that_is_not_a_member_is_reported() {
    let workspace = WORKSPACE_MANIFEST.replace("\"crates/snet-sample\"", "");
    let findings = check(&workspace, &[(SAMPLE_PATH, SAMPLE_CRATE_MANIFEST)]).unwrap();
    assert_eq!(
        details(&findings),
        vec!["the crate is not a workspace member"]
    );
}

#[test]
fn a_member_without_a_crate_manifest_is_reported() {
    let findings = check(WORKSPACE_MANIFEST, &[]).unwrap();
    assert_eq!(
        details(&findings),
        vec!["member crates/snet-sample has no matching snet crate manifest in the repository"]
    );
}

#[test]
fn missing_inheritance_is_reported_per_field() {
    let manifest = "[package]\nname = \"snet-sample\"\npublish = false\nlicense = \"MPL-2.0\"\n";
    let findings = check(WORKSPACE_MANIFEST, &[(SAMPLE_PATH, manifest)]).unwrap();
    assert_eq!(
        details(&findings),
        vec![
            "package.publish must be { workspace = true }",
            "package.license must be { workspace = true }",
            "[lints] must be workspace = true",
        ]
    );
}

#[test]
fn workspace_publish_and_licence_are_checked() {
    let workspace = WORKSPACE_MANIFEST
        .replace("publish = false", "publish = true")
        .replace("MPL-2.0", "MIT");
    let findings = check(&workspace, &[(SAMPLE_PATH, SAMPLE_CRATE_MANIFEST)]).unwrap();
    assert_eq!(
        details(&findings),
        vec![
            "workspace.package.publish must be false",
            "workspace.package.license must be \"MPL-2.0\"",
        ]
    );
}

#[test]
fn a_manifest_without_a_package_is_reported() {
    let findings = check(WORKSPACE_MANIFEST, &[(SAMPLE_PATH, "[workspace]\n")]).unwrap();
    let details = details(&findings);
    assert!(
        details.contains(
            &"every manifest besides the workspace's must describe one crate".to_string()
        )
    );
}

#[test]
fn an_unparsable_manifest_is_an_error() {
    let result = check(WORKSPACE_MANIFEST, &[(SAMPLE_PATH, "[package\n")]);
    let error = result.err().unwrap();
    assert!(matches!(
        error.downcast_ref::<RulesError>(),
        Some(RulesError::ManifestUnparsable { .. })
    ));
}
