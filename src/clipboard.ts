import * as fs from "node:fs";
import * as path from "node:path";
import * as fswin from "fswin";

import { hostName } from "./global";
import {
  deleteFileOrFolderRecursively,
  iterateThroughFilesRecursively,
} from "./utils";

export type ClipboardType = "text" | "image" | "files";

// returns null if not valid
export const parseClipboardFileName = (
  file: string,
  filter: "none" | "from-others" | "from-myself" = "none"
): {
  number: number;
  clipboardType: ClipboardType;
  from: "myself" | "others";
  filesCount?: number;
} => {
  let number = 0;
  let clipboardType: ClipboardType;
  let from: "myself" | "others";
  let filesCount: number | undefined;

  let fileStat;
  let parsedFile;
  try {
    parsedFile = path.parse(file);
    fileStat = fs.lstatSync(file);
  } catch {
    return null;
  }

  if (fileStat.isDirectory()) {
    const match = parsedFile.base.match(
      /^(0|[1-9][0-9]*)-([0-9a-zA-Z-]+)\.(0|[1-9][0-9]*)_files$/
    );
    if (match) {
      clipboardType = "files";
      filesCount = parseInt(match[3]);
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
  } else {
    const match = parsedFile.base.match(
      /^(0|[1-9][0-9]*)-([0-9a-zA-Z-]+)\.((text\.json)|png)$/
    );
    if (match) {
      clipboardType = match[3] === "text.json" ? "text" : "image";
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
  }

  if (number === 0) {
    return null;
  }

  return { number, clipboardType, from, filesCount };
};

export const getNextWriteTime = (syncFolder: string) => {
  const numbers: number[] = [];
  const files = fs.readdirSync(syncFolder);
  for (const file of files) {
    const filePath = path.join(syncFolder, file);
    const parsedFile = parseClipboardFileName(filePath);
    if (parsedFile) {
      numbers.push(parsedFile.number);
    }
  }
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
  const files = fs.readdirSync(syncFolder);
  for (const file of files) {
    const filePath = path.join(syncFolder, file);
    const parsedFile = parseClipboardFileName(filePath);
    if (parsedFile) {
      found++;
      if (found > 1) {
        return;
      }
    }
  }
  return found > 1;
};

export const isIsReceivingFile = (file: string) => {
  return file.endsWith(".is-receiving.txt");
};

// Unsyncs from-others files older than 1 minute,
// Removes from-myself files older than 5 minutes,
// And removes from-others files older than 10 minutes.
export const cleanFiles = (syncFolder: string) => {
  const now = Date.now();
  const currentTimeMinus1Min = now - 60000;
  const currentTimeMinus5Min = now - 300000;

  const files = fs.readdirSync(syncFolder);
  for (const file of files) {
    const filePath = path.join(syncFolder, file);

    const parsedFile = parseClipboardFileName(filePath);

    if (!parsedFile) {
      // These files will be deleted at application finish.
      if (isIsReceivingFile(filePath)) {
        continue;
      }

      // Check if it's a file used by previous versions.
      const match = path
        .parse(filePath)
        .base.match(/^((0|[1-9][0-9]*)|(receiving))-([0-9a-zA-Z-]+)\.txt$/);
      if (match) {
        console.log(`Deleting file used by previous versions: ${filePath}`);
        deleteFileOrFolderRecursively(filePath);
      }
      continue;
    }

    const fileStat = fs.statSync(filePath);
    if (fileStat.ctime.getTime() <= currentTimeMinus5Min) {
      console.log(`Deleting: ${filePath}`);
      deleteFileOrFolderRecursively(filePath);
      continue;
    }

    // (Windows only) Unsync files older than 1 minute. This helps OneDrive not to send files to trash bin.
    if (
      process.platform === "win32" &&
      parsedFile.from === "others" &&
      fileStat.ctime.getTime() <= currentTimeMinus1Min
    ) {
      console.log(`Unsyncing: ${filePath}`);
      unsyncFileOrFolderRecursively(filePath);
    }
  }
};

export const unsyncFileOrFolderRecursively = (fileOrFolder: string) => {
  iterateThroughFilesRecursively([fileOrFolder], (file) => {
    fswin.setAttributesSync(file, {
      IS_UNPINNED: true,
      IS_PINNED: false,
    });
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
