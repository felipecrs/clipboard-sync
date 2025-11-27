import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { setTimeout as setTimeoutAsync } from "node:timers/promises";

import {
  FSWatcher as ChokidarFSWatcher,
  watch as chokidarWatch,
} from "chokidar";
import {
  readClipboardFilePaths,
  writeClipboardFilePaths,
} from "clip-filepaths";
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
import {
  gt as semverGreaterThan,
  gte as semverGreaterThanOrEqual,
} from "semver";

import type { ClipboardEventListener } from "clipboard-event";

import {
  ClipboardText,
  ClipboardType,
  ParsedClipboardFileName,
  cleanFiles,
  isClipboardTextEmpty,
  isClipboardTextEquals,
  noComputersReceiving,
  parseClipboardFileName,
} from "./clipboard.js";
import { hostName, hostNameIsReceivingFileName } from "./global.js";
import {
  calculateSha256,
  copyFolderRecursive,
  getFilesSizeInMb,
  getRedirectedUrl,
  getTotalNumberOfFiles,
  arraysEqual,
} from "./utilities.js";

// Handle creating/removing shortcuts on Windows when installing/uninstalling.
{
  const { default: squirrelStartup } =
    await import("electron-squirrel-startup");
  if (squirrelStartup) {
    console.error("Squirrel event handled. Quitting...");
    app.exit();
  }
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
  watchMode: "native" | "polling" | "pollingHarder";
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
    watchMode: "native",
  },
});

let appIcon: Tray;

let syncFolder: string;

let lastBeat: number;

let lastTextRead: ClipboardText;
let lastImageSha256Read: string;
let lastClipboardFilePathsRead: string[];

let lastTextWritten: ClipboardText;
let lastImageSha256Written: string;

let initialized: boolean = false;
let initializingOrUnInitializing: boolean = false;
let clipboardListener: ClipboardEventListener;
let clipboardFilesWatcher: ChokidarFSWatcher | cron.ScheduledTask;
let keepAliveTask: cron.ScheduledTask;
let filesCleanerTask: cron.ScheduledTask;
let idleDetectorTask: cron.ScheduledTask;
let iconWaiter: NodeJS.Timeout;

let lastClipboardEvent: number;

