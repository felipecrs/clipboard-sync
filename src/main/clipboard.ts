import log from "electron-log";
import fs from "node:fs/promises";
import path from "node:path";
import { promisify } from "node:util";

import { hostName, isReceivingFileNameSuffix } from "./global.js";
import {
  deleteFileOrFolderRecursively,
  iterateThroughFilesRecursively,
} from "./utils.js";

import type {
  Attributes as FSWinAttributes,
  SetAttributes as FSWinSetAttributes,
} from "fswin";

let fswin: typeof import("fswin") | null = null;

if (process.platform === "win32") {
  fswin = require("fswin");
}

export type ClipboardType = "text" | "image" | "files";

export type ParsedClipboardFileName = {
  file: string;
  number: number;
  clipboardType: ClipboardType;
  from: "myself" | "others";
  filesCount?: number;
};

// This function always expects a file, not a folder
// returns null if not valid
export const parseClipboardFileName = (
  file: string,
  syncFolder: string,
  filter: "none" | "from-others" | "from-myself" = "none"
): ParsedClipboardFileName | null => {
  let number = 0;
  let clipboardType: ClipboardType;
  let from: "myself" | "others";
  let filesCount: number | undefined;

  // Get relative path to syncFolder
  const relativePath = path.relative(syncFolder, file);

  // Get only the first level of folders
  const baseFile = relativePath.split(path.sep)[0];

  const match = baseFile.match(
    /^([1-9][0-9]*)-([0-9a-zA-Z-]+)\.((text\.json)|png|([1-9][0-9]*)_files)$/
  );

  if (match) {
    if (match[5]) {
      clipboardType = "files";
      filesCount = parseInt(match[5]);
    } else {
      clipboardType = match[3] === "text.json" ? "text" : "image";
    }
    from = match[2] === hostName ? "myself" : "others";
    switch (filter) {
      case "from-myself":
        if (from === "myself") {
          number = parseInt(match[1]);
        }
        break;
      case "from-others":
        if (from === "others") {
          number = parseInt(match[1]);
        }
        break;
      default:
        number = parseInt(match[1]);
        break;
    }
  }

  if (number === 0) {
    return null;
  }

  return {
    file: path.join(syncFolder, baseFile),
    number,
    clipboardType,
    from,
    filesCount,
  };
};

export const getNextFileNumber = async (syncFolder: string) => {
  const numbers: number[] = [];
  const files = await fs.readdir(syncFolder);
  for (const file of files) {
    const filePath = path.join(syncFolder, file);
    const parsedFile = parseClipboardFileName(filePath, syncFolder);
    if (parsedFile) {
      numbers.push(parsedFile.number);
    }
  }
  if (numbers.length > 0) {
    return Math.max(...numbers) + 1;
  }
  return 1;
};

export const isThereMoreThanOneClipboardFile = async (
  syncFolder: string,
  filter: "none" | "from-others" | "from-myself" = "none"
) => {
  const files = await fs.readdir(syncFolder);
  for (const file of files) {
    if (
      parseClipboardFileName(path.join(syncFolder, file), syncFolder, filter)
    ) {
      return true;
    }
  }
  return false;
};

export const isIsReceivingFile = (file: string) => {
  return file.endsWith(isReceivingFileNameSuffix);
};

// Unsyncs from-others files older than 1 minute,
// Removes from-myself files older than 5 minutes,
// And removes from-others files older than 10 minutes.
export const cleanFiles = async (syncFolder: string) => {
  const now = Date.now();

  const files = await fs.readdir(syncFolder);
  for (const file of files) {
    const filePath = path.join(syncFolder, file);

    const parsedFile = parseClipboardFileName(filePath, syncFolder);

    if (!parsedFile) {
      // These files will be deleted at application finish.
      if (isIsReceivingFile(filePath)) {
        continue;
      }

      // Check if it's a file used by previous versions.
      const match = path
        .parse(filePath)
        .base.match(
          /^((0|[1-9][0-9]*)-[0-9a-zA-Z-]+\.txt)|(receiving-[0-9a-zA-Z-]+\.txt)|([0-9a-zA-Z-]+\.is-reading\.txt)$/
        );
      if (match) {
        log.info(`Deleting file used by previous versions: ${filePath}`);
        await deleteFileOrFolderRecursively(filePath);
      }
      continue;
    }

    // Delete from myself files older than 5 minutes and
    // from others older than 10 minutes
    const timeThreshold =
      parsedFile.from === "myself" ? now - 300000 : now - 600000;

    let fileStat;
    try {
      fileStat = await fs.lstat(filePath);
    } catch (error) {
      if (error.code === "ENOENT") {
        // Ignore ENOENT error (file not found), as our intention is to delete it anyway.
        continue;
      }
      throw error;
    }

    if (fileStat.ctime.getTime() <= timeThreshold) {
      log.info(`Deleting: ${filePath}`);
      await deleteFileOrFolderRecursively(filePath);
      continue;
    }

    // (Windows only) Unsync files older than 1 minute. This helps OneDrive not to send files to trash bin.
    if (
      process.platform === "win32" &&
      parsedFile.from === "others" &&
      fileStat.ctime.getTime() <= now - 60000
    ) {
      await unsyncFileOrFolderRecursively(filePath);
    }
  }
};

export const unsyncFileOrFolderRecursively = async (fileOrFolder: string) => {
  function getAttributesWrapper(
    path: string,
    callback: (arg0: Error, arg1: FSWinAttributes) => void
  ) {
    fswin.getAttributes(path, function (result) {
      if (result) {
        callback(null, result);
      } else {
        callback(new Error(`Failed to set attributes of ${path}`), null);
      }
    });
  }

  function setAttributesWrapper(
    path: string,
    attributes: FSWinSetAttributes,
    callback: (arg0: Error, arg1: null) => void
  ) {
    fswin.setAttributes(path, attributes, function (succeeded) {
      if (succeeded) {
        callback(null, null);
      } else {
        callback(new Error(`Failed to set attributes of ${path}`), null);
      }
    });
  }

  const getAttributesAsync = promisify(getAttributesWrapper);
  const setAttributesAsync = promisify(setAttributesWrapper);

  await iterateThroughFilesRecursively([fileOrFolder], async (file) => {
    // Only unsync file if it has IS_REPARSE_POINT attribute
    if ((await getAttributesAsync(file)).IS_REPARSE_POINT) {
      log.info(`Unsyncing: ${file}`);
      await setAttributesAsync(file, {
        IS_UNPINNED: true,
        IS_PINNED: false,
      });
      return;
    }
  });
};

export type ClipboardText = {
  text?: string;
  html?: string;
  rtf?: string;
};

export const isClipboardTextEquals = (
  text1: ClipboardText,
  text2: ClipboardText
) => {
  if (text1.text && text2.text) {
    return text1.text === text2.text;
  }
  if (text1.html && text2.html) {
    return text1.html === text2.html;
  }
  if (text1.rtf && text2.rtf) {
    return text1.rtf === text2.rtf;
  }
  return false;
};

export const isClipboardTextEmpty = (text: ClipboardText) => {
  return !text.text && !text.html && !text.rtf;
};
