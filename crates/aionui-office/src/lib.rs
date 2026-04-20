pub mod conversion;
pub mod error;
pub mod port;
pub mod snapshot;
pub mod star_office;
pub mod types;
pub mod watch_manager;

pub use conversion::ConversionService;
pub use error::OfficeError;
pub use snapshot::SnapshotService;
pub use star_office::StarOfficeDetector;
pub use types::{DocType, OfficecliStatus};
pub use watch_manager::{
    DefaultProcessSpawner, OfficecliWatchManager, ProcessHandle, ProcessSpawner,
};
