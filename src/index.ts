import * as watcher from "@parcel/watcher";
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
import * as clipboardEx from "electron-clipboard-ex";
import * as Store from "electron-store";
import * as cron from "node-cron";
import * as fs from "node:fs";
import * as path from "node:path";
import * as semver from "semver";

import {
  cleanFiles,
  getItemNumber,
  getNextWriteTime,
  isThereMoreThanOneClipboardFile,
} from "./clipboard";
import { hostname } from "./global";
import {
  calculateSha256,
  copyFolderRecursive,
  getFilesSizeInMb,
  getRedirectedUrl,
  getTotalNumberOfFiles,
  isArrayEquals,
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
  send: boolean;
  receive: boolean;
  autoCleanup: boolean;
};

type ClipboardListener = {
  startListening: () => void;
  on: (arg0: string, arg1: () => void) => void;
  stopListening: () => void;
};

type ClipboardIcon = "clipboard" | "clipboard_sent" | "clipboard_received";

type ClipboardType = "text" | "image" | "files";

const config = new Store<ConfigType>({
  defaults: {
    send: true,
    receive: true,
    autoCleanup: true,
  },
});

let appIcon: Tray = null;
let firstTime = true;

let syncFolder: string = null;

let lastTimeWritten: number = null;

let lastTextRead: string = null;
let lastImageSha256Read: string = null;
let lastClipboardFilePathsRead: string[] = null;
let lastTimeRead: number = null;

let clipboardListener: ClipboardListener = null;
let clipboardFilesWatcher: watcher.AsyncSubscription = null;
let filesCleanerTask: cron.ScheduledTask = null;
let iconWaiter: NodeJS.Timeout = null;

let lastTimeChecked: number = null;

const writeClipboardToFile = () => {
  // Prevents duplicated clipboard events
  const currentTime = Date.now();
  if (lastTimeChecked && currentTime - lastTimeChecked < 1000) {
    return;
  }
  lastTimeChecked = currentTime;

  let clipboardType: ClipboardType;
  let clipboardText: string;
  let clipboardImage: Buffer;
  let clipboardImageSha256: string;
  let clipboardFilePaths: string[];
  let clipboardFilesCount: number;
  const clipboardFormats = clipboard.availableFormats();

  try {
    if (clipboardFormats.includes("text/plain")) {
      clipboardText = clipboard.readText();
      clipboardType = "text";
    } else if (clipboardFormats.includes("image/png")) {
      clipboardImage = clipboard.readImage().toPNG();
      clipboardImageSha256 = calculateSha256(clipboardImage);
      clipboardType = "image";
    } else if (clipboardFormats.includes("text/uri-list")) {
      clipboardFilePaths = clipboardEx.readFilePaths();
      clipboardType = "files";
    }
  } catch (error) {
    console.error("Error reading current clipboard");
  }

  if (!clipboardType) {
    return;
  }

  // Prevent sending the clipboard that was just received
  if (
    clipboardType === "text" &&
    (!clipboardText ||
      (lastTimeRead &&
        currentTime - lastTimeRead < 5000 &&
        lastTextRead === clipboardText))
  ) {
    return;
  }

  if (
    clipboardType === "image" &&
    (!clipboardImage ||
      (lastTimeRead &&
        currentTime - lastTimeRead < 5000 &&
        lastImageSha256Read === clipboardImageSha256))
  ) {
    return;
  }

  if (
    clipboardType === "files" &&
    (!clipboardFilePaths ||
      (lastTimeRead &&
        currentTime - lastTimeRead < 5000 &&
        isArrayEquals(lastClipboardFilePathsRead, clipboardFilePaths)) ||
      getFilesSizeInMb(clipboardFilePaths) > 100)
  ) {
    return;
  }

  const writeTime = getNextWriteTime(syncFolder);
  let destinationPath: string;
  if (clipboardType === "text") {
    destinationPath = path.join(syncFolder, `${writeTime}-${hostname}.txt`);
    fs.writeFileSync(destinationPath, clipboardText, {
      encoding: "utf8",
    });
  } else if (clipboardType === "image") {
    destinationPath = path.join(syncFolder, `${writeTime}-${hostname}.png`);
    fs.writeFileSync(destinationPath, clipboardImage);
  } else if (clipboardType === "files") {
    clipboardFilesCount = getTotalNumberOfFiles(clipboardFilePaths);
    destinationPath = path.join(
      syncFolder,
      `${writeTime}-${hostname}.${clipboardFilesCount}_files`
    );
    fs.mkdirSync(destinationPath);
    clipboardFilePaths.forEach((filePath: string) => {
      const fullDestination = path.join(
        destinationPath,
        path.basename(filePath)
      );
      if (fs.statSync(filePath).isDirectory()) {
        copyFolderRecursive(filePath, fullDestination);
      } else {
        fs.copyFileSync(filePath, fullDestination);
      }
    });
  }
  console.log(`Clipboard written to ${destinationPath}`);
  lastTimeWritten = writeTime;

  setIconFor5Seconds("clipboard_sent");
};

