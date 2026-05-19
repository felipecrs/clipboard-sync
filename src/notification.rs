use crate::platform::{NotificationDuration, send_notification};

pub fn log_and_notify_error(title: &str, message: &str) {
    log::error!("{message}");
    if let Err(e) = send_notification(title, message, NotificationDuration::Long) {
        log::error!("Failed to send error notification: {e:#}");
    }
}