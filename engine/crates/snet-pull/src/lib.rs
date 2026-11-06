//! `snet pull`: downloads a model from a fixed catalog over verified TLS,
//! and keeps it only when its length and SHA-256 match the catalog.

mod catalog;
mod download;
mod errors;
mod store;

pub use catalog::{CATALOG, CatalogModel, DEFAULT_MODEL, catalog_model};
pub use download::Fetcher;
pub use errors::PullError;
pub use store::{Pulled, models_directory, pull, pulled_models, pulled_path};