const readClipboardFromFile = (file: string) => {
  const currentTime = Date.now();

  const filename = path.relative(syncFolder, file).split(path.sep)[0];
  file = path.join(syncFolder, filename);

  const currentFileTime = getItemNumber(file, "from-others");
  if (!currentFileTime) {
    return;
  }

  const fileName = path.parse(file).name;
  const fileExtension = path.parse(file).ext;
  const fileClipboardType =
    fileExtension === ".txt"
      ? "text"
      : fileExtension === ".png"
      ? "image"
      : fileExtension.endsWith("_files")
      ? "files"
      : null;

  let currentText: string;
  let currentImage: Buffer;
  let currentClipboardType: ClipboardType;
  let currentImageSha256: string;
  let currentFilePaths: string[];

  const clipboardFormats = clipboard.availableFormats();
  try {
    if (clipboardFormats.includes("text/plain")) {
      currentText = clipboard.readText();
      currentClipboardType = "text";
    } else if (clipboardFormats.includes("image/png")) {
      currentImage = clipboard.readImage().toPNG();
      currentImageSha256 = calculateSha256(currentImage);
      currentClipboardType = "image";
    } else if (clipboardFormats.includes("text/uri-list")) {
      currentFilePaths = clipboardEx.readFilePaths();
      currentClipboardType = "files";
    }
  } catch (error) {
    console.error("Error reading current clipboard");
  }

  let newText: string;
  let newImage: Buffer;
  let newImageSha256: string;
  let newFilePaths: string[];
  let newFilesCount: number;
  try {
    if (fileClipboardType === "text") {
      newText = fs.readFileSync(file, {
        encoding: "utf8",
      });
    } else if (fileClipboardType === "image") {
      newImage = fs.readFileSync(file);
      newImageSha256 = calculateSha256(newImage);
    } else if (fileClipboardType === "files") {
      const matches = fileExtension.match(/^\.(0|[1-9][0-9]*)_files$/);
      if (matches && matches.length > 0) {
        newFilesCount = parseInt(matches[1]);
      } else {
        console.error("Unrecognized _files folder, missing files count.");
        return;
      }
      newFilePaths = fs
        .readdirSync(file)
        .map((fileName: string) => path.join(file, fileName));
      const filesCountInFolder = getTotalNumberOfFiles(newFilePaths);
      if (newFilesCount !== filesCountInFolder) {
        console.error(
          `Not all files are yet present in _files folder. Current: ${filesCountInFolder}, expected: ${newFilesCount}. Skipping...`
        );
        return;
      }
    }
  } catch (error) {
    console.error(`Error reading clipboard from file ${fileName}`);
    return;
  }

  if (currentClipboardType === fileClipboardType) {
    if (
      currentClipboardType === "text" &&
      (!newText || currentText === newText)
    ) {
      // Prevents writing duplicated text to clipboard
      return;
    } else if (
      fileClipboardType === "image" &&
      (!newImage || currentImageSha256 === newImageSha256)
    ) {
      // Prevents writing duplicated image to clipboard
      return;
    } else if (
      fileClipboardType === "files" &&
      (!newFilePaths || isArrayEquals(currentFilePaths, newFilePaths))
    ) {
      // Prevents writing duplicated files to clipboard
      return;
    }
  }

  // Skips the read if a newer file was already wrote
  if (
    isThereMoreThanOneClipboardFile(syncFolder) &&
    lastTimeWritten &&
    currentFileTime <= lastTimeWritten
  ) {
    return;
  }

  if (fileClipboardType === "text") {
    clipboard.writeText(newText);
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

  if (config.get("send", true)) {
    clipboardListener = require("clipboard-event");
    clipboardListener.startListening();
    clipboardListener.on("change", writeClipboardToFile);
  }

  if (config.get("receive", true)) {
    // Watches for files and reads clipboard from it
    clipboardFilesWatcher = await watcher.subscribe(
      syncFolder,
      (err, events) => {
        // Execute readCLipboardFromFile only if there is a "create" event
        if (err) {
          console.error(err);
          return;
        }
        // Call readClipboardFromFile for each "create" event
        events.forEach((event) => {
          if (event.type === "create") {
            readClipboardFromFile(event.path);
          }
        });
      },
      {
        // TODO: Add support for other platforms
        backend: "windows",
        // This filters out temporary files created by the OneDrive client, example:
        // "C:\Users\user\OneDrive\Clipboard Sync\1-my-pc.txt~RF1a1c3c.TMP"
        ignore: ["**/*~*.TMP"],
      }
    );
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

const handleSendCheckBox = (checkBox: Electron.MenuItem) => {
  config.set("send", checkBox.checked);
  reload();
};

const handleReceiveCheckBox = (checkBox: Electron.MenuItem) => {
  config.set("receive", checkBox.checked);
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
      type: "checkbox",
      checked: config.get("send", true),
      click: handleSendCheckBox,
      toolTip: "Watch for new clipboards to send as files to the folder set",
    },
    {
      label: "Receive",
      type: "checkbox",
      checked: config.get("receive", true),
      click: handleReceiveCheckBox,
      toolTip: "Watch for new files on the folder set to receive to clipboard",
    },
    { type: "separator" },
    {
      label: "Auto-clean",
      type: "checkbox",
      checked: config.get("autoCleanup", true),
      click: handleCleanupCheckBox,
      toolTip: `Auto-clean the files created by ${app.name} older than 5 minutes, on every 5 minutes`,
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
  appIcon = new Tray(getTrayIcon("clipboard"));
  setContextMenu();
  appIcon.setToolTip(`${app.name} v${app.getVersion()}`);

  // sets left click to open the context menu too
  appIcon.on("click", () => {
    appIcon.popUpContextMenu();
  });

  appIcon.on("double-click", () => {
    shell.openPath(syncFolder);
  });

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
