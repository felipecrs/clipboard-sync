import type { ClipboardEventListener } from "clipboard-event";
import {
  Menu,
  Notification,
  Tray,
  app,
  clipboard,
  dialog,
  nativeImage,
  shell,
} from "electron";
import clipboardEx from "electron-clipboard-ex";
import log from "electron-log";
import Store from "electron-store";
import cron from "node-cron";
import { StatWatcher, watch } from "node:fs";
import fs from "node:fs/promises";
import path from "node:path";
import { gt as semverGreaterThan } from "semver";

import {
  ClipboardText,
  ClipboardType,
  ParsedClipboardFileName,
  cleanFiles,
  getNextFileNumber,
  isClipboardTextEmpty,
  isClipboardTextEquals,
  isIsReceivingFile,
  isThereMoreThanOneClipboardFile,
  parseClipboardFileName,
} from "./clipboard.js";
import { hostName, hostNameIsReceivingFileName } from "./global.js";
import {
  calculateSha256,
  copyFolderRecursive,
  getFilesSizeInMb,
  getRedirectedUrl,
  getTotalNumberOfFiles,
  isArrayEquals,
} from "./utils.js";

// Handle creating/removing shortcuts on Windows when installing/uninstalling.
if ((await import("electron-squirrel-startup")).default) {
  console.error("Squirrel event handled. Quitting...");
  app.exit();
}

const gotTheLock = app.requestSingleInstanceLock();

if (!gotTheLock) {
  console.error("Another instance is already running. Quitting...");
  app.exit();
}

log.errorHandler.startCatching({ showDialog: false });
log.eventLogger.startLogging();

