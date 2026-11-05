use std::env;
use std::process::ExitCode;

const GENERIC_VARIABLE: &str = "AWS_LC_SYS_USE_SYSTEM";

fn main() -> ExitCode {
    println!("cargo:rerun-if-env-changed={GENERIC_VARIABLE}");
    let target = env::var("TARGET").unwrap_or_default().replace('-', "_");
    let target_variable = format!("{GENERIC_VARIABLE}_{target}");
    println!("cargo:rerun-if-env-changed={target_variable}");

    if env::var(GENERIC_VARIABLE).as_deref() != Ok("0") {
        eprintln!(
            "snet-cloud must be built with {GENERIC_VARIABLE}=0 so that aws-lc-sys builds its bundled sources \
             instead of linking a system AWS-LC; engine/.cargo/config.toml sets it, so start the build inside engine/"
        );
        return ExitCode::FAILURE;
    }
    if let Ok(value) = env::var(&target_variable)
        && value != "0"
    {
        eprintln!(
            "{target_variable} is set to a value other than 0 and would make aws-lc-sys look for a system AWS-LC"
        );
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
