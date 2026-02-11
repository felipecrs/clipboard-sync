fn main() {
    #[cfg(target_os = "windows")]
    {
        let mut res = winresource::WindowsResource::new();
        // The first icon gets set as the executable icon
        res.set_icon_with_id("resources/appicons/ico/icon.ico", "app-icon");
        res.set_icon_with_id("resources/trayicons/ico/working.ico", "working-icon");
        res.set_icon_with_id("resources/trayicons/ico/sent.ico", "sent-icon");
        res.set_icon_with_id("resources/trayicons/ico/received.ico", "received-icon");
        res.set_icon_with_id("resources/trayicons/ico/suspended.ico", "suspended-icon");
        res.set_language(0x0009); // English
        res.compile().unwrap();
    }
}