if (process.platform === "darwin") {
  // This is just during development, because LSUIElement=1 already handles this on the packaged application
  app.dock.hide();
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

let appIcon: Tray = null;

let syncFolder: string = null;

let lastTextRead: ClipboardText = null;
let lastImageSha256Read: string = null;
let lastClipboardFilePathsRead: string[] = null;
let lastTimeRead: number = null;

let lastTextWritten: ClipboardText = null;
let lastImageSha256Written: string = null;
let lastTimeWritten: number = null;
let lastFileNumberWritten: number = null;

let clipboardListener: ClipboardEventListener = null;
let clipboardFilesWatcher: StatWatcher = null;
let filesCleanerTask: cron.ScheduledTask = null;
let iconWaiter: NodeJS.Timeout = null;

let lastTimeClipboardChecked: number = null;
let lastTimeFilesChecked: number = null;

const writeClipboardToFile = async () => {
  const currentTime = Date.now();

  // Avoids sending the clipboard if there is no other computer receiving
  if (
    (await fs.readdir(syncFolder)).filter(
      (file) => isIsReceivingFile(file) && file !== hostNameIsReceivingFileName
    ).length === 0
  ) {
    log.info(
      "No other computer is receiving clipboards. Skipping clipboard send..."
    );
    return;
  }

  let clipboardType: ClipboardType;
  let clipboardText: ClipboardText;
  let clipboardImage: Buffer;
  let clipboardImageSha256: string;
  let clipboardFilePaths: string[];
  let filesCount: number;
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
      clipboardText = {};
      if (clipboardFormats.includes("text/plain")) {
        clipboardText.text = clipboard.readText() ?? undefined;
      }
      if (clipboardFormats.includes("text/html")) {
        clipboardText.html = clipboard.readHTML() ?? undefined;
      }
      if (clipboardFormats.includes("text/rtf")) {
        clipboardText.rtf = clipboard.readRTF() ?? undefined;
      }
      clipboardType = "text";
    } else if (clipboardFormats.includes("image/png")) {
      if (!config.get("sendImages", true)) {
        return;
      }
      clipboardImage = clipboard.readImage().toPNG();
      clipboardImageSha256 = calculateSha256(clipboardImage);
      clipboardType = "image";
    } else if (clipboardFormats.includes("text/uri-list")) {
      if (!config.get("sendFiles", true)) {
        return;
      }
      clipboardFilePaths = clipboardEx.readFilePaths();
      clipboardType = "files";
    }
  } catch (error) {
    log.error(`Error reading current clipboard:\n${error}`);
    return;
  }

  if (!clipboardType) {
    return;
  }

  // Prevent sending the clipboard that was just received, or resend the same clipboard too quickly
  if (
    clipboardType === "text" &&
    (isClipboardTextEmpty(clipboardText) ||
      (lastTimeRead &&
        currentTime - lastTimeRead < 5000 &&
        isClipboardTextEquals(lastTextRead, clipboardText)) ||
      (lastTimeWritten &&
        currentTime - lastTimeWritten < 10000 &&
        isClipboardTextEquals(lastTextWritten, clipboardText)))
  ) {
    return;
  }

  if (
    clipboardType === "image" &&
    (!clipboardImage ||
      (lastTimeRead &&
        currentTime - lastTimeRead < 5000 &&
        lastImageSha256Read === clipboardImageSha256) ||
      (lastTimeWritten &&
        currentTime - lastTimeWritten < 10000 &&
        lastImageSha256Written === clipboardImageSha256))
  ) {
    return;
  }

  if (
    clipboardType === "files" &&
    (!clipboardFilePaths ||
      (lastTimeRead &&
        currentTime - lastTimeRead < 5000 &&
        isArrayEquals(lastClipboardFilePathsRead, clipboardFilePaths)) ||
      (await getFilesSizeInMb(clipboardFilePaths)) > 100)
  ) {
    return;
  }

  const fileNumber = await getNextFileNumber(syncFolder);
  let destinationPath: string;
  if (clipboardType === "text") {
    destinationPath = path.join(
      syncFolder,
      `${fileNumber}-${hostName}.text.json`
    );
    await fs.writeFile(
      destinationPath,
      JSON.stringify(clipboardText, null, 2),
      {
        encoding: "utf8",
      }
    );
    lastTextWritten = clipboardText;
  } else if (clipboardType === "image") {
    destinationPath = path.join(syncFolder, `${fileNumber}-${hostName}.png`);
    await fs.writeFile(destinationPath, clipboardImage);
    lastImageSha256Written = clipboardImageSha256;
  } else if (clipboardType === "files") {
    filesCount = await getTotalNumberOfFiles(clipboardFilePaths);
    destinationPath = path.join(
      syncFolder,
      `${fileNumber}-${hostName}.${filesCount}_files`
    );
    await fs.mkdir(destinationPath);
    for (const filePath of clipboardFilePaths) {
      const fullDestination = path.join(
        destinationPath,
        path.basename(filePath)
      );
      if ((await fs.stat(filePath)).isDirectory()) {
        await copyFolderRecursive(filePath, fullDestination);
      } else {
        await fs.copyFile(filePath, fullDestination);
      }
    }
  }
  log.info(`Clipboard written to ${destinationPath}`);
  lastTimeWritten = currentTime;
  lastFileNumberWritten = fileNumber;

  setIconFor5Seconds("clipboard_sent");
};

