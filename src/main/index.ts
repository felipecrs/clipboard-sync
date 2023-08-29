import {
  app,
  clipboard,
  dialog,
  Menu,
  nativeImage,
  Notification,
  shell,
  Tray,
} from "electron";

import Store from "electron-store";
import watcher from "@parcel/watcher";
import * as cron from "node-cron";
import * as fs from "node:fs";
import * as path from "node:path";
import * as semver from "semver";

import {
  cleanFiles,
  parseClipboardFileName,
  getNextWriteTime,
  isThereMoreThanOneClipboardFile,
  isIsReceivingFile,
  ClipboardType,
  ClipboardText,
  isClipboardTextEquals,
  isClipboardTextEmpty,
  ClipboardFile,
  ClipboardFiles,
  ClipboardImage,
} from "./clipboard";
import { hostName, hostNameIsReceivingFileName } from "./global";
import {
  calculateSha256,
  copyFolderRecursive,
  getFilesSizeInMb,
  getRedirectedUrl,
  getTotalNumberOfFiles,
} from "./utils";

// Handle creating/removing shortcuts on Windows when installing/uninstalling.
if (require("electron-squirrel-startup")) {
  app.exit();
}

const gotTheLock = app.requestSingleInstanceLock();

if (!gotTheLock) {
  process.exit();
}

type ConfigType = {
  folder?: string;
  sendTexts: boolean;
  sendImages: boolean;
  sendFiles: boolean;
  receiveTexts: boolean;
  receiveImages: boolean;
  receiveFiles: boolean;
  autoCleanup: boolean;
};

type ClipboardListener = {
  startListening: () => void;
  on: (arg0: string, arg1: () => void) => void;
  stopListening: () => void;
};

type ClipboardIcon = "clipboard" | "clipboard_sent" | "clipboard_received";

const config = new Store<ConfigType>({
  defaults: {
    sendTexts: true,
    sendImages: true,
    sendFiles: true,
    receiveTexts: true,
    receiveImages: true,
    receiveFiles: true,
    autoCleanup: true,
  },
});

let appIcon: Tray | null = null;
let firstTime = true;

let syncFolder: string | null = null;

let lastTimeWritten: number | null = null;

let lastTextRead: ClipboardText | null = null;
let lastImageSha256Read: string | null = null;
let lastClipboardFilePathsRead: string[] | null = null;
let lastTimeRead: number | null = null;

let clipboardListener: ClipboardListener | null = null;
let clipboardFilesWatcher: watcher.AsyncSubscription | null = null;
let filesCleanerTask: cron.ScheduledTask | null = null;
let iconWaiter: NodeJS.Timeout | null = null;

let lastTimeChecked: number | null = null;

