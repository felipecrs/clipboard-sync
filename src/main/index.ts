import chokidar from "chokidar";
import type { ClipboardEventListener } from "clipboard-event";
import {
  Menu,
  Notification,
  Tray,
  app,
  clipboard,
  dialog,
  nativeImage,
  powerMonitor,
  shell,
} from "electron";
import log from "electron-log";
import Store from "electron-store";
import { execa } from "execa";
import cron from "node-cron";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import {
  gt as semverGreaterThan,
  gte as semverGreaterThanOrEqual,
} from "semver";
import { setTimeout as setTimeoutAsync } from "timers/promises";

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

// eslint-disable-next-line @typescript-eslint/ban-ts-comment
// @ts-ignore: clipboard-event is an optional dependency
let clipboardEx: typeof import("electron-clipboard-ex") | null = null;

if (process.platform !== "linux") {
  clipboardEx = require("electron-clipboard-ex");
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

type ClipboardIcon = "working" | "sent" | "received" | "suspended";

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

let initialized: boolean = false;
let initializingOrUnInitializing: boolean = false;
let clipboardListener: ClipboardEventListener = null;
let clipboardFilesWatcher: chokidar.FSWatcher = null;
let filesCleanerTask: cron.ScheduledTask = null;
let idleDetectorTask: cron.ScheduledTask = null;
let iconWaiter: NodeJS.Timeout = null;

let lastTimeClipboardChecked: number = null;

async function writeClipboardToFile(): Promise<void> {
  const currentTime = Date.now();

  // Avoids sending the clipboard if there is no other computer receiving
  if (
    (await fs.readdir(syncFolder)).filter(
      (file) => isIsReceivingFile(file) && file !== hostNameIsReceivingFileName,
    ).length === 0
  ) {
    log.info(
      "No other computer is receiving clipboards. Skipping clipboard send...",
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
    // macOS writes text/plain even for uri-list and image/png, so we check
    // them before checking for text/plain
    if (clipboardFormats.includes("text/uri-list") && clipboardEx) {
      if (!config.get("sendFiles", true)) {
        return;
      }
      clipboardFilePaths = clipboardEx.readFilePaths();
      clipboardType = "files";
    } else if (clipboardFormats.includes("image/png")) {
      if (!config.get("sendImages", true)) {
        return;
      }
      clipboardImage = clipboard.readImage().toPNG();
      clipboardImageSha256 = calculateSha256(clipboardImage);
      clipboardType = "image";
    } else if (
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
    }
  } catch (error) {
    log.error(`Error reading current clipboard:\n${error}`);
    return;
  }

  if (!clipboardType) {
    log.warn(`Unknown clipboard format: ${clipboardFormats}`);
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
      `${fileNumber}-${hostName}.text.json`,
    );
    await fs.writeFile(
      destinationPath,
      JSON.stringify(clipboardText, null, 2),
      {
        encoding: "utf8",
      },
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
      `${fileNumber}-${hostName}.${filesCount}_files`,
    );
    await fs.mkdir(destinationPath);
    for (const filePath of clipboardFilePaths) {
      const fullDestination = path.join(
        destinationPath,
        path.basename(filePath),
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

  setIconFor5Seconds("sent");
}

async function readClipboardFromFile(
  parsedFile: ParsedClipboardFileName,
): Promise<void> {
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
        }),
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
          `Not all files are yet present in _files folder. Current: ${filesCountInFolder}, expected: ${newFilesCount}. Skipping...`,
        );
        return;
      }
      newFilePaths = (await fs.readdir(file)).map((fileName: string) =>
        path.join(file, fileName),
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
    // macOS writes text/plain even for uri-list and image/png, so we check
    // them before checking for text/plain
    if (clipboardFormats.includes("text/uri-list") && clipboardEx) {
      currentFilePaths = clipboardEx.readFilePaths();
      currentClipboardType = "files";
    } else if (clipboardFormats.includes("image/png")) {
      currentImage = clipboard.readImage().toPNG();
      currentImageSha256 = calculateSha256(currentImage);
      currentClipboardType = "image";
    } else if (
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
      `Skipping reading clipboard from ${file} as a newer clipboard was already sent`,
    );
    return;
  }

  if (fileClipboardType === "text") {
    clipboard.write(newText);
    lastTextRead = newText;
  } else if (fileClipboardType === "image") {
    clipboard.writeImage(nativeImage.createFromBuffer(newImage));
    lastImageSha256Read = newImageSha256;
  } else if (fileClipboardType === "files" && clipboardEx) {
    clipboardEx.writeFilePaths(newFilePaths);
    lastClipboardFilePathsRead = newFilePaths;
  }
  log.info(`Clipboard was read from ${file}`);
  lastTimeRead = currentTime;

  setIconFor5Seconds("received");
}

