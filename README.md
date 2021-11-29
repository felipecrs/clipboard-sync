<img align="left" width="100" height="100" src="./assets/appicons/png/icon.png">

# Clipboard Sync

A simple tool which helps to sync the clipboard between computers by using a shared folder.

In other words, if you have a shared folder between your computers (including OneDrive and other folder synchronization tools), this tool helps you sync your clipboard by leveraging it.

Currently supports the following formats in the clipboard:

- `text`
- `image`
- `files`

## Get Started

Download the [latest release](https://github.com/felipecrs/clipboard-sync/releases/latest) for your platform and open it.

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
2. If you are on Windows, you will need some additional tools because of [@parcel/watcher](https://github.com/parcel-bundler/watcher):

   1. Python 3:

      ```console
      winget install Python.Python.3 --source winget
      ```

   2. Visual Studio Build Tools with C++ development component:

      ```console
      winget install Microsoft.VisualStudio.2022.BuildTools --source winget
      ```

      And then enable the _Desktop development with C++_ component as following:

      ![Visual Studio Installer C++ tip](https://user-images.githubusercontent.com/29582865/143795034-62c5da8a-353d-4402-862f-4e480fc6fa69.gif)

   3. `npm install` to install the dependencies and `npm start` to build and run the project.

## References

This project is a continuation of <https://github.com/felipecrs/clipboard-sync-preview>.

Icons made by [Freepik](https://www.flaticon.com/authors/freepik) and [Flat Icons](https://www.flaticon.com/authors/flat-icons) from [www.flaticon.com](https://www.flaticon.com/).