const writeClipboardToFile = () => {
  if (!syncFolder) {
    // This should never happen
    console.error("Sync folder is not defined. Skipping clipboard send...");
    return;
  }

  // Avoids sending the clipboard if there is no other computer receiving
  if (
    fs
      .readdirSync(syncFolder)
      .filter(
        (file) =>
          isIsReceivingFile(file) && file !== hostNameIsReceivingFileName
      ).length === 0
  ) {
    console.error(
      "No other computer is receiving clipboards. Skipping clipboard send..."
    );
    return;
  }

  // Prevents duplicated clipboard events
  const currentTime = Date.now();
  if (lastTimeChecked && currentTime - lastTimeChecked < 1000) {
    return;
  }
  lastTimeChecked = currentTime;

  let clipboardType: ClipboardType;
  let clipboardText: ClipboardText | undefined;
  let clipboardImage: Buffer | undefined;
  let clipboardImageSha256: string | undefined;
  let clipboardFilePaths: string[] | undefined;
  let clipboardFilesCount: number;

  const clipboardFormats = clipboard.availableFormats();

  try {
    if (
      clipboardFormats.includes("text/plain") ||
      clipboardFormats.includes("text/html") ||
      clipboardFormats.includes("text/rtf")
    ) {
      if (!config.get("sendTexts", true)) {
        return;
      }
      clipboardType = "text";
      clipboardText = {
        text: clipboardFormats.includes("text/plain")
          ? clipboard.readText()
          : undefined,
        html: clipboardFormats.includes("text/html")
          ? clipboard.readHTML()
          : undefined,
        rtf: clipboardFormats.includes("text/rtf")
          ? clipboard.readRTF()
          : undefined,
      };
    } else if (clipboardFormats.includes("image/png")) {
      if (!config.get("sendImages", true)) {
        return;
      }
      clipboardType = "image";
      clipboardImage = clipboard.readImage().toPNG();
      clipboardImageSha256 = calculateSha256(clipboardImage);
    } else if (clipboardFormats.includes("text/uri-list")) {
      if (!config.get("sendFiles", true)) {
        return;
      }
      clipboardType = "files";
      clipboardFilePaths = clipboardEx.readFilePaths();
    } else {
      console.error(
        "Clipboard format was not recognized. Skipping clipboard send..."
      );
      return;
    }
  } catch (error) {
    console.error("Cannot read current clipboard, skipping clipboard send...");
    return;
  }

  // Prevent sending the clipboard that was just received
  if (clipboardType === "text") {
    if (!clipboardText || isClipboardTextEmpty(clipboardText)) {
      return;
    }

    if (
      lastTimeRead &&
      currentTime - lastTimeRead < 5000 &&
      lastTextRead &&
      isClipboardTextEquals(lastTextRead, clipboardText)
    ) {
      return;
    }
  }

  if (clipboardType === "image") {
    if (!clipboardImage) {
      return;
    }

    if (
      lastTimeRead &&
      currentTime - lastTimeRead < 5000 &&
      lastImageSha256Read === clipboardImageSha256
    ) {
      return;
    }
  }

  if (clipboardType === "files") {
    if (!clipboardFilePaths) {
      return;
    }

    if (
      lastTimeRead &&
      currentTime - lastTimeRead < 5000 &&
      lastClipboardFilePathsRead &&
      isArrayEquals(lastClipboardFilePathsRead, clipboardFilePaths)
    ) {
      return;
    }
  }

  const writeTime = getNextWriteTime(syncFolder);
  let destinationPath: string;
  if (clipboardType === "text") {
    destinationPath = path.join(
      syncFolder,
      `${writeTime}-${hostName}.text.json`
    );
    fs.writeFileSync(destinationPath, JSON.stringify(clipboardText, null, 2), {
      encoding: "utf8",
    });
  } else if (clipboardType === "image") {
    destinationPath = path.join(syncFolder, `${writeTime}-${hostName}.png`);
    fs.writeFileSync(destinationPath, clipboardImage);
  } else if (clipboardType === "files") {
    clipboardFilesCount = getTotalNumberOfFiles(clipboardFilePaths);
    destinationPath = path.join(
      syncFolder,
      `${writeTime}-${hostName}.${clipboardFilesCount}_files`
    );
    fs.mkdirSync(destinationPath);
    for (const filePath of clipboardFilePaths) {
      const fullDestination = path.join(
        destinationPath,
        path.basename(filePath)
      );
      if (fs.statSync(filePath).isDirectory()) {
        copyFolderRecursive(filePath, fullDestination);
      } else {
        fs.copyFileSync(filePath, fullDestination);
      }
    }
  }
  console.log(`Clipboard written to ${destinationPath}`);
  lastTimeWritten = writeTime;

  setIconFor5Seconds("clipboard_sent");
};

