use std::process::ExitCode;

fn main() -> ExitCode {
    ExitCode::from(snet_command::run_from_process())
}
