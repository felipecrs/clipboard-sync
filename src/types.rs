use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// The type of clipboard content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClipboardContentType {
    Text,
    Image,
    Files,
}

/// Text clipboard content, may contain plain text, HTML, and RTF.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClipboardText {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub html: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rtf: Option<String>,
}

impl ClipboardText {
    pub fn is_empty(&self) -> bool {
        self.text.as_ref().is_none_or(|t| t.is_empty())
            && self.html.as_ref().is_none_or(|h| h.is_empty())
            && self.rtf.as_ref().is_none_or(|r| r.is_empty())
    }

    pub fn equals(&self, other: &ClipboardText) -> bool {
        self.text == other.text && self.html == other.html && self.rtf == other.rtf
    }
}

/// A parsed clipboard file from the sync folder.
#[derive(Debug, Clone)]
pub struct ParsedClipboardFile {
    /// Full path to the file/folder in the sync directory.
    pub path: PathBuf,
    /// The timestamp (beat) extracted from filename.
    pub beat: u64,
    /// The type of clipboard data.
    pub content_type: ClipboardContentType,
    /// Whether this file was created by ourselves or others.
    pub origin: ClipboardOrigin,
    /// Number of files (only for Files type).
    pub files_count: Option<u32>,
}

/// Whether a clipboard file originated from this host or another.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardOrigin {
    Myself,
    Others,
}

/// Which tray icon to display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayIconState {
    Working,
    Sent,
    Received,
    Suspended,
}

/// Events sent to the main event loop.
#[derive(Debug)]
pub enum UserEvent {
    TrayIcon(tray_icon::TrayIconEvent),
    Menu(tray_icon::menu::MenuEvent),
    /// Clipboard changed on the local machine.
    ClipboardChanged,
    /// A new clipboard file was detected in the sync folder.
    ClipboardFileDetected(PathBuf),
    /// Request a config reload / reinitialize.
    Reload,
}
