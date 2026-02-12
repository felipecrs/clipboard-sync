use sha2::{Digest, Sha256};
use std::path::PathBuf;
use walkdir::WalkDir;

pub fn get_executable_path() -> PathBuf {
    let exe_path = std::env::current_exe().unwrap();
    std::fs::canonicalize(&exe_path).unwrap_or(exe_path)
}

pub fn get_executable_directory() -> PathBuf {
    get_executable_path().parent().unwrap().to_path_buf()
}

/// Get the hostname of this machine (first part before any dot).
pub fn get_hostname() -> String {
    let full = gethostname::gethostname();
    let full_str = full.to_string_lossy();
    full_str.split('.').next().unwrap_or(&full_str).to_string()
}

/// Calculate the SHA-256 hash of a byte slice.
pub fn calculate_sha256(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

/// Get the total number of files (not directories) recursively in a list of paths.
pub fn get_total_number_of_files(paths: &[PathBuf]) -> u32 {
    let mut count = 0u32;
    for path in paths {
        for entry in WalkDir::new(path).into_iter().flatten() {
            if entry.file_type().is_file() {
                count += 1;
            }
        }
    }
    count
}

/// Get the total size of files in megabytes.
pub fn get_files_size_mb(paths: &[PathBuf]) -> f64 {
    let mut total: u64 = 0;
    for path in paths {
        for entry in WalkDir::new(path).into_iter().flatten() {
            if entry.file_type().is_file() {
                if let Ok(meta) = entry.metadata() {
                    total += meta.len();
                }
            }
        }
    }
    total as f64 / (1024.0 * 1024.0)
}

/// Copy a folder recursively from source to destination.
pub fn copy_folder_recursive(
    source: &std::path::Path,
    destination: &std::path::Path,
) -> std::io::Result<()> {
    std::fs::create_dir_all(destination)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let dest_path = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_folder_recursive(&entry.path(), &dest_path)?;
        } else {
            std::fs::copy(entry.path(), dest_path)?;
        }
    }
    Ok(())
}

/// Delete a file or folder recursively, logging errors.
///
/// Uses rmbrr's POSIX-semantics delete for files on Windows (immediate namespace
/// removal, ignores readonly attributes). Standard library for directories.
pub fn delete_file_or_folder(path: &std::path::Path) {
    if path.is_dir() {
        if let Err(e) = std::fs::remove_dir_all(path) {
            log::error!("Error deleting {}: {e}", path.display());
        }
    } else if let Err(e) = rmbrr::winapi::delete_file(path) {
        if e.kind() != std::io::ErrorKind::NotFound {
            log::error!("Error deleting {}: {e}", path.display());
        }
    }
}

/// Open a path in the system file explorer.
pub fn open_path(path: &std::path::Path) {
    let _ = open::that_detached(path);
}

/// Open a URL in the default browser.
pub fn open_url(url: &str) {
    let _ = open::that_detached(url);
}