function askForFolder(): void {
  const previousFolder = config.get("folder");

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
      "Please start the application again to select a folder.",
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
}

async function initialize(handleTasks = true): Promise<void> {
  initializingOrUnInitializing = true;

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
    clipboardListener.on("change", async () => {
      // Prevents duplicated clipboard events
      const currentTime = Date.now();
      if (
        lastTimeClipboardChecked &&
        currentTime - lastTimeClipboardChecked < 500
      ) {
        return;
      }
      lastTimeClipboardChecked = currentTime;

      // Wait a bit so that clipboard is fully written
      await setTimeoutAsync(100);

      await writeClipboardToFile();
    });
  }

  if (
    config.get("receiveTexts", true) ||
    config.get("receiveImages", true) ||
    config.get("receiveFiles", true)
  ) {
    // Watches for files and reads clipboard from it
    clipboardFilesWatcher = chokidar
      .watch(syncFolder, {
        usePolling: config.get("usePolling", false),
        interval: 1000,
        binaryInterval: 1000,
        ignoreInitial: true,
        disableGlobbing: true,
        ignored: [
          // This filters out temporary files created by the OneDrive client, example:
          // "C:\Users\user\OneDrive\Clipboard Sync\1-my-pc.txt.json~RF1a1c3c.TMP"
          "**/*~*.TMP",
        ],
      })
      .on("add", async (filename) => {
        const parsedFile = parseClipboardFileName(
          filename,
          syncFolder,
          "from-others",
        );

        if (!parsedFile) {
          return;
        }

        // Wait a bit so that the file is fully written
        await setTimeoutAsync(200);

        await readClipboardFromFile(parsedFile);
      });

    // Create a file to indicate that this computer is receiving clipboards
    await fs.writeFile(path.join(syncFolder, hostNameIsReceivingFileName), "");
  }

  if (handleTasks) {
    if (config.get("autoCleanup", true)) {
      filesCleanerTask = cron.schedule(
        "*/1 * * * *",
        () => {
          cleanFiles(syncFolder);
        },
        {
          runOnInit: true,
        },
      );
    }

    idleDetectorTask = cron.schedule(
      "* * * * * *", // every second
      async () => {
        if (initializingOrUnInitializing) {
          return;
        }

        // Consider the system idle if it has been inactive for 15 minutes
        // TODO: revert to 15 minutes after testing
        const idleState = powerMonitor.getSystemIdleState(10);

        if (idleState === "unknown") {
          log.warn("System idle state is unknown");
          return;
        }

        if (idleState === "active") {
          if (initialized) {
            return;
          }
          log.info("System is active. Resuming...");
          await initialize(false);
          return;
        }

        if (initialized) {
          log.info("System is idle. Suspending...");
          await unInitialize(false);
        }
      },
    );
  }

  appIcon.setImage(getTrayIcon("working"));

  initialized = true;
  initializingOrUnInitializing = false;
}

