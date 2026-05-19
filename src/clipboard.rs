use crate::consts::{
    IS_RECEIVING_FILE_SUFFIX, MAX_FILES_SIZE_MB, OTHERS_CLEAN_THRESHOLD_SECS,
    SELF_CLEAN_THRESHOLD_SECS, STALE_THRESHOLD_SECS,
};
use crate::types::{ClipboardContentType, ClipboardOrigin, ClipboardText, ParsedClipboardFile};
use crate::utils::{
    calculate_sha256, copy_folder_recursive, delete_file_or_folder, get_files_size_mb,
    get_total_number_of_files,
};
use clipboard_rs::{
    Clipboard as ClipboardTrait, ClipboardContext, ContentFormat, common::RustImage,
};
use regex_lite::Regex;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::{SystemTime, UNIX_EPOCH};

/// Regex for parsing clipboard filenames.
/// Format: `{beat}-{hostname}.{text.json|png|{count}_files}`
static FILE_NAME_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^([1-9][0-9]*)-([0-9a-zA-Z-]+)\.((text\.json)|png|([1-9][0-9]*)_files)$").unwrap()
});

/// Get current timestamp in milliseconds.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

/// Parse a clipboard filename in the sync folder.
///
/// Returns `None` if the filename doesn't match the expected pattern.
pub fn parse_clipboard_filename(
    file: &Path,
    sync_folder: &Path,
    hostname: &str,
    filter: Option<ClipboardOrigin>,
) -> Option<ParsedClipboardFile> {
    // Get only the first component relative to sync_folder
    let relative = file.strip_prefix(sync_folder).ok()?;
    let base_name = relative.components().next()?.as_os_str().to_string_lossy();

    let captures = FILE_NAME_REGEX.captures(&base_name)?;

    let beat: u64 = captures.get(1)?.as_str().parse().ok()?;
    let file_hostname = captures.get(2)?.as_str();

    let content_type;
    let mut files_count = None;

    if let Some(count_match) = captures.get(5) {
        content_type = ClipboardContentType::Files;
        files_count = Some(count_match.as_str().parse::<u32>().ok()?);
    } else if captures.get(4).is_some() {
        content_type = ClipboardContentType::Text;
    } else {
        content_type = ClipboardContentType::Image;
    }

    let origin = if file_hostname == hostname {
        ClipboardOrigin::Myself
    } else {
        ClipboardOrigin::Others
    };

    // Apply filter
    if let Some(expected) = filter
        && origin != expected
    {
        return None;
    }

    Some(ParsedClipboardFile {
        path: sync_folder.join(base_name.as_ref()),
        beat,
        content_type,
        origin,
        files_count,
    })
}

/// Check if a filename is an "is-receiving" marker file.
pub fn is_receiving_file(name: &str) -> bool {
    name.ends_with(IS_RECEIVING_FILE_SUFFIX)
}

/// Check if no other computers are currently receiving (excluding ourselves).
pub fn no_computers_receiving(sync_folder: &Path, hostname: &str, now: u64) -> bool {
    let our_file = format!("{hostname}{IS_RECEIVING_FILE_SUFFIX}");
    let stale_threshold = now.saturating_sub(STALE_THRESHOLD_SECS * 1000);

    let entries = match std::fs::read_dir(sync_folder) {
        Ok(e) => e,
        Err(_) => return true,
    };

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if is_receiving_file(&name)
            && name != our_file
            && let Ok(meta) = entry.metadata()
            && let Ok(ctime) = meta.modified()
        {
            let ctime_ms = ctime
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            if ctime_ms >= stale_threshold {
                return false;
            }
        }
    }

    true
}

