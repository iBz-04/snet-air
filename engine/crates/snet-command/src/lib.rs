//! The `snet` command: ask a question about local files, answered by a
//! local model that reads them and a cloud model that never sees them.

mod arguments;
mod ask;
mod documents;
mod environment;
mod exit;
#[cfg(unix)]
mod hardening;
mod interactive;
mod output;
mod settings;
#[cfg(test)]
mod test_support;

pub use environment::{Environment, ProcessEnvironment};
pub use exit::{CommandError, Exit};

/// Replaces the panic message with its location only, so a panic can never
/// print material or the API key.
pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let location = info
            .location()
            .map(|location| format!("{}:{}", location.file(), location.line()))
            .unwrap_or_else(|| "an unknown place".to_string());
        eprintln!("snet: internal error at {location}");
    }));
}

/// The binary's entry point: this process's arguments, environment and
/// standard streams.
pub fn run_from_process() -> u8 {
    install_panic_hook();
    run(
        std::env::args_os().collect(),
        &ProcessEnvironment,
        &mut std::io::stdout(),
        &mut std::io::stderr(),
    )
}

/// Runs one command and returns its exit code.
pub fn run(
    arguments: Vec<std::ffi::OsString>,
    environment: &dyn Environment,
    output: &mut dyn std::io::Write,
    errors: &mut dyn std::io::Write,
) -> u8 {
    let result = harden()
        .and_then(|()| arguments::from_os(arguments))
        .and_then(|command| match command {
            arguments::Command::Interactive if interactive::available() => {
                interactive::run(environment)
            }
            arguments::Command::Interactive => Err(CommandError::usage(arguments::USAGE)),
            arguments::Command::Help => {
                writeln!(output, "{}", arguments::USAGE)
                    .map_err(|error| CommandError::failed(error.to_string()))?;
                Ok(Exit::Success)
            }
            arguments::Command::Version => {
                writeln!(output, "snet {}", env!("CARGO_PKG_VERSION"))
                    .map_err(|error| CommandError::failed(error.to_string()))?;
                Ok(Exit::Success)
            }
            arguments::Command::Ask(ask_arguments) => {
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                    .map_err(|error| {
                        CommandError::failed(format!("the runtime did not start: {error}"))
                    })?;
                runtime.block_on(ask::run(&ask_arguments, environment, output, errors))
            }
        });
    match result {
        Ok(exit) => exit.code(),
        Err(error) => {
            // Standard error is the last place left to report to.
            let _unreported = writeln!(errors, "snet: {error}");
            error.exit.code()
        }
    }
}

#[cfg(unix)]
fn harden() -> Result<(), CommandError> {
    hardening::forbid_core_dumps()
}

/// Windows writes no core dump unless the user set one up.
#[cfg(not(unix))]
fn harden() -> Result<(), CommandError> {
    Ok(())
}