const receiveClipboardFromFile = (file: string) => {
  const currentTime = Date.now();

  const clipboardFile = ClipboardFile.fromFileName(file);
  if (!clipboardFile || clipboardFile.from === "myself") {
    if (!isIsReceivingFile(file)) {
      console.error(`Unrecognized file: ${file}`);
    }
    return;
  }
  const currentFileTime = clipboardFile.number;

  const currentClipboard = ClipboardFile.fromCurrentClipboard();

  if (!currentClipboard) {
    console.error("Error reading current clipboard");
    return;
  }

  if (
    clipboardFile.content instanceof ClipboardText &&
    !config.get("receiveTexts", true)
  ) {
    return;
  }

  if (
    clipboardFile.content instanceof ClipboardImage &&
    !config.get("receiveImages", true)
  ) {
    return;
  }

  if (
    clipboardFile.content instanceof ClipboardFiles &&
    !config.get("receiveFiles", true)
  ) {
    return;
  }

  try {
    clipboardFile.content.loadFromFileOrFolder(file);
  } catch (error) {
    if (error instanceof Error) {
      console.error(error.message);
    }
    return;
  }

  if (clipboardFile.content.isEmpty()) {
    return;
  }

  if (clipboardFile.content.equals(currentClipboard.content)) {
    return;
  }

  // Skips the read if a newer file was already wrote
  if (
    isThereMoreThanOneClipboardFile(syncFolder) &&
    lastTimeWritten &&
    currentFileTime < lastTimeWritten
  ) {
    return;
  }

  if (fileClipboardType === "text") {
    clipboard.write(newText);
    lastTextRead = newText;
  } else if (fileClipboardType === "image") {
    clipboard.writeImage(nativeImage.createFromBuffer(newImage));
    lastImageSha256Read = newImageSha256;
  } else if (fileClipboardType === "files") {
    clipboardEx.writeFilePaths(newFilePaths);
    lastClipboardFilePathsRead = newFilePaths;
  }
  console.log(`Clipboard was read from ${file}`);
  lastTimeRead = currentTime;

  setIconFor5Seconds("clipboard_received");
};

const askForFolder = () => {
  let previousFolder = config.get("folder");

  const foldersSelected = dialog.showOpenDialogSync({
    title: "Select folder to save and read clipboard files",
    properties: ["openDirectory"],
    defaultPath: previousFolder,
  });

  let folderSelected;
  if (foldersSelected) {
    folderSelected = foldersSelected[0];
  }

  if (!folderSelected && !previousFolder) {
    dialog.showErrorBox(
      "Folder was not selected",
      "Please start the application again to select a folder."
    );
    finish(1);
    return;
  } else if (!folderSelected) {
    return;
  }
  syncFolder = folderSelected;
  config.set("folder", folderSelected);

  if (folderSelected !== previousFolder) {
    reload();
  }
};

const initialize = async () => {
  syncFolder = config.get("folder");

  if (!(typeof syncFolder === "string" || typeof syncFolder === "undefined")) {
    return;
  }

  if (
    !syncFolder ||
    (fs.existsSync(syncFolder) && !fs.lstatSync(syncFolder).isDirectory())
  ) {
    askForFolder();
  }

  if (!fs.existsSync(syncFolder)) {
    fs.mkdirSync(syncFolder);
  }

  if (
    config.get("sendTexts", true) ||
    config.get("sendImages", true) ||
    config.get("sendFiles", true)
  ) {
    clipboardListener = require("clipboard-event");
    clipboardListener.startListening();
    clipboardListener.on(
      "change",
      // Wait 100ms so that clipboard is fully written
      () => setTimeout(writeClipboardToFile, 100)
    );
  }

  if (
    config.get("receiveTexts", true) ||
    config.get("receiveImages", true) ||
    config.get("receiveFiles", true)
  ) {
    // Watches for files and reads clipboard from it
    clipboardFilesWatcher = await watcher.subscribe(
      syncFolder,
      (err, events) => {
        if (err) {
          console.error(err);
          return;
        }
        // Execute readCLipboardFromFile only if there is a "create" event
        for (const event of events) {
          if (event.type === "create") {
            receiveClipboardFromFile(event.path);
          }
        }
      },
      {
        backend: "watchman",
        // This filters out temporary files created by the OneDrive client, example:
        // "C:\Users\user\OneDrive\Clipboard Sync\1-my-pc.txt~RF1a1c3c.TMP"
        ignore: ["**/*~*.TMP"],
      }
    );

    // Create a file to indicate that this computer is receiving clipboards
    fs.writeFileSync(path.join(syncFolder, hostNameIsReceivingFileName), "");
  }

  if (config.get("autoCleanup", true)) {
    filesCleanerTask = cron.schedule(
      "*/1 * * * *",
      () => {
        cleanFiles(syncFolder);
      },
      {
        scheduled: true,
        runOnInit: true,
      }
    );
  }
};

