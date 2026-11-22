//! Model packs, device classification and the model recommendation.
//!
//! This crate is deliberately free of I/O beyond the pack folder it is handed:
//! the network, the hardware probe and the UI live in the desktop shell, while
//! the catalog, the thresholds and the on-disk install rules live here where
//! they can be tested without a GUI or a download.

#![deny(clippy::expect_used, clippy::unwrap_used)]

pub mod catalog;
pub mod device;
pub mod install;

pub use catalog::{
    FileFormat, FileRole, ModelPack, PACKS, PackFile, PackPaths, Tier, default_pack, pack,
    pack_dir, pack_paths,
};
pub use device::{
    Criterion, DeviceClass, DeviceProfile, Recommendation, classify, criteria, recommend, score,
};
pub use install::{
    ACTIVE, InstallState, Manifest, PackStatus, active_pack, clear_active, disk_requirement_mb,
    hash_file, installed_bytes, prepare_file, promote, recover, remove, set_active, status, verify,
};
