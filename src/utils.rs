use anyhow::Context;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use walkdir::WalkDir;

pub fn get_executable_path() -> anyhow::Result<PathBuf> {
    let exe_path =
        std::env::current_exe().context("failed to determine current executable path")?;
    // Resolves symbolic links (e.g., when installed via winget)
    Ok(dunce::canonicalize(&exe_path).unwrap_or(exe_path))
}

pub fn get_executable_directory() -> anyhow::Result<PathBuf> {
    Ok(get_executable_path()?
        .parent()
        .context("executable path has no parent directory")?
        .to_path_buf())
}

pub fn get_executable_path_str() -> anyhow::Result<String> {
    Ok(get_executable_path()?
        .to_str()
        .context("executable path is not valid UTF-8")?
        .to_string())
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
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect()
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
            if entry.file_type().is_file()
                && let Ok(meta) = entry.metadata()
            {
                total += meta.len();
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
pub fn delete_file_or_folder(path: &std::path::Path) {
    if path.is_dir() {
        if let Err(e) = std::fs::remove_dir_all(path) {
            log::error!("Error deleting {}: {e}", path.display());
        }
    } else if let Err(e) = std::fs::remove_file(path)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        log::error!("Error deleting {}: {e}", path.display());
    }
}

pub fn open_path(path: &std::path::Path) -> anyhow::Result<()> {
    open::that_detached(path).context("failed to open path")
}

pub fn open_url(url: &str) -> anyhow::Result<()> {
    open::that_detached(url).context("failed to open URL")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn get_hostname_returns_non_empty() {
        let hostname = get_hostname();
        assert!(!hostname.is_empty());
        assert!(!hostname.contains('.'), "should strip domain suffix");
    }

    #[test]
    fn calculate_sha256_known_value() {
        // SHA-256 of empty input
        let hash = calculate_sha256(b"");
        assert_eq!(
            hash,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn calculate_sha256_different_inputs_differ() {
        let h1 = calculate_sha256(b"hello");
        let h2 = calculate_sha256(b"world");
        assert_ne!(h1, h2);
    }

    #[test]
    fn get_total_number_of_files_empty() {
        let dir = tempfile::tempdir().unwrap();
        let paths = vec![dir.path().to_path_buf()];
        assert_eq!(get_total_number_of_files(&paths), 0);
    }

    #[test]
    fn get_total_number_of_files_counts_correctly() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "a").unwrap();
        std::fs::write(dir.path().join("b.txt"), "b").unwrap();
        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("c.txt"), "c").unwrap();
        let paths = vec![dir.path().to_path_buf()];
        assert_eq!(get_total_number_of_files(&paths), 3);
    }

    #[test]
    fn get_files_size_mb_empty() {
        let dir = tempfile::tempdir().unwrap();
        let paths = vec![dir.path().to_path_buf()];
        assert!((get_files_size_mb(&paths) - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn get_files_size_mb_nonzero() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("data.bin"), vec![0u8; 1024]).unwrap();
        let paths = vec![dir.path().to_path_buf()];
        let size = get_files_size_mb(&paths);
        assert!(size > 0.0);
        assert!(size < 0.01); // 1KB < 0.01 MB
    }

    #[test]
    fn copy_folder_recursive_copies_all() {
        let src = tempfile::tempdir().unwrap();
        std::fs::write(src.path().join("file.txt"), "content").unwrap();
        let sub = src.path().join("nested");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("inner.txt"), "inner").unwrap();

        let dst = tempfile::tempdir().unwrap();
        let dst_path = dst.path().join("copy");
        copy_folder_recursive(src.path(), &dst_path).unwrap();

        assert_eq!(
            std::fs::read_to_string(dst_path.join("file.txt")).unwrap(),
            "content"
        );
        assert_eq!(
            std::fs::read_to_string(dst_path.join("nested").join("inner.txt")).unwrap(),
            "inner"
        );
    }

    #[test]
    fn delete_file_or_folder_removes_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("to_delete.txt");
        std::fs::write(&path, "bye").unwrap();
        assert!(path.exists());
        delete_file_or_folder(&path);
        assert!(!path.exists());
    }

    #[test]
    fn delete_file_or_folder_removes_directory() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("subdir");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("file.txt"), "data").unwrap();
        assert!(sub.exists());
        delete_file_or_folder(&sub);
        assert!(!sub.exists());
    }

    #[test]
    fn delete_file_or_folder_nonexistent_noop() {
        let path = Path::new("/tmp/nonexistent_desloppify_test_file");
        delete_file_or_folder(path); // should not panic
    }
}
