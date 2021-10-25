import {
  app,
  Tray,
  Menu,
  dialog,
  shell,
  Notification,
  clipboard,
  nativeImage,
} from "electron";
import { createHash } from "crypto";
import { https } from "follow-redirects";
import Store = require("electron-store");
import chokidar = require("chokidar");
import cron = require("node-cron");
import path = require("path");
import fs = require("fs");
import semver = require("semver");

// Handle creating/removing shortcuts on Windows when installing/uninstalling.
// eslint-disable-line global-require
if (require("electron-squirrel-startup")) {
  app.exit();
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

type ClipboardType = "text" | "image" | "none";

const config = new Store<ConfigType>({
  defaults: {
    send: true,
    receive: true,
    autoCleanup: true,
  },
});

let appIcon: Tray = null;

let syncFolder: string = null;

let lastTextWritten: string = null;
let lastImageSha256Written: string = null;
let lastTimeWritten: string = null;

let lastTextRead: string = null;
let lastImageSha256Read: string = null;
let lastTimeRead: string = null;

let clipboardListener: ClipboardListener = null;
let clipboardFilesWatcher: chokidar.FSWatcher = null;
let filesCleanerTask: cron.ScheduledTask = null;
let iconWaiter: NodeJS.Timeout = null;

const isValidFile = (file: string) => {
  const parsedFile = path.parse(file);
  if (
    (parsedFile.ext !== ".txt" && parsedFile.ext !== ".png") ||
    !fs.lstatSync(file).isFile() ||
    !(new Date(parseInt(parsedFile.name)).getTime() > 0)
  ) {
    return false;
  }
  return true;
};

const calculateSha256 = (data: Buffer) => {
  return createHash("sha256").update(data).digest("hex");
};

const writeClipboardToFile = () => {
  let clipboardType: ClipboardType;
  let clipboardText: string;
  let clipboardImage: Buffer;
  let clipboardImageSha256: string;
  const clipboardFormats = clipboard.availableFormats();

  try {
    if (clipboardFormats.includes("text/plain")) {
      clipboardText = clipboard.readText();
      clipboardType = "text";
    } else if (clipboardFormats.includes("image/png")) {
      clipboardImage = clipboard.readImage().toPNG();
      clipboardImageSha256 = calculateSha256(clipboardImage);
      clipboardType = "image";
    }
  } catch (error) {
    console.error("Error reading current clipboard");
  }

  if (clipboardType === "none") {
    return;
  }

  if (
    clipboardType === "text" &&
    (!clipboardText ||
      lastTextRead === clipboardText ||
      lastTextWritten === clipboardText)
  ) {
    return;
  }

  if (
    clipboardType === "image" &&
    (!clipboardImage ||
      lastImageSha256Read === clipboardImageSha256 ||
      lastImageSha256Written === clipboardImageSha256)
  ) {
    return;
  }

  const writeTime = `${Date.now()}`;
  let filePath: string;
  if (clipboardType === "text") {
    filePath = path.join(syncFolder, `${writeTime}.txt`);
    fs.writeFileSync(filePath, clipboardText, {
      encoding: "utf8",
    });
    lastTextWritten = clipboardText;
  } else if (clipboardType === "image") {
    filePath = path.join(syncFolder, `${writeTime}.png`);
    fs.writeFileSync(filePath, clipboardImage);
    lastImageSha256Written = clipboardImageSha256;
  }
  console.log(`Clipboard written to ${filePath}`);
  lastTimeWritten = writeTime;

  setIconFor5Seconds("clipboard_sent");
};

const readClipboardFromFile = (file: string) => {
  if (!isValidFile(file)) {
    return;
  }

  const fileName = path.parse(file).name;
  const fileExtension = path.parse(file).ext;
  const fileClipboardType =
    fileExtension === ".txt"
      ? "text"
      : fileExtension === ".png"
      ? "image"
      : null;

  let currentText: string;
  let currentImage: Buffer;
  let currentClipboardType: ClipboardType;
  let currentImageSha256: string;

  const clipboardFormats = clipboard.availableFormats();
  try {
    if (clipboardFormats.includes("text/plain")) {
      currentText = clipboard.readText();
      currentClipboardType = "text";
    } else if (clipboardFormats.includes("image/png")) {
      currentImage = clipboard.readImage().toPNG();
      currentImageSha256 = calculateSha256(currentImage);
      currentClipboardType = "image";
    }
  } catch (error) {
    console.error("Error reading current clipboard");
  }

  let newText: string;
  let newImage: Buffer;
  let newImageSha256: string;
  try {
    if (fileClipboardType === "text") {
      newText = fs.readFileSync(file, {
        encoding: "utf8",
      });
    } else if (fileClipboardType === "image") {
      newImage = fs.readFileSync(file);
      newImageSha256 = calculateSha256(newImage);
    }
  } catch (error) {
    console.error(`Error reading clipboard from file ${fileName}`);
    return;
  }

  if (fileClipboardType === "text") {
    // Prevents writing duplicated text to clipboard
    if (!newText || currentText === newText) {
      return;
    }
  } else if (fileClipboardType === "image") {
    // Prevents writing duplicated image to clipboard
    if (!newImage || currentImageSha256 === newImageSha256) {
      return;
    }
  }

  const currentFileTime = fileName;
  // Skips the read if a newer file was already wrote
  if (lastTimeWritten && currentFileTime <= lastTimeWritten) {
    return;
  }

  // Skips if a newer file was already read
  if (lastTimeRead && currentFileTime <= lastTimeRead) {
    return;
  }

  if (fileClipboardType === "text") {
    clipboard.writeText(newText);
    lastTextRead = newText;
  } else if (fileClipboardType === "image") {
    clipboard.writeImage(nativeImage.createFromBuffer(newImage));
    lastImageSha256Read = newImageSha256;
  }
  console.log(`Clipboard was read from ${file}`);
  lastTimeRead = currentFileTime;

  setIconFor5Seconds("clipboard_received");
};

const cleanFiles = () => {
  const currentTimeMinus5Min = `${Date.now() - 300000}`;
  fs.readdirSync(syncFolder).forEach((file) => {
    const filePath = path.join(syncFolder, file);
    const fileName = path.parse(file).name;
    if (isValidFile(filePath) && fileName <= currentTimeMinus5Min) {
      fs.unlinkSync(filePath);
    }
  });
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

const initialize = () => {
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
    clipboardFilesWatcher = chokidar
      .watch(syncFolder, {
        ignoreInitial: true,
        disableGlobbing: true,
        depth: 1,
      })
      .on("add", readClipboardFromFile);
  }

  if (config.get("autoCleanup", true)) {
    // Remove files older than 5 minutes
    cleanFiles();
    filesCleanerTask = cron.schedule("*/5 * * * *", cleanFiles, {
      scheduled: true,
    });
  }
};

const cleanup = () => {
  if (clipboardListener) {
    clipboardListener.stopListening();
    clipboardListener = null;
  }

  if (clipboardFilesWatcher) {
    clipboardFilesWatcher.close();
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

const finish = (exitCode?: number) => {
  cleanup();
  app.exit(exitCode !== undefined ? exitCode : 0);
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

const createAppIcon = () => {
  appIcon = new Tray(getTrayIcon("clipboard"));
  const contextMenu = Menu.buildFromTemplate([
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
      click: (checkBox) => {
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
      label: "Check for updates",
      type: "normal",
      click: checkForUpdates,
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
  appIcon.setToolTip(`${app.name} v${app.getVersion()}`);
  appIcon.setContextMenu(contextMenu);

  // sets left click to open the context menu too
  appIcon.on("click", () => {
    appIcon.popUpContextMenu();
  });

  initialize();
};

const checkForUpdates = () => {
  const request = https.request(
    {
      hostname: "github.com",
      path: "/felipecrs/clipboard-sync/releases/latest",
    },
    (response) => {
      const redirectedUrl = response.responseUrl;
      const latestVersion = redirectedUrl.split("/").pop().replace(/^v/, "");
      const currentVersion = app.getVersion();
      if (semver.gt(latestVersion, currentVersion)) {
        new Notification({
          title: "Update available",
          body: "Opening download page...",
          icon: getAppIcon(),
        }).show();
        if (process.platform === "win32") {
          shell.openExternal(
            `https://github.com/felipecrs/clipboard-sync/releases/download/v${latestVersion}/Clipboard.Sync-${latestVersion}.Setup.exe`
          );
        } else {
          shell.openExternal(redirectedUrl);
        }
      } else {
        new Notification({
          title: "No updates found",
          body: "You are running the latest version.",
          icon: getAppIcon(),
        }).show();
      }
    }
  );
  request.end();
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
