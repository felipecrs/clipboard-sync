<img align="right" width="128" alt="Clipboard Sync" src="./resources/appicons/png/icon.png" />

<p align="center">
⭐<b>Please star this project in GitHub if it helps you!</b>⭐
</p>

# Clipboard Sync

Clipboard Sync is a tray app that synchronizes your clipboard between computers by using a shared folder such as OneDrive, Syncthing, Dropbox, a network share, or any other file synchronization tool.

It is a lightweight Rust application focused on practical clipboard syncing with minimal setup. It currently supports:

- `text`, including hyperlinks and rich text
- `image`
- `files`, up to 100 MB total

## Demo

https://user-images.githubusercontent.com/29582865/138568560-011bb822-fb8a-4c18-930e-fc310e472a53.mp4

## Getting Started

Simply grab the executable from the [releases page](https://github.com/felipecrs/clipboard-sync/releases), place it somewhere like `C:\Apps\Clipboard Sync\ClipboardSync.exe` and run it.

Or you can copy and paste this into _Windows PowerShell_, and execute:

```powershell
New-Item -ItemType Directory -Path 'C:\Apps\Clipboard Sync' -Force >$null; `
  Get-Process | Where-Object { $_.Path -eq 'C:\Apps\Clipboard Sync\ClipboardSync.exe' } | Stop-Process; `
  curl.exe --progress-bar --location --output 'C:\Apps\Clipboard Sync\ClipboardSync.exe' `
  'https://github.com/felipecrs/clipboard-sync/releases/latest/download/ClipboardSync.exe'; `
  Start-Process 'C:\Apps\Clipboard Sync\ClipboardSync.exe'
```

You can also use the snippet above to update the app, just run it again.

When the app starts for the first time, select the folder that will be used for synchronization. Use the same shared folder on every computer where you want the clipboard to stay in sync.

## Usage

Click the Clipboard Sync tray icon to access the menu. The menu is organized into the following sections:

1. **Clipboard**: Choose which clipboard formats are sent and received.
2. **Sync**: Change the shared folder and optionally configure a sync command to run before synchronization starts.
3. **Preferences**: Choose the folder watch mode, auto-clean behavior, update checks, and auto-launch.
4. **Troubleshooting**: Reinitialize the app, open relevant folders, restart OneDrive on Windows, and access the project page.

### Sync Folder

Use a folder that is reliably synchronized across your machines. Clipboard Sync watches that folder for changes and writes clipboard payloads into it.

If you use OneDrive, make sure the sync folder is configured as **Always keep on this device** on every computer:

![Always keep on this device OneDrive example](https://user-images.githubusercontent.com/29582865/138023653-c284670c-0019-42f9-9018-e98e138bf18f.png)

### Watch Modes

Clipboard Sync supports two watch modes:

1. **Native**: Uses the operating system file watching APIs and should be preferred when it works reliably with your sync provider.
2. **Polling**: Checks the folder repeatedly and can be more reliable with some synchronization tools.

### Slow to Sync

Clipboard Sync is only as fast as the underlying folder synchronization tool. If syncing feels slow, check the tray icon state to see whether the app is currently sending, receiving, or waiting:

![Sending and receiving icon](https://user-images.githubusercontent.com/29582865/138508741-2b5fe84b-ab3d-446b-97fa-4c25907479d0.gif)

## How It Works

When the local clipboard changes, Clipboard Sync writes the clipboard contents to the shared folder.

When a new clipboard payload appears in that folder from another machine, Clipboard Sync reads it and writes it into the local clipboard.

To avoid loops and stale state, the app keeps track of recently processed clipboard data, maintains keep-alive files for active receivers, and periodically cleans up old sync artifacts.

## Credits

- The [original clipboard icon](https://www.flaticon.com/free-icon/clipboard_2542070) was made by [Freepik](https://www.flaticon.com/authors/freepik).
- [tauri-apps/tray-icon](https://github.com/tauri-apps/tray-icon) powers the tray integration.
- [GitHub Copilot](https://github.com/copilot/) helped a lot with the Rust rewriting.