const cleanup = () => {
  // Deletes the file that indicates that this computer is receiving clipboards
  fs.rmSync(path.join(syncFolder, hostNameIsReceivingFileName), {
    force: true,
  });

  if (clipboardListener) {
    clipboardListener.stopListening();
    clipboardListener = null;
  }

  if (clipboardFilesWatcher) {
    clipboardFilesWatcher.unsubscribe();
    clipboardFilesWatcher = null;
  }

  if (filesCleanerTask) {
    filesCleanerTask.stop();
    filesCleanerTask = null;
  }
};

const reload = () => {
  console.log("Reloading configuration...");
  cleanup();
  initialize();
};

const finish = (exitCode: number = 0) => {
  cleanup();
  app.exit(exitCode);
};

const getAppIcon = () => {
  const iconExtension =
    process.platform === "win32"
      ? "ico"
      : process.platform === "darwin"
      ? "icns"
      : "png";

  return path.resolve(
    __dirname,
    `../assets/appicons/${iconExtension}/icon.${iconExtension}`
  );
};

const getTrayIcon = (icon: ClipboardIcon) => {
  const iconExtension = process.platform === "win32" ? "ico" : "png";

  return path.resolve(
    __dirname,
    `../assets/trayicons/${iconExtension}/${icon}.${iconExtension}`
  );
};

const setIconFor5Seconds = (icon: ClipboardIcon) => {
  appIcon.setImage(getTrayIcon(icon));

  if (iconWaiter) {
    clearTimeout(iconWaiter);
  }
  iconWaiter = setTimeout(() => {
    appIcon.setImage(getTrayIcon("clipboard"));
  }, 5000);
};

const handleCheckBoxClick = (checkBox: Electron.MenuItem, key: string) => {
  config.set(key, checkBox.checked);
  reload();
};

const handleCleanupCheckBox = (checkBox: Electron.MenuItem) => {
  config.set("autoCleanup", checkBox.checked);
  reload();
};

let updateLabel = "Check for updates";

const isUpdateAvailable = async () => {
  let available = false;

  const newVersionUrl = await getRedirectedUrl({
    hostname: "github.com",
    path: "/felipecrs/clipboard-sync/releases/latest",
  });
  if (typeof newVersionUrl !== "string") {
    console.error(`Could not get latest version from GitHub.`);
    return false;
  }
  const newVersion = newVersionUrl.split("/").pop().replace(/^v/, "");
  const currentVersion = app.getVersion();
  if (semver.gt(newVersion, currentVersion)) {
    available = true;
  }

  if (available) {
    updateLabel = "Download update";
    setContextMenu();
    return {
      newVersion,
      newVersionUrl,
    };
  }
  return false;
};

const checkForUpdatesPress = async () => {
  const update = await isUpdateAvailable();
  if (update) {
    new Notification({
      title: "Update available",
      body: "Opening download page...",
      icon: getAppIcon(),
    }).show();
    if (process.platform === "win32") {
      shell.openExternal(
        `https://github.com/felipecrs/clipboard-sync/releases/download/v${update.newVersion}/Clipboard.Sync-${update.newVersion}.Setup.exe`
      );
    }
    shell.openExternal("https://github.com/felipecrs/clipboard-sync/releases");
  } else {
    new Notification({
      title: "No updates found",
      body: "You are already running the latest version.",
      icon: getAppIcon(),
    }).show();
  }
};

const autoCheckForUpdates = async () => {
  const update = await isUpdateAvailable();
  if (update) {
    new Notification({
      title: "Update available",
      body: "Click in the tray icon to download.",
      icon: getAppIcon(),
    }).show();
  }
};

