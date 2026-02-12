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
    Regex::new(r"^([1-9][0-9]*)-([0-9a-zA-Z-]+)\.((text\.json)|png|([1-9][0-9]*)_files)$")
        .unwrap()
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
    let base_name = relative
        .components()
        .next()?
        .as_os_str()
        .to_string_lossy();

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
    if let Some(expected) = filter {
        if origin != expected {
            return None;
        }
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
        if is_receiving_file(&name) && name != our_file {
            if let Ok(meta) = entry.metadata() {
                if let Ok(ctime) = meta.modified() {
                    let ctime_ms = ctime
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64;
                    if ctime_ms >= stale_threshold {
                        return false;
                    }
                }
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

        let parsed = parse_clipboard_filename(&path, sync_folder, hostname, None);

        if parsed.is_none() {
            // Skip is-receiving marker files (cleaned on shutdown)
            if is_receiving_file(&name) {
                continue;
            }

            // Check for files from previous versions and delete them
            let is_legacy = name.ends_with(".txt")
                && (name.starts_with("receiving-") || name.contains(".is-reading."));
            if is_legacy {
                log::info!("Deleting file used by previous versions: {}", path.display());
                delete_file_or_folder(&path);
            }
            continue;
        }

        let parsed = parsed.unwrap();

        let threshold_ms = match parsed.origin {
            ClipboardOrigin::Myself => SELF_CLEAN_THRESHOLD_SECS * 1000,
            ClipboardOrigin::Others => OTHERS_CLEAN_THRESHOLD_SECS * 1000,
        };

        if let Ok(meta) = std::fs::metadata(&path) {
            if let Ok(ctime) = meta.modified() {
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
}

/// Read the current clipboard content and write it to a file in the sync folder.
///
/// Returns `true` if a file was written.
pub fn write_clipboard_to_file(
    sync_folder: &Path,
    hostname: &str,
    config: &crate::config::Config,
    last_beat: &mut Option<u64>,
    last_text_written: &mut Option<ClipboardText>,
    last_image_sha256_written: &mut Option<String>,
    last_text_read: &Option<ClipboardText>,
    last_image_sha256_read: &Option<String>,
    last_file_paths_read: &Option<Vec<String>>,
) -> bool {
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
        if !config.send_files {
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
        if !config.send_images {
            return false;
        }
        match ctx.get_image() {
            Ok(img) => {
                match img.to_png() {
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
                }
            }
            Err(e) => {
                log::error!("Error reading clipboard image: {e}");
                return false;
            }
        }
    } else if ctx.has(ContentFormat::Text)
        || ctx.has(ContentFormat::Html)
        || ctx.has(ContentFormat::Rtf)
    {
        if !config.send_texts {
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

    match content_type {
        ClipboardContentType::Text => {
            let ct = clipboard_text.as_ref().unwrap();
            if ct.is_empty() {
                return false;
            }
            if recent {
                if let Some(lr) = last_text_read {
                    if lr.equals(ct) {
                        return false;
                    }
                }
                if let Some(lw) = last_text_written {
                    if lw.equals(ct) {
                        return false;
                    }
                }
            }
        }
        ClipboardContentType::Image => {
            let sha = clipboard_image_sha256.as_ref().unwrap();
            if recent {
                if let Some(lr) = last_image_sha256_read {
                    if lr == sha {
                        return false;
                    }
                }
                if let Some(lw) = last_image_sha256_written {
                    if lw == sha {
                        return false;
                    }
                }
            }
        }
        ClipboardContentType::Files => {
            let files = clipboard_file_paths.as_ref().unwrap();
            if files.is_empty() {
                return false;
            }
            if recent {
                if let Some(lr) = last_file_paths_read {
                    let mut a = files.clone();
                    let mut b = lr.clone();
                    a.sort();
                    b.sort();
                    if a == b {
                        return false;
                    }
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
        }
    }

    *last_beat = Some(beat);

    // Write the clipboard to disk
    match content_type {
        ClipboardContentType::Text => {
            let dest = sync_folder.join(format!("{beat}-{hostname}.text.json"));
            let ct = clipboard_text.unwrap();
            match serde_json::to_string_pretty(&ct) {
                Ok(json) => {
                    if let Err(e) = std::fs::write(&dest, json) {
                        log::error!("Error writing clipboard text file: {e}");
                        return false;
                    }
                    *last_text_written = Some(ct);
                }
                Err(e) => {
                    log::error!("Error serializing clipboard text: {e}");
                    return false;
                }
            }
            log::info!("Clipboard written to {}", dest.display());
        }
        ClipboardContentType::Image => {
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
            let paths: Vec<PathBuf> = files.iter().map(PathBuf::from).collect();
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

    true
}

/// Read a clipboard file from the sync folder and set it to the system clipboard.
///
/// Returns `true` if the clipboard was updated.
pub fn read_clipboard_from_file(
    parsed: &ParsedClipboardFile,
    config: &crate::config::Config,
    last_beat: &mut Option<u64>,
    last_text_read: &mut Option<ClipboardText>,
    last_image_sha256_read: &mut Option<String>,
    last_file_paths_read: &mut Option<Vec<String>>,
) -> bool {
    let beat = now_ms();
    let file = &parsed.path;

    // Read the new content from file
    let mut new_text: Option<ClipboardText> = None;
    let mut new_image_bytes: Option<Vec<u8>> = None;
    let mut new_image_sha256: Option<String> = None;
    let mut new_file_paths: Option<Vec<String>> = None;

    match parsed.content_type {
        ClipboardContentType::Text => {
            if !config.receive_texts {
                return false;
            }
            match std::fs::read_to_string(file) {
                Ok(content) => match serde_json::from_str::<ClipboardText>(&content) {
                    Ok(ct) => new_text = Some(ct),
                    Err(e) => {
                        log::error!("Error parsing clipboard text {}: {e}", file.display());
                        return false;
                    }
                },
                Err(e) => {
                    log::error!("Error reading clipboard text {}: {e}", file.display());
                    return false;
                }
            }
        }
        ClipboardContentType::Image => {
            if !config.receive_images {
                return false;
            }
            match std::fs::read(file) {
                Ok(bytes) => {
                    let sha = calculate_sha256(&bytes);
                    new_image_bytes = Some(bytes);
                    new_image_sha256 = Some(sha);
                }
                Err(e) => {
                    log::error!("Error reading clipboard image {}: {e}", file.display());
                    return false;
                }
            }
        }
        ClipboardContentType::Files => {
            if !config.receive_files {
                return false;
            }
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

            let actual_count = get_total_number_of_files(&[file.clone()]);
            if actual_count != expected_count {
                log::info!(
                    "Not all files are yet present in _files folder. Current: {actual_count}, expected: {expected_count}. Skipping..."
                );
                return false;
            }

            match std::fs::read_dir(file) {
                Ok(entries) => {
                    let paths: Vec<String> = entries
                        .flatten()
                        .map(|e| e.path().to_string_lossy().to_string())
                        .collect();
                    new_file_paths = Some(paths);
                }
                Err(e) => {
                    log::error!("Error reading clipboard files dir {}: {e}", file.display());
                    return false;
                }
            }
        }
    }

    // Read current clipboard for duplicate detection
    let ctx = match ClipboardContext::new() {
        Ok(ctx) => ctx,
        Err(e) => {
            log::error!("Failed to create clipboard context: {e}");
            return false;
        }
    };

    // Duplicate detection: compare against current clipboard
    match parsed.content_type {
        ClipboardContentType::Text => {
            let nt = new_text.as_ref().unwrap();
            if nt.is_empty() {
                return false;
            }
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
                if current.equals(nt) {
                    return false;
                }
            }
        }
        ClipboardContentType::Image => {
            let sha = new_image_sha256.as_ref().unwrap();
            if ctx.has(ContentFormat::Image) {
                if let Ok(img) = ctx.get_image() {
                    if let Ok(png) = img.to_png() {
                        let current_sha = calculate_sha256(png.get_bytes());
                        if current_sha == *sha {
                            return false;
                        }
                    }
                }
            }
        }
        ClipboardContentType::Files => {
            let nf = new_file_paths.as_ref().unwrap();
            if ctx.has(ContentFormat::Files) {
                if let Ok(current_files) = ctx.get_files() {
                    let mut a = nf.clone();
                    let mut b = current_files;
                    a.sort();
                    b.sort();
                    if a == b {
                        return false;
                    }
                }
            }
        }
    }

    // Skip if the beat is older than what was already processed
    if let Some(lb) = *last_beat {
        if parsed.beat < lb {
            log::info!(
                "Skipping reading clipboard from {} as a newer clipboard was already processed",
                file.display()
            );
            return false;
        }
    }

    *last_beat = Some(beat);

    // Set clipboard
    match parsed.content_type {
        ClipboardContentType::Text => {
            let ct = new_text.unwrap();
            // Set each format that's available
            // clipboard-rs's set() clears and sets, but we need to set multiple formats.
            // Use the set() method with ClipboardContent variants.
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
            let bytes = new_image_bytes.unwrap();
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
            *last_image_sha256_read = new_image_sha256;
        }
        ClipboardContentType::Files => {
            let file_paths = new_file_paths.unwrap();
            if let Err(e) = ctx.set_files(file_paths.clone()) {
                log::error!("Error setting clipboard files: {e}");
                return false;
            }
            *last_file_paths_read = Some(file_paths);
        }
    }

    log::info!("Clipboard was read from {}", file.display());
    true
}
