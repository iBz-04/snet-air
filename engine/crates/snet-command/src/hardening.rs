use crate::exit::CommandError;

/// Stops the process from ever writing a core dump, which could hold the
/// API key or material. On Linux a zero
/// core size is ignored when core dumps are piped to a program, so the
/// process is also marked not dumpable.
pub fn forbid_core_dumps() -> Result<(), CommandError> {
    let hardening = |step: &str, error: rustix::io::Errno| {
        CommandError::failed(format!("hardening failed: {step}: {error}"))
    };
    rustix::process::setrlimit(
        rustix::process::Resource::Core,
        rustix::process::Rlimit {
            current: Some(0),
            maximum: Some(0),
        },
    )
    .map_err(|error| hardening("setting the core-dump size limit to zero", error))?;
    #[cfg(target_os = "linux")]
    rustix::process::set_dumpable_behavior(rustix::process::DumpableBehavior::NotDumpable)
        .map_err(|error| hardening("marking the process not dumpable", error))?;
    Ok(())
}