async function unInitialize(handleTasks = true): Promise<void> {
  initializingOrUnInitializing = true;

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
    await clipboardFilesWatcher.close();
    clipboardFilesWatcher = null;
  }

  if (handleTasks) {
    if (filesCleanerTask) {
      filesCleanerTask.stop();
      filesCleanerTask = null;
    }

    if (idleDetectorTask) {
      idleDetectorTask.stop();
      idleDetectorTask = null;
    }
  }

  // TODO: change to suspend
  appIcon.setImage(getTrayIcon("received"));

  initialized = false;
  initializingOrUnInitializing = false;
}

async function reload(): Promise<void> {
  log.info("Reloading configuration...");
  await unInitialize();
  await initialize();
  if (process.platform === "linux") {
    setContextMenu();
  }
}

function getAppIcon(): string {
  const iconExtension =
    process.platform === "win32"
      ? "ico"
      : process.platform === "darwin"
        ? "icns"
        : "png";

  return path.resolve(
    app.getAppPath(),
    `resources/appicons/${iconExtension}/icon.${iconExtension}`,
  );
}

function getTrayIcon(icon: ClipboardIcon): string {
  const iconExtension = process.platform === "win32" ? "ico" : "png";

  return path.resolve(
    app.getAppPath(),
    `resources/trayicons/${iconExtension}/${icon}.${iconExtension}`,
  );
}

function setIconFor5Seconds(icon: ClipboardIcon): void {
  appIcon.setImage(getTrayIcon(icon));

  if (iconWaiter) {
    clearTimeout(iconWaiter);
  }
  iconWaiter = setTimeout(() => {
    appIcon.setImage(getTrayIcon("working"));
  }, 5000);
}

function handleCheckBoxClick(checkBox: Electron.MenuItem, key: string): void {
  config.set(key, checkBox.checked);
  reload();
}

let updateLabel = "Check for updates";

async function isUpdateAvailable(): Promise<
  | false
  | {
      newVersion: string;
      newVersionUrl: string;
    }
> {
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
}

async function checkForUpdatesPress(): Promise<void> {
  const update = await isUpdateAvailable();
  if (update) {
    new Notification({
      title: "Update available",
      body: "Opening download page...",
      icon: getAppIcon(),
    }).show();
    const baseUrl = "https://github.com/felipecrs/clipboard-sync/releases";
    if (process.platform === "win32") {
      shell.openExternal(
        `${baseUrl}/download/v${update.newVersion}/Clipboard.Sync-${update.newVersion}.Setup.exe`,
      );
    } else if (process.platform === "darwin") {
      shell.openExternal(
        `${baseUrl}/download/v${update.newVersion}/Clipboard.Sync-${update.newVersion}-x64.dmg`,
      );
    }
    shell.openExternal(baseUrl);
  } else {
    new Notification({
      title: "No updates found",
      body: "You are already running the latest version.",
      icon: getAppIcon(),
    }).show();
  }
}

async function autoCheckForUpdates(): Promise<void> {
  const update = await isUpdateAvailable();
  if (update) {
    new Notification({
      title: "Update available",
      body: "Click in the tray icon to download.",
      icon: getAppIcon(),
    }).show();
  }
}

async function restartOneDrive(): Promise<void> {
  const scriptPath = path.resolve(
    app.getAppPath(),
    "resources/scripts/Restart-OneDrive.ps1",
  );
  const result = await execa(
    "PowerShell.exe",
    ["-File", scriptPath, "-NoProfile", "-ExecutionPolicy", "Bypass"],
    {
      reject: false,
    },
  );

  if (result.failed) {
    log.error(`Error restarting OneDrive: ${result}`);
    dialog.showErrorBox("Error restarting OneDrive", result.toString());
  }
}

