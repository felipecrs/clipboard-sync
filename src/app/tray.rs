use crate::types::TrayIconState;

pub fn get_tray_icon(state: TrayIconState) -> tray_icon::Icon {
    #[cfg(target_os = "windows")]
    {
        let resource_name = match state {
            TrayIconState::Working => "working-icon",
            TrayIconState::Sent => "sent-icon",
            TrayIconState::Received => "received-icon",
            TrayIconState::Suspended => "suspended-icon",
        };
        tray_icon::Icon::from_resource_name(resource_name, None).unwrap()
    }
    #[cfg(not(target_os = "windows"))]
    {
        let bytes = match state {
            TrayIconState::Working => crate::consts::WORKING_TRAY_ICON_BYTES,
            TrayIconState::Sent => crate::consts::SENT_TRAY_ICON_BYTES,
            TrayIconState::Received => crate::consts::RECEIVED_TRAY_ICON_BYTES,
            TrayIconState::Suspended => crate::consts::SUSPENDED_TRAY_ICON_BYTES,
        };
        // Decode PNG to RGBA
        let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        let mut reader = decoder.read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buf).unwrap();
        buf.truncate(info.buffer_size());
        tray_icon::Icon::from_rgba(buf, info.width, info.height).unwrap()
    }
}