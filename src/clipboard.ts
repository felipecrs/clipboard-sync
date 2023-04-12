import * as fs from "node:fs";
import * as path from "node:path";
import * as fswin from "fswin";

import { hostname } from "./global";
import { deleteFolderRecursive, iterateThroughFilesRecursively } from "./utils";

// returns 0 if not valid
export const getItemNumber = (
  file: string,
  filter: "none" | "from-others" | "from-myself" = "none"
) => {
  const parsedFile = path.parse(file);
  let itemNumber = 0;
  let fileStat;

  try {
    fileStat = fs.lstatSync(file);
  } catch (error) {
    return itemNumber;
  }

  if (fileStat.isDirectory()) {
    const match = parsedFile.base.match(
      /^(0|[1-9][0-9]*)-([0-9a-zA-Z-]+)\.(0|[1-9][0-9]*)_files$/
    );
    if (match) {
      switch (filter) {
        case "from-myself":
          if (match[2] === hostname) {
            itemNumber = parseInt(match[1]);
          }
          break;
        case "from-others":
          if (match[2] !== hostname) {
            itemNumber = parseInt(match[1]);
          }
          break;
        default:
          itemNumber = parseInt(match[1]);
          break;
      }
    }
  } else {
    const match = parsedFile.base.match(
      /^(0|[1-9][0-9]*)-([0-9a-zA-Z-]+)\.(txt|png)$/
    );
    if (match) {
      switch (filter) {
        case "from-myself":
          if (match[2] === hostname) {
            itemNumber = parseInt(match[1]);
          }
          break;
        case "from-others":
          if (match[2] !== hostname) {
            itemNumber = parseInt(match[1]);
          }
          break;
        default:
          itemNumber = parseInt(match[1]);
          break;
      }
    }
  }
  return itemNumber;
};

export const getNextWriteTime = (syncFolder: string) => {
  const numbers: number[] = [];
  fs.readdirSync(syncFolder).forEach((file) => {
    file = path.join(syncFolder, file);
    const itemNumber = getItemNumber(file);
    if (itemNumber) {
      numbers.push(itemNumber);
    }
  });
  if (numbers.length > 0) {
    // https://stackoverflow.com/a/1063027/12156188
    return (
      numbers
        .sort((a, b) => {
          return a - b;
        })
        .at(-1) + 1
    );
  }
  return 1;
};

export const isThereMoreThanOneClipboardFile = (syncFolder: string) => {
  let found = 0;
  fs.readdirSync(syncFolder).forEach((file) => {
    file = path.join(syncFolder, file);
    const itemNumber = getItemNumber(file);
    if (itemNumber) {
      found++;
      if (found > 1) {
        return;
      }
    }
  });
  return found > 1;
};

// Removes from-myself files older than 5 minutes,
// Unsyncs from-others files older than 1 minute,
// And removes from-others files older than 10 minutes.
export const cleanFiles = (syncFolder: string) => {
  const now = Date.now();
  const currentTimeMinus1Min = now - 60000;
  const currentTimeMinus5Min = now - 300000;
  const currentTimeMinus10Min = now - 600000;
  fs.readdirSync(syncFolder).forEach((file) => {
    const filePath = path.join(syncFolder, file);
    if (getItemNumber(filePath, "from-myself")) {
      const fileStat = fs.statSync(filePath);
      if (fileStat.ctime.getTime() <= currentTimeMinus5Min) {
        if (fileStat.isDirectory()) {
          deleteFolderRecursive(filePath);
        } else {
          fs.unlinkSync(filePath);
        }
      }
    } else if (getItemNumber(filePath, "from-others")) {
      const fileStat = fs.statSync(filePath);
      if (fileStat.ctime.getTime() <= currentTimeMinus1Min) {
        if (process.platform === "win32") {
          unsyncFileOrFolderRecursively(filePath);
        }
      } else if (fileStat.ctime.getTime() <= currentTimeMinus10Min) {
        if (fileStat.isDirectory()) {
          deleteFolderRecursive(filePath);
        } else {
          fs.unlinkSync(filePath);
        }
      }
    }
  });
};

export const unsyncFileOrFolderRecursively = (fileOrFolder: string) => {
  iterateThroughFilesRecursively([fileOrFolder], (file) => {
    fswin.setAttributesSync(file, {
      IS_UNPINNED: true,
      IS_PINNED: false,
    });
  });
};