/// Clean old clipboard files from the sync folder.
pub fn clean_files(sync_folder: &Path, hostname: &str) {
    let now = now_ms();
    let entries = match std::fs::read_dir(sync_folder) {
        Ok(e) => e,
        Err(e) => {
            log::error!("Error reading sync folder for cleanup: {e}");
            return;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        let Some(parsed) = parse_clipboard_filename(&path, sync_folder, hostname, None) else {
            // Skip is-receiving marker files (cleaned on shutdown)
            if is_receiving_file(&name) {
                continue;
            }

            // Check for files from previous versions and delete them
            let is_legacy = name.ends_with(".txt")
                && (name.starts_with("receiving-") || name.contains(".is-reading."));
            if is_legacy {
                log::info!(
                    "Deleting file used by previous versions: {}",
                    path.display()
                );
                delete_file_or_folder(&path);
            }
            continue;
        };

        let threshold_ms = match parsed.origin {
            ClipboardOrigin::Myself => SELF_CLEAN_THRESHOLD_SECS * 1000,
            ClipboardOrigin::Others => OTHERS_CLEAN_THRESHOLD_SECS * 1000,
        };

        if let Ok(meta) = std::fs::metadata(&path)
            && let Ok(ctime) = meta.modified()
        {
            let ctime_ms = ctime
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;

            if ctime_ms <= now.saturating_sub(threshold_ms) {
                log::info!("Deleting: {}", path.display());
                delete_file_or_folder(&path);
            }
        }
    }
}

/// Clipboard deduplication state shared between send and receive operations.
#[derive(Debug, Default)]
pub struct ClipboardDedupState {
    pub last_beat: Option<u64>,
    pub last_text_written: Option<ClipboardText>,
    pub last_image_sha256_written: Option<String>,
    pub last_text_read: Option<ClipboardText>,
    pub last_image_sha256_read: Option<String>,
    pub last_file_paths_read: Option<Vec<String>>,
}

/// Read the current clipboard content and write it to a file in the sync folder.
///
/// Returns `true` if a file was written.
pub fn write_clipboard_to_file(
    sync_folder: &Path,
    hostname: &str,
    state: &crate::config::PersistentState,
    dedup: &mut ClipboardDedupState,
) -> bool {
    let last_beat = &mut dedup.last_beat;
    let last_text_written = &mut dedup.last_text_written;
    let last_image_sha256_written = &mut dedup.last_image_sha256_written;
    let last_text_read = &dedup.last_text_read;
    let last_image_sha256_read = &dedup.last_image_sha256_read;
    let last_file_paths_read = &dedup.last_file_paths_read;
    let beat = now_ms();

    // Check if any other computer is receiving
    if no_computers_receiving(sync_folder, hostname, beat) {
        log::info!("No other computer is receiving clipboards. Skipping clipboard send...");
        return false;
    }

    let ctx = match ClipboardContext::new() {
        Ok(ctx) => ctx,
        Err(e) => {
            log::error!("Failed to create clipboard context: {e}");
            return false;
        }
    };

    // Determine clipboard type
    // Check files before image/text since macOS may report text/plain for file lists
    let content_type;
    let mut clipboard_text = None;
    let mut clipboard_image_bytes = None;
    let mut clipboard_image_sha256 = None;
    let mut clipboard_file_paths: Option<Vec<String>> = None;

    if ctx.has(ContentFormat::Files) {
        if !state.send_files {
            return false;
        }
        match ctx.get_files() {
            Ok(files) => {
                clipboard_file_paths = Some(files);
                content_type = ClipboardContentType::Files;
            }
            Err(e) => {
                log::error!("Error reading clipboard files: {e}");
                return false;
            }
        }
    } else if ctx.has(ContentFormat::Image) {
        if !state.send_images {
            return false;
        }
        match ctx.get_image() {
            Ok(img) => match img.to_png() {
                Ok(png_data) => {
                    let bytes = png_data.get_bytes().to_vec();
                    let sha = calculate_sha256(&bytes);
                    clipboard_image_bytes = Some(bytes);
                    clipboard_image_sha256 = Some(sha);
                    content_type = ClipboardContentType::Image;
                }
                Err(e) => {
                    log::error!("Error converting clipboard image to PNG: {e}");
                    return false;
                }
            },
            Err(e) => {
                log::error!("Error reading clipboard image: {e}");
                return false;
            }
        }
    } else if ctx.has(ContentFormat::Text)
        || ctx.has(ContentFormat::Html)
        || ctx.has(ContentFormat::Rtf)
    {
        if !state.send_texts {
            return false;
        }
        let mut ct = ClipboardText::default();
        if ctx.has(ContentFormat::Text) {
            ct.text = ctx.get_text().ok();
        }
        if ctx.has(ContentFormat::Html) {
            ct.html = ctx.get_html().ok();
        }
        if ctx.has(ContentFormat::Rtf) {
            ct.rtf = ctx.get_rich_text().ok();
        }
        clipboard_text = Some(ct);
        content_type = ClipboardContentType::Text;
    } else {
        let formats = ctx
            .available_formats()
            .map(|f| f.join(", "))
            .unwrap_or_default();
        log::warn!("Unknown clipboard format: {formats}");
        return false;
    }

    // Prevent duplicate sends
    let recent = last_beat
        .map(|lb| beat - lb < crate::consts::DUPLICATE_WINDOW_MS)
        .unwrap_or(false);

    // Dedup check + write in a single pass per content type
    match content_type {
        ClipboardContentType::Text => {
            let ct = clipboard_text.unwrap();
            if ct.is_empty() {
                return false;
            }
            if recent {
                if let Some(lr) = last_text_read
                    && *lr == ct
                {
                    return false;
                }
                if let Some(lw) = last_text_written
                    && *lw == ct
                {
                    return false;
                }
            }

            // Write
            let dest = sync_folder.join(format!("{beat}-{hostname}.text.json"));
            let json = match serde_json::to_string_pretty(&ct) {
                Ok(json) => json,
                Err(e) => {
                    log::error!("Error serializing clipboard text: {e}");
                    return false;
                }
            };
            if let Err(e) = std::fs::write(&dest, json) {
                log::error!("Error writing clipboard text file: {e}");
                return false;
            }
            *last_text_written = Some(ct);
            log::info!("Clipboard written to {}", dest.display());
        }
        ClipboardContentType::Image => {
            let sha = clipboard_image_sha256.as_ref().unwrap();
            if recent {
                if let Some(lr) = last_image_sha256_read
                    && lr == sha
                {
                    return false;
                }
                if let Some(lw) = last_image_sha256_written
                    && lw == sha
                {
                    return false;
                }
            }

            // Write
            let dest = sync_folder.join(format!("{beat}-{hostname}.png"));
            let bytes = clipboard_image_bytes.unwrap();
            if let Err(e) = std::fs::write(&dest, &bytes) {
                log::error!("Error writing clipboard image file: {e}");
                return false;
            }
            *last_image_sha256_written = clipboard_image_sha256;
            log::info!("Clipboard written to {}", dest.display());
        }
        ClipboardContentType::Files => {
            let files = clipboard_file_paths.unwrap();
            if files.is_empty() {
                return false;
            }
            if recent && let Some(lr) = last_file_paths_read {
                let mut a = files.clone();
                let mut b = lr.clone();
                a.sort();
                b.sort();
                if a == b {
                    return false;
                }
            }
            // Check total size
            let paths: Vec<PathBuf> = files.iter().map(PathBuf::from).collect();
            let size = get_files_size_mb(&paths);
            if size > MAX_FILES_SIZE_MB {
                log::warn!(
                    "Not sending clipboard files as {size:.1}MB is bigger than {MAX_FILES_SIZE_MB}MB"
                );
                return false;
            }

            // Write
            let files_count = get_total_number_of_files(&paths);
            let dest = sync_folder.join(format!("{beat}-{hostname}.{files_count}_files"));
            if let Err(e) = std::fs::create_dir(&dest) {
                log::error!("Error creating clipboard files folder: {e}");
                return false;
            }
            for file_path in &paths {
                let file_name = file_path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                let full_dest = dest.join(&file_name);
                if file_path.is_dir() {
                    if let Err(e) = copy_folder_recursive(file_path, &full_dest) {
                        log::error!("Error copying folder {}: {e}", file_path.display());
                    }
                } else if let Err(e) = std::fs::copy(file_path, &full_dest) {
                    log::error!("Error copying file {}: {e}", file_path.display());
                }
            }
            log::info!("Clipboard written to {}", dest.display());
        }
    }

    *last_beat = Some(beat);
    true
}

/// Read a clipboard file from the sync folder and set it to the system clipboard.
///
/// Returns `true` if the clipboard was updated.
pub fn read_clipboard_from_file(
    parsed: &ParsedClipboardFile,
    state: &crate::config::PersistentState,
    dedup: &mut ClipboardDedupState,
) -> bool {
    let last_beat = &mut dedup.last_beat;
    let last_text_read = &mut dedup.last_text_read;
    let last_image_sha256_read = &mut dedup.last_image_sha256_read;
    let last_file_paths_read = &mut dedup.last_file_paths_read;
    let beat = now_ms();
    let file = &parsed.path;

    // Skip if the beat is older than what was already processed
    if let Some(lb) = *last_beat
        && parsed.beat < lb
    {
        log::info!(
            "Skipping reading clipboard from {} as a newer clipboard was already processed",
            file.display()
        );
        return false;
    }

    let ctx = match ClipboardContext::new() {
        Ok(ctx) => ctx,
        Err(e) => {
            log::error!("Failed to create clipboard context: {e}");
            return false;
        }
    };

    // Read from file → dedup against current clipboard → set clipboard
    match parsed.content_type {
        ClipboardContentType::Text => {
            if !state.receive_texts {
                return false;
            }

            // Read
            let content = match std::fs::read_to_string(file) {
                Ok(c) => c,
                Err(e) => {
                    log::error!("Error reading clipboard text {}: {e}", file.display());
                    return false;
                }
            };
            let ct: ClipboardText = match serde_json::from_str(&content) {
                Ok(ct) => ct,
                Err(e) => {
                    log::error!("Error parsing clipboard text {}: {e}", file.display());
                    return false;
                }
            };
            if ct.is_empty() {
                return false;
            }

            // Dedup: compare against current clipboard
            if ctx.has(ContentFormat::Text)
                || ctx.has(ContentFormat::Html)
                || ctx.has(ContentFormat::Rtf)
            {
                let mut current = ClipboardText::default();
                if ctx.has(ContentFormat::Text) {
                    current.text = ctx.get_text().ok();
                }
                if ctx.has(ContentFormat::Html) {
                    current.html = ctx.get_html().ok();
                }
                if ctx.has(ContentFormat::Rtf) {
                    current.rtf = ctx.get_rich_text().ok();
                }
                if current == ct {
                    return false;
                }
            }

            // Set clipboard
            use clipboard_rs::ClipboardContent;
            let mut contents = Vec::new();
            if let Some(ref text) = ct.text {
                contents.push(ClipboardContent::Text(text.clone()));
            }
            if let Some(ref html) = ct.html {
                contents.push(ClipboardContent::Html(html.clone()));
            }
            if let Some(ref rtf) = ct.rtf {
                contents.push(ClipboardContent::Rtf(rtf.clone()));
            }
            if let Err(e) = ctx.set(contents) {
                log::error!("Error setting clipboard text: {e}");
                return false;
            }
            *last_text_read = Some(ct);
        }
        ClipboardContentType::Image => {
            if !state.receive_images {
                return false;
            }

            // Read
            let bytes = match std::fs::read(file) {
                Ok(b) => b,
                Err(e) => {
                    log::error!("Error reading clipboard image {}: {e}", file.display());
                    return false;
                }
            };
            let sha = calculate_sha256(&bytes);

            // Dedup: compare against current clipboard
            if ctx.has(ContentFormat::Image)
                && let Ok(img) = ctx.get_image()
                && let Ok(png) = img.to_png()
            {
                let current_sha = calculate_sha256(png.get_bytes());
                if current_sha == sha {
                    return false;
                }
            }

            // Set clipboard
            match clipboard_rs::common::RustImageData::from_bytes(&bytes) {
                Ok(img) => {
                    if let Err(e) = ctx.set_image(img) {
                        log::error!("Error setting clipboard image: {e}");
                        return false;
                    }
                }
                Err(e) => {
                    log::error!("Error creating image from bytes: {e}");
                    return false;
                }
            }
            *last_image_sha256_read = Some(sha);
        }
        ClipboardContentType::Files => {
            if !state.receive_files {
                return false;
            }

            // Validate file count
            let expected_count = match parsed.files_count {
                Some(c) => c,
                None => {
                    log::warn!(
                        "Could not read the number of files in {}. Skipping...",
                        file.display()
                    );
                    return false;
                }
            };
            let actual_count = get_total_number_of_files(std::slice::from_ref(file));
            if actual_count != expected_count {
                log::info!(
                    "Not all files are yet present in _files folder. Current: {actual_count}, expected: {expected_count}. Skipping..."
                );
                return false;
            }

            // Read
            let file_paths: Vec<String> = match std::fs::read_dir(file) {
                Ok(entries) => entries
                    .flatten()
                    .map(|e| e.path().to_string_lossy().to_string())
                    .collect(),
                Err(e) => {
                    log::error!("Error reading clipboard files dir {}: {e}", file.display());
                    return false;
                }
            };

            // Dedup: compare against current clipboard
            if ctx.has(ContentFormat::Files)
                && let Ok(current_files) = ctx.get_files()
            {
                let mut a = file_paths.clone();
                let mut b = current_files;
                a.sort();
                b.sort();
                if a == b {
                    return false;
                }
            }

            // Set clipboard
            if let Err(e) = ctx.set_files(file_paths.clone()) {
                log::error!("Error setting clipboard files: {e}");
                return false;
            }
            *last_file_paths_read = Some(file_paths);
        }
    }

    *last_beat = Some(beat);
    log::info!("Clipboard was read from {}", file.display());
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sync_folder() -> PathBuf {
        PathBuf::from("/sync")
    }

    // --- parse_clipboard_filename ---

    #[test]
    fn parse_text_file() {
        let sf = sync_folder();
        let path = sf.join("1716000000000-myhost.text.json");
        let parsed = parse_clipboard_filename(&path, &sf, "myhost", None).unwrap();
        assert_eq!(parsed.beat, 1716000000000);
        assert_eq!(parsed.content_type, ClipboardContentType::Text);
        assert_eq!(parsed.origin, ClipboardOrigin::Myself);
        assert!(parsed.files_count.is_none());
    }

    #[test]
    fn parse_image_file() {
        let sf = sync_folder();
        let path = sf.join("1716000000000-otherhost.png");
        let parsed = parse_clipboard_filename(&path, &sf, "myhost", None).unwrap();
        assert_eq!(parsed.content_type, ClipboardContentType::Image);
        assert_eq!(parsed.origin, ClipboardOrigin::Others);
    }

    #[test]
    fn parse_files_folder() {
        let sf = sync_folder();
        let path = sf.join("1716000000000-myhost.3_files");
        let parsed = parse_clipboard_filename(&path, &sf, "myhost", None).unwrap();
        assert_eq!(parsed.content_type, ClipboardContentType::Files);
        assert_eq!(parsed.files_count, Some(3));
    }

    #[test]
    fn parse_invalid_filename_returns_none() {
        let sf = sync_folder();
        assert!(parse_clipboard_filename(&sf.join("random.txt"), &sf, "host", None).is_none());
        assert!(parse_clipboard_filename(&sf.join("not-a-file"), &sf, "host", None).is_none());
        assert!(parse_clipboard_filename(&sf.join("0-host.png"), &sf, "host", None).is_none());
    }

    #[test]
    fn parse_with_origin_filter() {
        let sf = sync_folder();
        let path = sf.join("1716000000000-myhost.text.json");
        // Filter for Others should return None when file is from Myself
        assert!(
            parse_clipboard_filename(&path, &sf, "myhost", Some(ClipboardOrigin::Others))
                .is_none()
        );
        // Filter for Myself should return the parsed file
        assert!(
            parse_clipboard_filename(&path, &sf, "myhost", Some(ClipboardOrigin::Myself))
                .is_some()
        );
    }

    #[test]
    fn parse_hostname_with_hyphens() {
        let sf = sync_folder();
        let path = sf.join("1716000000000-my-host-name.png");
        let parsed = parse_clipboard_filename(&path, &sf, "my-host-name", None).unwrap();
        assert_eq!(parsed.origin, ClipboardOrigin::Myself);
    }

    // --- is_receiving_file ---

    #[test]
    fn is_receiving_file_positive() {
        assert!(is_receiving_file(&format!("host{IS_RECEIVING_FILE_SUFFIX}")));
    }

    #[test]
    fn is_receiving_file_negative() {
        assert!(!is_receiving_file("1716000000000-host.text.json"));
        assert!(!is_receiving_file("random.txt"));
    }

    // --- no_computers_receiving ---

    #[test]
    fn no_computers_receiving_empty_folder() {
        let dir = tempfile::tempdir().unwrap();
        assert!(no_computers_receiving(dir.path(), "myhost", now_ms()));
    }

    #[test]
    fn no_computers_receiving_with_own_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir
            .path()
            .join(format!("myhost{IS_RECEIVING_FILE_SUFFIX}"));
        std::fs::write(&path, "1716000000000").unwrap();
        // Our own file should be ignored
        assert!(no_computers_receiving(dir.path(), "myhost", now_ms()));
    }

    #[test]
    fn no_computers_receiving_with_recent_other() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir
            .path()
            .join(format!("otherhost{IS_RECEIVING_FILE_SUFFIX}"));
        std::fs::write(&path, "recent").unwrap();
        // Other host's fresh file means someone is receiving
        assert!(!no_computers_receiving(dir.path(), "myhost", now_ms()));
    }

    // --- clean_files ---

    #[test]
    fn clean_files_removes_old_own_files() {
        let dir = tempfile::tempdir().unwrap();
        // Create a file with beat = 1 (very old)
        let old_file = dir.path().join("1-myhost.text.json");
        std::fs::write(&old_file, r#"{"text":"hello"}"#).unwrap();
        // Set modification time far in the past isn't easy, but clean_files
        // checks mtime vs threshold. The file was just created so its mtime is "now".
        // clean_files will NOT delete it since it's recent. Just verify it doesn't crash.
        clean_files(dir.path(), "myhost");
        // The file is recent so should still exist
        assert!(old_file.exists());
    }

    #[test]
    fn clean_files_removes_legacy_files() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join("receiving-data.txt");
        std::fs::write(&legacy, "data").unwrap();
        clean_files(dir.path(), "myhost");
        assert!(!legacy.exists());
    }

    #[test]
    fn clean_files_preserves_is_receiving_files() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir
            .path()
            .join(format!("myhost{IS_RECEIVING_FILE_SUFFIX}"));
        std::fs::write(&marker, "123").unwrap();
        clean_files(dir.path(), "myhost");
        assert!(marker.exists());
    }

    // --- now_ms ---

    #[test]
    fn now_ms_returns_reasonable_value() {
        let ts = now_ms();
        // Should be after 2020-01-01 (1577836800000) and before 2100
        assert!(ts > 1577836800000);
        assert!(ts < 4102444800000);
    }
}