const readClipboardFromFile = async (parsedFile: ParsedClipboardFileName) => {
  const currentTime = Date.now();

  const file = parsedFile.file;
  const currentFileNumber = parsedFile.number;
  const fileClipboardType = parsedFile.clipboardType;

  let newText: ClipboardText;
  let newImage: Buffer;
  let newImageSha256: string;
  let newFilePaths: string[];
  let newFilesCount: number;
  try {
    if (fileClipboardType === "text") {
      if (!config.get("receiveTexts", true)) {
        return;
      }
      newText = JSON.parse(
        await fs.readFile(file, {
          encoding: "utf8",
        })
      );
    } else if (fileClipboardType === "image") {
      if (!config.get("receiveImages", true)) {
        return;
      }
      newImage = await fs.readFile(file);
      newImageSha256 = calculateSha256(newImage);
    } else if (fileClipboardType === "files") {
      if (!config.get("receiveFiles", true)) {
        return;
      }
      newFilesCount = parsedFile.filesCount;
      if (!newFilesCount) {
        // This should not happen, but just in case
        log.warn(`Could not read the number of files in ${file}. Skipping...`);
        return;
      }
      const filesCountInFolder = await getTotalNumberOfFiles([file]);
      if (newFilesCount !== filesCountInFolder) {
        log.info(
          `Not all files are yet present in _files folder. Current: ${filesCountInFolder}, expected: ${newFilesCount}. Skipping...`
        );
        return;
      }
      newFilePaths = (await fs.readdir(file)).map((fileName: string) =>
        path.join(file, fileName)
      );
    }
  } catch (error) {
    log.error(`Error reading clipboard ${file}:\n${error}`);
    return;
  }

  let currentText: ClipboardText;
  let currentImage: Buffer;
  let currentClipboardType: ClipboardType;
  let currentImageSha256: string;
  let currentFilePaths: string[];

  const clipboardFormats = clipboard.availableFormats();
  try {
    if (
      clipboardFormats.includes("text/plain") ||
      clipboardFormats.includes("text/html") ||
      clipboardFormats.includes("text/rtf")
    ) {
      currentText = {};
      if (clipboardFormats.includes("text/plain")) {
        currentText.text = clipboard.readText() ?? undefined;
      }
      if (clipboardFormats.includes("text/html")) {
        currentText.html = clipboard.readHTML() ?? undefined;
      }
      if (clipboardFormats.includes("text/rtf")) {
        currentText.rtf = clipboard.readRTF() ?? undefined;
      }
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
    log.error(`Error reading current clipboard: ${error}`);
    return;
  }

  // Prevents writing duplicated stuff to clipboard
  if (currentClipboardType === fileClipboardType) {
    if (
      currentClipboardType === "text" &&
      (isClipboardTextEmpty(newText) ||
        isClipboardTextEquals(currentText, newText))
    ) {
      return;
    } else if (
      fileClipboardType === "image" &&
      (!newImage || currentImageSha256 === newImageSha256)
    ) {
      return;
    } else if (
      fileClipboardType === "files" &&
      (!newFilePaths || isArrayEquals(currentFilePaths, newFilePaths))
    ) {
      return;
    }
  }

  // Skips the read if a newer clipboard was already sent, which can happen if
  // OneDrive takes too long to sync
  if (
    (await isThereMoreThanOneClipboardFile(syncFolder, "from-myself")) &&
    lastFileNumberWritten &&
    currentFileNumber < lastFileNumberWritten
  ) {
    log.info(
      `Skipping reading clipboard from ${file} as a newer clipboard was already sent`
    );
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
  log.info(`Clipboard was read from ${file}`);
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
    quit(1);
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

  try {
    if (!syncFolder || !(await fs.lstat(syncFolder)).isDirectory()) {
      askForFolder();
    }
  } catch (error) {
    if (error.code === "ENOENT") {
      askForFolder();
    } else {
      throw error;
    }
  }

  try {
    await fs.access(syncFolder);
  } catch (error) {
    if (error.code === "ENOENT") {
      await fs.mkdir(syncFolder);
    } else {
      throw error;
    }
  }

  if (
    config.get("sendTexts", true) ||
    config.get("sendImages", true) ||
    config.get("sendFiles", true)
  ) {
    clipboardListener = (await import("clipboard-event")).default;
    clipboardListener.startListening();
    clipboardListener.on("change", () => {
      // Prevents duplicated clipboard events
      const currentTime = Date.now();
      if (
        lastTimeClipboardChecked &&
        currentTime - lastTimeClipboardChecked < 1000
      ) {
        return;
      }
      lastTimeClipboardChecked = currentTime;

      // Wait 100ms so that clipboard is fully written
      setTimeout(writeClipboardToFile, 100);
    });
  }

  if (
    config.get("receiveTexts", true) ||
    config.get("receiveImages", true) ||
    config.get("receiveFiles", true)
  ) {
    // Watches for files and reads clipboard from it

    // Prevents events for files that previously existed
    lastTimeFilesChecked = Date.now();

    clipboardFilesWatcher = watch(
      syncFolder,
      { recursive: true },
      (eventType: "rename" | "change", filename: string) => {
        if (eventType !== "change") {
          return;
        }

        // This filters out temporary files created by the OneDrive client, example:
        // "C:\Users\user\OneDrive\Clipboard Sync\1-my-pc.txt.json~RF1a1c3c.TMP"
        if (filename.match(/~RF[0-9a-f]+\.TMP$/)) {
          return;
        }

        // Prevents duplicated events
        const currentTime = Date.now();
        if (lastTimeFilesChecked && currentTime - lastTimeFilesChecked < 1000) {
          return;
        }
        lastTimeFilesChecked = currentTime;

        const parsedFile = parseClipboardFileName(
          path.join(syncFolder, filename),
          syncFolder,
          "from-others"
        );

        if (!parsedFile) {
          return;
        }

        // Wait 200ms so that file is fully written
        setTimeout(() => readClipboardFromFile(parsedFile), 200);
      }
    );

    // Create a file to indicate that this computer is receiving clipboards
    await fs.writeFile(path.join(syncFolder, hostNameIsReceivingFileName), "");
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

const cleanup = async () => {
  // Deletes the file that indicates that this computer is receiving clipboards
  if (syncFolder) {
    await fs.rm(path.join(syncFolder, hostNameIsReceivingFileName), {
      force: true,
    });
  }

  if (clipboardListener) {
    clipboardListener.stopListening();
    clipboardListener = null;
  }

  if (clipboardFilesWatcher) {
    clipboardFilesWatcher.unref();
    clipboardFilesWatcher = null;
  }

  if (filesCleanerTask) {
    filesCleanerTask.stop();
    filesCleanerTask = null;
  }
};

const reload = async () => {
  log.info("Reloading configuration...");
  await cleanup();
  await initialize();
};

const getAppIcon = () => {
  const iconExtension =
    process.platform === "win32"
      ? "ico"
      : process.platform === "darwin"
      ? "icns"
      : "png";

  return path.resolve(
    app.getAppPath(),
    `resources/appicons/${iconExtension}/icon.${iconExtension}`
  );
};

const getTrayIcon = (icon: ClipboardIcon) => {
  const iconExtension = process.platform === "win32" ? "ico" : "png";

  return path.resolve(
    app.getAppPath(),
    `resources/trayicons/${iconExtension}/${icon}.${iconExtension}`
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
  let newVersionUrl;
  try {
    newVersionUrl = await getRedirectedUrl({
      hostname: "github.com",
      path: "/felipecrs/clipboard-sync/releases/latest",
    });
  } catch (error) {
    log.error(`Could not get latest version from GitHub:\n${error}`);
    return false;
  }

  const newVersion = newVersionUrl.split("/").pop().replace(/^v/, "");
  const currentVersion = app.getVersion();

  if (semverGreaterThan(newVersion, currentVersion)) {
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
      click: () => quit(),
    },
  ]);
  appIcon.setContextMenu(menu);
};

const createAppIcon = async () => {
  appIcon = new Tray(
    getTrayIcon("clipboard")
    // This GUID should not be changed. It ensures the tray icon position is kept between app updates.
    // TODO: restore GUID, see:
    // "72812af2-6bcc-40d9-b35d-0b43e72ac346"
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

  await initialize();

  await autoCheckForUpdates();
};

// This method will be called when Electron has finished
// initialization and is ready to create browser windows.
// Some APIs can only be used after this event occurs.
app.on("ready", createAppIcon);

let cleanupBeforeQuitDone = false;
const cleanupBeforeQuit = async () => {
  if (cleanupBeforeQuitDone) {
    return;
  }
  await cleanup();
  cleanupBeforeQuitDone = true;
};

const quit = (exitCode: number = 0) => {
  if (exitCode === 0) {
    app.quit();
  } else {
    cleanupBeforeQuit();
    app.exit(exitCode);
  }
};

app.on("before-quit", cleanupBeforeQuit);