function setContextMenu(): void {
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
          click: (checkBox): void => handleCheckBoxClick(checkBox, "sendTexts"),
          toolTip: "Whether to enable sending copied texts or not",
        },
        {
          label: "Images",
          type: "checkbox",
          checked: config.get("sendImages", true),
          click: (checkBox): void =>
            handleCheckBoxClick(checkBox, "sendImages"),
          toolTip: "Whether to enable sending copied images or not",
        },
        {
          label: "Files",
          type: "checkbox",
          checked: config.get("sendFiles", true),
          click: (checkBox): void => handleCheckBoxClick(checkBox, "sendFiles"),
          toolTip: "Whether to enable sending copied files or not",
          visible: !!clipboardEx,
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
          click: (checkBox): void =>
            handleCheckBoxClick(checkBox, "receiveTexts"),
          toolTip: "Whether to enable receiving texts or not",
        },
        {
          label: "Images",
          type: "checkbox",
          checked: config.get("receiveImages", true),
          click: (checkBox): void =>
            handleCheckBoxClick(checkBox, "receiveImages"),
          toolTip: "Whether to enable receiving images or not",
        },
        {
          label: "Files",
          type: "checkbox",
          checked: config.get("receiveFiles", true),
          click: (checkBox): void =>
            handleCheckBoxClick(checkBox, "receiveFiles"),
          toolTip: "Whether to enable receiving files or not",
          visible: !!clipboardEx,
        },
      ],
    },
    { type: "separator" },
    {
      label: "Use polling",
      type: "checkbox",
      checked: config.get("usePolling", false),
      click: (checkBox): void => handleCheckBoxClick(checkBox, "usePolling"),
      toolTip: `Try enabling this option if ${app.name} is not receiving clipboards, usually on network drives`,
    },
    {
      label: "Auto-clean",
      type: "checkbox",
      checked: config.get("autoCleanup", true),
      click: (checkBox): void => handleCheckBoxClick(checkBox, "autoCleanup"),
      toolTip: `Auto-clean the files created by ${app.name}`,
    },
    {
      label: "Auto-start on login",
      type: "checkbox",
      checked: app.getLoginItemSettings().openAtLogin,
      click: (checkBox: Electron.MenuItem): void => {
        app.setLoginItemSettings({
          openAtLogin: checkBox.checked,
        });
      },
      visible: process.platform !== "linux",
    },
    { type: "separator" },
    { label: "Change folder", type: "normal", click: askForFolder },
    {
      label: "Open folder",
      type: "normal",
      click: (): void => {
        shell.openPath(syncFolder);
      },
    },
    { type: "separator", visible: process.platform === "win32" },
    {
      label: "Restart OneDrive",
      type: "normal",
      visible: process.platform === "win32",
      click: restartOneDrive,
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
      click: (): void => {
        shell.openExternal("https://github.com/felipecrs/clipboard-sync");
      },
      toolTip:
        "Open the GitHub page of the project. Please star it if you like it!",
    },
    { type: "separator" },
    {
      label: "Exit",
      type: "normal",
      click: (): void => quit(),
    },
  ]);
  appIcon.setContextMenu(menu);
}

async function createAppIcon(): Promise<void> {
  // guid only works on Windows 11+
  // https://github.com/electron/electron/issues/41773
  if (
    os.platform() === "win32" &&
    semverGreaterThanOrEqual(os.release(), "10.0.22000")
  ) {
    appIcon = new Tray(
      getTrayIcon("working"),
      // This GUID should not be changed. It ensures the tray icon position is kept between app updates.
      "72812af2-6bcc-40d9-b35d-0b43e72ac346",
    );
  } else {
    appIcon = new Tray(getTrayIcon("working"));
  }
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
}

// This method will be called when Electron has finished
// initialization and is ready to create browser windows.
// Some APIs can only be used after this event occurs.
app.on("ready", createAppIcon);

let cleanupBeforeQuitDone = false;
async function cleanupBeforeQuit(): Promise<void> {
  if (cleanupBeforeQuitDone) {
    return;
  }
  await unInitialize();
  cleanupBeforeQuitDone = true;
}

function quit(exitCode: number = 0): void {
  if (exitCode === 0) {
    app.quit();
  } else {
    cleanupBeforeQuit();
    app.exit(exitCode);
  }
}

app.on("before-quit", cleanupBeforeQuit);
