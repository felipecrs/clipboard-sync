const { app, Tray, Menu, dialog, shell } = require("electron");
const Store = require("electron-store");
const clipboard = require("clipboardy");
const chokidar = require("chokidar");
const cron = require("node-cron");
const path = require("path");
const fs = require("fs");

// Handle creating/removing shortcuts on Windows when installing/uninstalling.
// eslint-disable-line global-require
if (require("electron-squirrel-startup")) {
  app.exit();
}

let appIcon = null;
let store = new Store();

let syncFolder = null;

let lastTextWritten = "";
let lastTimeWritten = "";

let lastTextRead = "";
let lastTimeRead = "";

let clipboardListener = null;
let clipboardFilesWatcher = null;
let filesCleanerTask = null;

let iconWaiter = null;

const isValidFile = (file) => {
  const parsedFile = path.parse(file);
  if (
    parsedFile.ext !== ".txt" ||
    !fs.lstatSync(file).isFile() ||
    !new Date(parseInt(parsedFile.name)).getTime() > 0
  ) {
    return false;
  }
  return true;
};

const writeClipboardToFile = () => {
  let textToWrite = "";
  try {
    textToWrite = clipboard.readSync();
  } catch (error) {
    console.error("Error reading current clipboard");
    return;
  }
  if (
    !textToWrite ||
    lastTextRead === textToWrite ||
    lastTextWritten === textToWrite
  ) {
    return;
  }

  const writeTime = `${Date.now()}`;
  const filePath = path.join(syncFolder, `${writeTime}.txt`);

  lastTimeWritten = writeTime;
  lastTextWritten = textToWrite;

  console.log(`Writing clipboard to ${filePath}`);
  fs.writeFileSync(filePath, textToWrite, {
    encoding: "utf8",
  });

  setIconFor5Seconds("clipboard_sent");
};

const readClipboardFromFile = (filePath) => {
  if (!isValidFile(filePath)) {
    return;
  }

  const fileName = path.parse(filePath).name;

  let currentText = "";
  try {
    currentText = clipboard.readSync();
  } catch (error) {
    currentText = "";
  }

  let newText = "";
  try {
    newText = fs.readFileSync(filePath, {
      encoding: "utf8",
    });
  } catch (error) {
    console.error(`Error reading text from file ${fileName}`);
    return;
  }

  // Prevents writing duplicated text to clipboard
  if (!newText || currentText === newText) {
    return;
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

  lastTextRead = newText;
  lastTimeRead = currentFileTime;

  console.log(`Reading clipboard from ${filePath}`);
  clipboard.writeSync(newText);

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
  const previousFolder = store.get("folder");

  let folderSelected = dialog.showOpenDialogSync({
    title: "Select folder to save and read clipboard files",
    properties: ["openDirectory"],
    defaultPath: previousFolder,
  });

  if (folderSelected) {
    folderSelected = folderSelected[0];
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
  store.set("folder", folderSelected);

  if (folderSelected !== previousFolder) {
    reload();
  }
};

const initialize = () => {
  syncFolder = store.get("folder");

  if (
    !syncFolder ||
    (fs.existsSync(syncFolder) && !fs.lstatSync(syncFolder).isDirectory())
  ) {
    askForFolder();
  }

  if (!fs.existsSync(syncFolder)) {
    fs.mkdirSync(syncFolder);
  }

  if (store.get("send", true)) {
    clipboardListener = require("clipboard-event");
    clipboardListener.startListening();
    clipboardListener.on("change", writeClipboardToFile);
  }

  if (store.get("receive", true)) {
    // Watches for files and reads clipboard from it
    clipboardFilesWatcher = chokidar
      .watch(syncFolder, {
        ignoreInitial: true,
        disableGlobbing: true,
        depth: 1,
      })
      .on("add", readClipboardFromFile);
  }

  if (store.get("autoCleanup", true)) {
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

const finish = (exitCode = 0) => {
  cleanup();
  app.exit(exitCode);
};

const getTrayIcon = (icon) => {
  const iconExtension = process.platform === "win32" ? "ico" : "png";

  return path.resolve(
    __dirname,
    `../assets/trayicons/${iconExtension}/${icon}.${iconExtension}`
  );
};

const setIconFor5Seconds = (icon) => {
  appIcon.setImage(getTrayIcon(icon));

  if (iconWaiter) {
    clearTimeout(iconWaiter);
  }
  iconWaiter = setTimeout(() => {
    appIcon.setImage(getTrayIcon("clipboard"));
  }, 5000);
};

const handleSendCheckBox = (checkBox) => {
  store.set("send", checkBox.checked);
  reload();
};

const handleReceiveCheckBox = (checkBox) => {
  store.set("receive", checkBox.checked);
  reload();
};

const handleCleanupCheckBox = (checkBox) => {
  store.set("autoCleanup", checkBox.checked);
  reload();
};

const createAppIcon = () => {
  appIcon = new Tray(getTrayIcon("clipboard"));
  const contextMenu = Menu.buildFromTemplate([
    {
      label: "Send",
      type: "checkbox",
      checked: store.get("send", true),
      click: handleSendCheckBox,
    },
    {
      label: "Receive",
      type: "checkbox",
      checked: store.get("receive", true),
      click: handleReceiveCheckBox,
    },
    {
      label: "Auto-clean",
      type: "checkbox",
      checked: store.get("autoCleanup", true),
      click: handleCleanupCheckBox,
    },
    { type: "separator" },
    {
      label: "Auto-start",
      type: "checkbox",
      checked: app.getLoginItemSettings().openAtLogin,
      click: (checkBox) => {
        app.setLoginItemSettings({
          openAtLogin: checkBox.checked,
        });
      },
    },
    { label: "Change folder", type: "normal", click: askForFolder },
    {
      label: "Open folder",
      type: "normal",
      click: () => {
        shell.openPath(syncFolder);
      },
    },
    { type: "separator" },
    { label: "Exit", type: "normal", click: finish },
  ]);
  appIcon.setToolTip("Clipboard Sync");
  appIcon.setContextMenu(contextMenu);

  initialize();
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