const setContextMenu = () => {
  const menu = Menu.buildFromTemplate([
    {
      label: "Send",
      type: "submenu",
      toolTip: "Select what to send",
      submenu: [
        {
          label: "Texts",
          type: "checkbox",
          checked: config.get("sendTexts", true),
          click: (checkBox) => handleCheckBoxClick(checkBox, "sendTexts"),
          toolTip: "Whether to enable sending copied texts or not",
        },
        {
          label: "Images",
          type: "checkbox",
          checked: config.get("sendImages", true),
          click: (checkBox) => handleCheckBoxClick(checkBox, "sendImages"),
          toolTip: "Whether to enable sending copied images or not",
        },
        {
          label: "Files",
          type: "checkbox",
          checked: config.get("sendFiles", true),
          click: (checkBox) => handleCheckBoxClick(checkBox, "sendFiles"),
          toolTip: "Whether to enable sending copied files or not",
        },
      ],
    },
    {
      label: "Receive",
      type: "submenu",
      toolTip: "Select what to receive",
      submenu: [
        {
          label: "Texts",
          type: "checkbox",
          checked: config.get("receiveTexts", true),
          click: (checkBox) => handleCheckBoxClick(checkBox, "receiveTexts"),
          toolTip: "Whether to enable receiving texts or not",
        },
        {
          label: "Images",
          type: "checkbox",
          checked: config.get("receiveImages", true),
          click: (checkBox) => handleCheckBoxClick(checkBox, "receiveImages"),
          toolTip: "Whether to enable receiving images or not",
        },
        {
          label: "Files",
          type: "checkbox",
          checked: config.get("receiveFiles", true),
          click: (checkBox) => handleCheckBoxClick(checkBox, "receiveFiles"),
          toolTip: "Whether to enable receiving files or not",
        },
      ],
    },
    { type: "separator" },
    {
      label: "Auto-clean",
      type: "checkbox",
      checked: config.get("autoCleanup", true),
      click: handleCleanupCheckBox,
      toolTip: `Auto-clean the files created by ${app.name}`,
    },
    {
      label: "Auto-start on login",
      type: "checkbox",
      checked: app.getLoginItemSettings().openAtLogin,
      click: (checkBox: Electron.MenuItem) => {
        app.setLoginItemSettings({
          openAtLogin: checkBox.checked,
        });
      },
    },
    { type: "separator" },
    { label: "Change folder", type: "normal", click: askForFolder },
    {
      label: "Open folder",
      type: "normal",
      click: () => {
        shell.openPath(syncFolder);
      },
    },
    { type: "separator" },
    {
      label: updateLabel,
      type: "normal",
      click: checkForUpdatesPress,
    },
    {
      label: "GitHub",
      type: "normal",
      click: () => {
        shell.openExternal("https://github.com/felipecrs/clipboard-sync");
      },
      toolTip:
        "Open the GitHub page of the project. Please star it if you like it!",
    },
    { type: "separator" },
    {
      label: "Exit",
      type: "normal",
      click: () => finish(),
    },
  ]);
  appIcon.setContextMenu(menu);
};

const createAppIcon = () => {
  appIcon = new Tray(
    getTrayIcon("clipboard"),
    // This GUID should not be changed. It ensures the tray icon position is kept between app updates.
    "72812af2-6bcc-40d9-b35d-0b43e72ac346"
  );
  setContextMenu();
  appIcon.setToolTip(`${app.name} v${app.getVersion()}`);

  // sets left click to open the context menu too
  appIcon.on("click", () => {
    appIcon.popUpContextMenu();
  });

  appIcon.on("double-click", () => {
    shell.openPath(syncFolder);
  });

  // Set PATH to include bundled watchman binaries
  const watchmanBinDir = path.resolve(
    path.join(
      __dirname,
      "../binaries/win32/x64/watchman-v2023.08.07.00-windows/bin"
    )
  );

  process.env.PATH = `${process.env.PATH};${watchmanBinDir}`;

  initialize();

  if (firstTime) {
    firstTime = false;
    autoCheckForUpdates();
  }
};

// This method will be called when Electron has finished
// initialization and is ready to create browser windows.
// Some APIs can only be used after this event occurs.
app.on("ready", createAppIcon);

app.on("window-all-closed", () => {
  finish();
});

app.on("before-quit", () => {
  cleanup();
});