async function writeClipboardToFile(): Promise<void> {
  const beat = Date.now();

  // Avoids sending the clipboard if there is no other computer receiving
  if (await noComputersReceiving(syncFolder, beat)) {
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
    if (clipboardFormats.includes("text/uri-list")) {
      if (!config.get("sendFiles", true)) {
        return;
      }
      clipboardFilePaths = readClipboardFilePaths().filePaths;
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
  const recent = lastBeat && beat - lastBeat < 15_000;

  switch (clipboardType) {
    case "text": {
      if (isClipboardTextEmpty(clipboardText)) {
        return;
      }
      if (
        recent &&
        (isClipboardTextEquals(lastTextRead, clipboardText) ||
          isClipboardTextEquals(lastTextWritten, clipboardText))
      ) {
        return;
      }

      break;
    }
    case "image": {
      if (!clipboardImage) {
        return;
      }
      if (
        recent &&
        (lastImageSha256Read === clipboardImageSha256 ||
          lastImageSha256Written === clipboardImageSha256)
      ) {
        return;
      }

      break;
    }
    case "files": {
      if (!clipboardFilePaths) {
        return;
      }
      if (
        recent &&
        arraysEqual(lastClipboardFilePathsRead, clipboardFilePaths)
      ) {
        return;
      }
      const size = await getFilesSizeInMb(clipboardFilePaths);
      if (size > 100) {
        log.warn(
          `Not sending cliboard files as ${size}MB is bigger than 100MB`,
        );
        return;
      }

      break;
    }
  }

  lastBeat = beat;

  let destinationPath: string;
  switch (clipboardType) {
    case "text": {
      destinationPath = path.join(syncFolder, `${beat}-${hostName}.text.json`);
      await fs.writeFile(
        destinationPath,
        JSON.stringify(clipboardText, undefined, 2),
        { encoding: "utf8" },
      );
      lastTextWritten = clipboardText;

      break;
    }
    case "image": {
      destinationPath = path.join(syncFolder, `${beat}-${hostName}.png`);
      await fs.writeFile(destinationPath, clipboardImage);
      lastImageSha256Written = clipboardImageSha256;

      break;
    }
    case "files": {
      filesCount = await getTotalNumberOfFiles(clipboardFilePaths);
      destinationPath = path.join(
        syncFolder,
        `${beat}-${hostName}.${filesCount}_files`,
      );
      await fs.mkdir(destinationPath);
      for (const filePath of clipboardFilePaths) {
        const fullDestination = path.join(
          destinationPath,
          path.basename(filePath),
        );
        const stat = await fs.stat(filePath);
        stat.isDirectory()
          ? copyFolderRecursive(filePath, fullDestination)
          : fs.copyFile(filePath, fullDestination);
      }

      break;
    }
  }
  log.info(`Clipboard written to ${destinationPath}`);

  setIconFor5Seconds("sent");
}

async function readClipboardFromFile(
  parsedFile: ParsedClipboardFileName,
): Promise<void> {
  const beat = Date.now();

  const file = parsedFile.file;
  const fileBeat = parsedFile.beat;
  const fileClipboardType = parsedFile.clipboardType;

  let newText: ClipboardText;
  let newImage: Buffer;
  let newImageSha256: string;
  let newFilePaths: string[];
  let newFilesCount: number;
  try {
    switch (fileClipboardType) {
      case "text": {
        if (!config.get("receiveTexts", true)) {
          return;
        }
        newText = JSON.parse(await fs.readFile(file, { encoding: "utf8" }));

        break;
      }
      case "image": {
        if (!config.get("receiveImages", true)) {
          return;
        }
        newImage = await fs.readFile(file);
        newImageSha256 = calculateSha256(newImage);

        break;
      }
      case "files": {
        if (!config.get("receiveFiles", true)) {
          return;
        }
        newFilesCount = parsedFile.filesCount;

        if (!newFilesCount) {
          // This should not happen, but just in case
          log.warn(
            `Could not read the number of files in ${file}. Skipping...`,
          );
          return;
        }

        const filesCountInFolder = await getTotalNumberOfFiles([file]);
        if (newFilesCount !== filesCountInFolder) {
          log.info(
            `Not all files are yet present in _files folder. Current: ${filesCountInFolder}, expected: ${newFilesCount}. Skipping...`,
          );
          // lastFileNumberRead is not set here to allow reading this clipboard
          // again when more files are present
          return;
        }

        const directoryMembers = await fs.readdir(file);
        newFilePaths = directoryMembers.map((fileName: string) =>
          path.join(file, fileName),
        );

        break;
      }
      // No default
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
    if (clipboardFormats.includes("text/uri-list")) {
      currentFilePaths = readClipboardFilePaths().filePaths;
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
    switch (currentClipboardType) {
      case "text": {
        if (
          isClipboardTextEmpty(newText) ||
          isClipboardTextEquals(currentText, newText)
        ) {
          return;
        }

        break;
      }
      case "image": {
        if (!newImage || currentImageSha256 === newImageSha256) {
          return;
        }

        break;
      }
      case "files": {
        if (!newFilePaths || arraysEqual(currentFilePaths, newFilePaths)) {
          return;
        }

        break;
      }
    }
  }

  // Skips the read if a newer clipboard was already processed, which can
  // happen if OneDrive takes too long to sync
  if (lastBeat && fileBeat < lastBeat) {
    log.info(
      `Skipping reading clipboard from ${file} as a newer clipboard was already processed`,
    );
    return;
  }

  lastBeat = beat;

  switch (fileClipboardType) {
    case "text": {
      clipboard.write(newText);
      lastTextRead = newText;
      break;
    }
    case "image": {
      clipboard.writeImage(nativeImage.createFromBuffer(newImage));
      lastImageSha256Read = newImageSha256;
      break;
    }
    case "files": {
      writeClipboardFilePaths(newFilePaths);
      lastClipboardFilePathsRead = newFilePaths;
      break;
    }
  }
  log.info(`Clipboard was read from ${file}`);

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

async function initialize(fromSuspension = false): Promise<void> {
  initializingOrUnInitializing = true;

  syncFolder = config.get("folder");

  if (!(typeof syncFolder === "string" || syncFolder === undefined)) {
    return;
  }

  if (syncFolder) {
    try {
      const stat = await fs.lstat(syncFolder);
      if (!stat.isDirectory()) {
        askForFolder();
      }
    } catch (error) {
      if (error.code === "ENOENT") {
        askForFolder();
      } else {
        throw error;
      }
    }
  } else {
    askForFolder();
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
    const { default: clipboardEvent } = await import("clipboard-event");
    clipboardListener = clipboardEvent;
    clipboardListener.startListening();
    clipboardListener.on("change", async () => {
      // Prevents duplicated clipboard events
      const now = Date.now();
      if (lastClipboardEvent && now - lastClipboardEvent < 500) {
        return;
      }
      lastClipboardEvent = now;

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
    const watchMode = config.get("watchMode");
    log.info(`Watch mode: ${watchMode}`);
    if (watchMode === "pollingHarder") {
      clipboardFilesWatcher = cron.schedule(
        "*/2 * * * * *", // every 2 seconds
        async () => {
          const beat = Date.now();
          const files = await fs.readdir(syncFolder);
          const clipboardFiles: ParsedClipboardFileName[] = [];
          for (const file of files) {
            const parsedFile = parseClipboardFileName(
              path.join(syncFolder, file),
              syncFolder,
              "from-others",
            );
            if (parsedFile) {
              clipboardFiles.push(parsedFile);
            }
          }

          // Only process the most recent clipboard file
          clipboardFiles.sort((a, b) => a.beat - b.beat);
          const file = clipboardFiles.pop();
          if (!file) {
            return;
          }

          // Avoids reading existing files when first starting
          if (beat - file.beat > 15_000) {
            return;
          }

          // Keeping this logic here instead of inside readClipboardFromFile
          // since this situation can only happen in this watching mode
          if (lastBeat && file.beat <= lastBeat) {
            return;
          }

          await readClipboardFromFile(file);
        },
      );
    } else {
      clipboardFilesWatcher = chokidarWatch(syncFolder, {
        usePolling: watchMode === "polling",
        interval: 1000,
        binaryInterval: 1000,
        ignoreInitial: true,
        // This filters out temporary files created by the OneDrive client, examples:
        // "C:\Users\user\OneDrive\Clipboard Sync\1-my-pc.txt.json~RF1a1c3c.TMP"
        // "C:\Users\user\OneDrive\Clipboard Sync\1-my-pc.txt.json~RF1495807e.TMP"
        ignored: (filename) => /~RF[0-9a-f]+\.TMP$/.test(filename),
      }).on("add", async (filename) => {
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
    }

    keepAliveTask = cron.schedule(
      // every 4 minutes
      "*/4 * * * *",
      async () => {
        if (!syncFolder) {
          return;
        }
        // Create a file to indicate that this computer is receiving clipboards
        await fs.writeFile(
          path.join(syncFolder, hostNameIsReceivingFileName),
          `${Date.now()}`,
        );
      },
    );
    keepAliveTask.execute();
  }

  if (!fromSuspension) {
    if (config.get("autoCleanup", true)) {
      filesCleanerTask = cron.schedule("*/1 * * * *", async () => {
        await cleanFiles(syncFolder);
      });
      filesCleanerTask.execute();
    }

    idleDetectorTask = cron.schedule(
      "* * * * * *", // every second
      async () => {
        if (initializingOrUnInitializing) {
          return;
        }

        // Consider the system idle if it has been inactive for 15 minutes
        const idleState = powerMonitor.getSystemIdleState(900);

        if (idleState === "unknown") {
          log.warn("System idle state is unknown");
          return;
        }

        if (idleState === "active") {
          if (initialized) {
            return;
          }
          log.info("System is active. Resuming...");
          await initialize(true);
          return;
        }

        if (initialized) {
          log.info("System is idle. Suspending...");
          await unInitialize(true);
        }
      },
    );
  }

  appIcon.setImage(getTrayIcon("working"));

  initialized = true;
  initializingOrUnInitializing = false;
}

async function unInitialize(fromSuspension = false): Promise<void> {
  initializingOrUnInitializing = true;

  if (keepAliveTask) {
    keepAliveTask.stop();
    keepAliveTask = undefined;

    // Deletes the file that indicates that this computer is receiving clipboards
    if (syncFolder) {
      await fs.rm(path.join(syncFolder, hostNameIsReceivingFileName), {
        force: true,
      });
    }
  }

  if (clipboardListener) {
    clipboardListener.stopListening();
    clipboardListener = undefined;
  }

  if (clipboardFilesWatcher) {
    if (clipboardFilesWatcher instanceof ChokidarFSWatcher) {
      await clipboardFilesWatcher.close();
    } else {
      clipboardFilesWatcher.stop();
    }
    clipboardFilesWatcher = undefined;
  }

  if (!fromSuspension) {
    if (filesCleanerTask) {
      filesCleanerTask.stop();
      filesCleanerTask = undefined;
    }

    if (idleDetectorTask) {
      idleDetectorTask.stop();
      idleDetectorTask = undefined;
    }
  }

  appIcon.setImage(getTrayIcon("suspended"));

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

function handleRadioClick(key: string, value: string): void {
  config.set(key, value);
  reload();
}

let updateLabel = "Check for updates";

async function isUpdateAvailable(): Promise<
  false | { newVersion: string; newVersionUrl: string }
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

  // Development version should not attempt to update
  if (currentVersion === "0.0.0-development") {
    return false;
  }

  if (semverGreaterThan(newVersion, currentVersion)) {
    updateLabel = "Download update";
    setContextMenu();
    return { newVersion, newVersionUrl };
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
    ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", scriptPath],
    { reject: false, all: true },
  );

  log.info(result.all);

  if (result.failed) {
    log.error(`Error restarting OneDrive: ${result.shortMessage}`);
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
          checked: config.get("sendTexts"),
          click: (checkBox): void => handleCheckBoxClick(checkBox, "sendTexts"),
          toolTip: "Whether to enable sending copied texts or not",
        },
        {
          label: "Images",
          type: "checkbox",
          checked: config.get("sendImages"),
          click: (checkBox): void =>
            handleCheckBoxClick(checkBox, "sendImages"),
          toolTip: "Whether to enable sending copied images or not",
        },
        {
          label: "Files",
          type: "checkbox",
          checked: config.get("sendFiles"),
          click: (checkBox): void => handleCheckBoxClick(checkBox, "sendFiles"),
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
          checked: config.get("receiveTexts"),
          click: (checkBox): void =>
            handleCheckBoxClick(checkBox, "receiveTexts"),
          toolTip: "Whether to enable receiving texts or not",
        },
        {
          label: "Images",
          type: "checkbox",
          checked: config.get("receiveImages"),
          click: (checkBox): void =>
            handleCheckBoxClick(checkBox, "receiveImages"),
          toolTip: "Whether to enable receiving images or not",
        },
        {
          label: "Files",
          type: "checkbox",
          checked: config.get("receiveFiles"),
          click: (checkBox): void =>
            handleCheckBoxClick(checkBox, "receiveFiles"),
          toolTip: "Whether to enable receiving files or not",
        },
      ],
    },
    { type: "separator" },
    {
      label: "Watch mode",
      type: "submenu",
      toolTip: "Select how to watch for clipboard files",
      submenu: [
        {
          label: "Native",
          type: "radio",
          checked: config.get("watchMode") === "native",
          click: (): void => handleRadioClick("watchMode", "native"),
          toolTip: `The default mode. This is the fastest and most efficient way to watch for clipboard files. Use it if it works.`,
        },
        {
          label: "Polling",
          type: "radio",
          checked: config.get("watchMode") === "polling",
          click: (): void => handleRadioClick("watchMode", "polling"),
          toolTip: `Try this if native mode is not receiving clipboards. It is slower and less efficient than native mode. Usually needed on network drives.`,
        },
        {
          label: "Polling harder",
          type: "radio",
          checked: config.get("watchMode") === "pollingHarder",
          click: (): void => handleRadioClick("watchMode", "pollingHarder"),
          toolTip: `Try this if polling mode is not receiving clipboards. It is the slowest and least efficient way to watch for clipboard files. Usually needed on WinFSP mounts.`,
        },
      ],
    },
    {
      label: "Auto-clean",
      type: "checkbox",
      checked: config.get("autoCleanup"),
      click: (checkBox): void => handleCheckBoxClick(checkBox, "autoCleanup"),
      toolTip: `Auto-clean the files created by ${app.name}`,
    },
    {
      label: "Auto-start on login",
      type: "checkbox",
      checked: app.getLoginItemSettings().openAtLogin,
      click: (checkBox: Electron.MenuItem): void => {
        app.setLoginItemSettings({ openAtLogin: checkBox.checked });
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
      visible: process.platform !== "linux",
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
    { label: "Exit", type: "normal", click: (): void => quit() },
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

  if (process.platform !== "linux") {
    await autoCheckForUpdates();
  }
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
