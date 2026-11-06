use std::path::{Path, PathBuf};

use crate::catalog::{CATALOG, CatalogModel};
use crate::download::Fetcher;
use crate::errors::PullError;

const PARTIAL_SUFFIX: &str = ".partial";

/// Where pulled models live: `models` under snet's home.
pub fn models_directory(home: &Path) -> PathBuf {
    home.join("models")
}

/// The pulled file of `model` when it is there with its full length. A file
/// reaches its final name only after its hash matched, so its presence is
/// the proof.
pub fn pulled_path(home: &Path, model: &CatalogModel) -> Option<PathBuf> {
    let path = models_directory(home).join(model.file);
    let length = std::fs::metadata(&path).ok()?.len();
    (length == model.bytes).then_some(path)
}

/// Every catalog model that has been pulled, in catalog order.
pub fn pulled_models(home: &Path) -> Vec<(&'static CatalogModel, PathBuf)> {
    CATALOG
        .iter()
        .filter_map(|model| pulled_path(home, model).map(|path| (model, path)))
        .collect()
}

#[derive(Debug, PartialEq, Eq)]
pub enum Pulled {
    Downloaded(PathBuf),
    AlreadyThere(PathBuf),
}

/// Downloads `model` into the models directory unless it is there already,
/// writing a partial file that is renamed only once it checks out and
/// removed when it does not.
pub async fn pull(
    home: &Path,
    model: &CatalogModel,
    fetcher: &Fetcher,
    progress: &mut dyn FnMut(u64),
) -> Result<Pulled, PullError> {
    if let Some(path) = pulled_path(home, model) {
        return Ok(Pulled::AlreadyThere(path));
    }
    let directory = models_directory(home);
    let io_failed = |path: &Path, error: std::io::Error| PullError::Io {
        path: path.display().to_string(),
        cause: error.to_string(),
    };
    std::fs::create_dir_all(&directory).map_err(|error| io_failed(&directory, error))?;
    let partial = directory.join(format!("{}{PARTIAL_SUFFIX}", model.file));
    let downloaded = fetcher
        .download(model.url, &partial, model.bytes, model.sha256, progress)
        .await;
    if let Err(error) = downloaded {
        // A partial file that failed its checks must not linger next to the
        // verified ones; a failure to remove it is reported instead.
        if partial.exists() {
            std::fs::remove_file(&partial).map_err(|error| io_failed(&partial, error))?;
        }
        return Err(error);
    }
    let path = directory.join(model.file);
    std::fs::rename(&partial, &path).map_err(|error| io_failed(&path, error))?;
    Ok(Pulled::Downloaded(path))
}
