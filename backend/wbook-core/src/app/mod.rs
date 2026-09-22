use camino::Utf8PathBuf;

use serde::{Deserialize, Serialize};
use specta::Type;

mod session_manager;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
pub struct Params {
    pub data_dir: Utf8PathBuf,
    pub config_dir: Utf8PathBuf,
}

pub struct Wbook {
    pub start_params: Params,
}
