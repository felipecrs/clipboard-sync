<p align="center">
⭐<b>Please star this project in GitHub if it helps you!</b>⭐
</p>

# Clipboard Sync

<img align="right" width="100" height="100" src="./resources/appicons/png/icon.png">

A simple tool which helps to sync the clipboard between computers by using a shared folder.

In other words, if you have a shared folder between your computers (including OneDrive and other folder synchronization tools), this tool helps you sync your clipboard by leveraging it.

Currently supports the following formats in the clipboard:

- `text` (including hyperlinks and rich text)
- `image`
- `files` (max of 100MB)

## Get Started

Download the [latest release](https://github.com/felipecrs/clipboard-sync/releases/latest) for your platform and open it.

Alternatively, you can install it with [`winget`](https://github.com/microsoft/winget-cli#readme):

```console
winget install clipboard-sync
```

When running for the first time, the tool will ask you which folder to use for synchronizing the clipboard. Select the same shared folder between your computers in both of them.

## See it in action

https://user-images.githubusercontent.com/29582865/138568560-011bb822-fb8a-4c18-930e-fc310e472a53.mp4

## How it works

It could not be simpler:

When a new text is detected in your clipboard, the tool will create a file in the folder which you selected with the clipboard contents.

When a new file is detected in the same folder, the tool will read its contents and write it to the clipboard.

Some safeguards are implemented to prevent infinite loops and unneeded operations.

Also, it deletes the files created when they become 5 minutes old.

## Tips

### Configuring the folder on OneDrive

Make sure the _Always keep on this device_ option is enabled for the folder on both computers:

![Always keep on this device OneDrive example](https://user-images.githubusercontent.com/29582865/138023653-c284670c-0019-42f9-9018-e98e138bf18f.png)

### OneDrive for Linux?

If you are using Linux, you can use the non-official [OneDrive client for Linux](https://github.com/abraunegg/onedrive).

### Auto-start on boot?

Yes!

![Auto-start on boot example](https://user-images.githubusercontent.com/29582865/138464616-0cc2d14f-08f8-42f5-840c-8c217081be13.png)

### Slow to sync

The Clipboard Sync should be as fast (and as slow) as your folder synchronization tool. OneDrive takes some seconds to do its job, and in order to help you handle it, you can watch the Clipboard System tray icon:

![Sending and receiving icon](https://user-images.githubusercontent.com/29582865/138508741-2b5fe84b-ab3d-446b-97fa-4c25907479d0.gif)

## Development

If you want to build this project locally, you will need:

1. [Volta](https://github.com/volta-cli/volta) for handling the correct version of Node.js and NPM (or see the correct version of Node.js and NPM in the `volta` key of [`package.json`](./package.json) and install them by yourself)
2. `npm install` to install the dependencies
3. `npm start` to build and run the project

## Credits

The [original icon](https://www.flaticon.com/free-icon/clipboard_2542070) was made by [Freepik](https://www.flaticon.com/authors/freepik).
