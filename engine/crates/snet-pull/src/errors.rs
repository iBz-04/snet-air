#[derive(Debug, thiserror::Error)]
pub enum PullError {
    #[error("`{name}` is not a model snet can pull; choose one of: {choices}")]
    NotInCatalog { name: String, choices: String },
    #[error("the address `{url}` is not an https URL with a host")]
    Url { url: String },
    #[error("the download was sent to `{host}`, which is not the model host")]
    HostRefused { host: String },
    #[error("the download was redirected more than {limit} times")]
    TooManyRedirects { limit: u8 },
    #[error("connecting to {host} failed: {cause}; check your internet connection")]
    Connect { host: String, cause: String },
    #[error("the download from {host} stalled for {seconds} seconds")]
    Stalled { host: String, seconds: u64 },
    #[error("the HTTP exchange failed: {cause}")]
    Http { cause: String },
    #[error("the model host answered status {status}")]
    Status { status: u16 },
    #[error("the file is {got} bytes, not the {expected} the catalog pins")]
    Length { expected: u64, got: u64 },
    #[error("the file's SHA-256 is {got}, not the {expected} the catalog pins; it was deleted")]
    Checksum { expected: String, got: String },
    #[error("writing the model under {path} failed: {cause}")]
    Io { path: String, cause: String },
}
