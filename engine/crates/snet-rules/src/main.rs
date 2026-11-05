use std::path::PathBuf;
use std::process::ExitCode;

use snet_rules::errors::RulesError;
use snet_rules::forbidden_terms::ForbiddenTerms;
use snet_rules::report::Finding;
use snet_rules::repository::check_repository;

const EXIT_FINDINGS: u8 = 1;
const EXIT_ERROR: u8 = 2;

fn main() -> ExitCode {
    match run() {
        Ok(findings) if findings.is_empty() => ExitCode::SUCCESS,
        Ok(findings) => {
            for finding in &findings {
                println!("{finding}");
            }
            eprintln!("snet-rules: {} finding(s)", findings.len());
            ExitCode::from(EXIT_FINDINGS)
        }
        Err(error) => {
            eprintln!("snet-rules: {error}");
            ExitCode::from(EXIT_ERROR)
        }
    }
}

fn run() -> Result<Vec<Finding>, RulesError> {
    let mut arguments = std::env::args_os().skip(1);
    let root = PathBuf::from(arguments.next().ok_or(RulesError::Usage)?);
    if arguments.next().is_some() {
        return Err(RulesError::Usage);
    }
    let terms = ForbiddenTerms::compiled()?;
    check_repository(&root, &terms)
}
