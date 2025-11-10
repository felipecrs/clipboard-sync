import fs from "node:fs/promises";
import path from "node:path";

import fswin from "fswin";
import log from "electron-log";

import {
  hostName,
  hostNameIsReceivingFileName,
  isReceivingFileNameSuffix,
} from "./global.js";
import {
  deleteFileOrFolderRecursively,
  iterateThroughFilesRecursively,
} from "./utilities.js";

export type ClipboardType = "text" | "image" | "files";

export type ParsedClipboardFileName = {
  file: string;
  beat: number;
  clipboardType: ClipboardType;
  from: "myself" | "others";
  filesCount?: number;
};

// This function always expects a file, not a folder
// returns undefined if not valid
export const parseClipboardFileName = (
  file: string,
  syncFolder: string,
  filter: "none" | "from-others" | "from-myself" = "none",
): ParsedClipboardFileName | undefined => {
  let beat = 0;
  let clipboardType: ClipboardType;
  let from: "myself" | "others";
  let filesCount: number | undefined;

  // Get relative path to syncFolder
  const relativePath = path.relative(syncFolder, file);

  // Get only the first level of folders
  const baseFile = relativePath.split(path.sep)[0];

  const match = baseFile.match(
    /^([1-9][0-9]*)-([0-9a-zA-Z-]+)\.((text\.json)|png|([1-9][0-9]*)_files)$/,
  );

  if (match) {
    if (match[5]) {
      clipboardType = "files";
      filesCount = Number.parseInt(match[5]);
    } else {
      clipboardType = match[3] === "text.json" ? "text" : "image";
    }
    from = match[2] === hostName ? "myself" : "others";
    switch (filter) {
      case "from-myself": {
        if (from === "myself") {
          beat = Number.parseInt(match[1]);
        }
        break;
      }
      case "from-others": {
        if (from === "others") {
          beat = Number.parseInt(match[1]);
        }
        break;
      }
      default: {
        beat = Number.parseInt(match[1]);
        break;
      }
    }
  }

  if (beat === 0) {
    return undefined;
  }

  return {
    file: path.join(syncFolder, baseFile),
    beat,
    clipboardType,
    from,
    filesCount,
  };
};

export function isIsReceivingFile(file: string): boolean {
  return file.endsWith(isReceivingFileNameSuffix);
}

export async function noComputersReceiving(
  syncFolder: string,
  now: number,
): Promise<boolean> {
  const directoryMembers = await fs.readdir(syncFolder);
  const computersReceiving = directoryMembers.filter(
    (file) => isIsReceivingFile(file) && file !== hostNameIsReceivingFileName,
  );

  // This file will be renewed on every 4 minutes, this will conside stale
  // any files older than 10 minutes
  const tenMinutesAgo = now - 600_000;
  for (const computerReceiving of computersReceiving) {
    const fileStat = await fs.stat(path.join(syncFolder, computerReceiving));
    if (fileStat.ctimeMs >= tenMinutesAgo) {
      return false;
    }
  }

  return true;
}

// Unsyncs from-others files older than 1 minute,
// Removes from-myself files older than 5 minutes,
// And removes from-others files older than 10 minutes.
export async function cleanFiles(syncFolder: string): Promise<void> {
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
          /^((0|[1-9][0-9]*)-[0-9a-zA-Z-]+\.txt)|(receiving-[0-9a-zA-Z-]+\.txt)|([0-9a-zA-Z-]+\.is-reading\.txt)$/,
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
      parsedFile.from === "myself" ? now - 300_000 : now - 600_000;

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

    if (fileStat.ctimeMs <= timeThreshold) {
      log.info(`Deleting: ${filePath}`);
      await deleteFileOrFolderRecursively(filePath);
      continue;
    }

    // (Windows only) Unsync files older than 1 minute. This helps OneDrive not to send files to trash bin.
    if (
      process.platform === "win32" &&
      parsedFile.from === "others" &&
      fileStat.ctimeMs <= now - 60_000
    ) {
      await unsyncFileOrFolderRecursively(filePath);
    }
  }
}

export async function unsyncFileOrFolderRecursively(
  fileOrFolder: string,
): Promise<void> {
  await iterateThroughFilesRecursively([fileOrFolder], async (file) => {
    // Only unsync file if it has IS_REPARSE_POINT attribute
    const attributes = await fswin.getAttributesAsync(file);
    if (attributes === null) {
      throw new Error(`Failed to get attributes for file: ${file}`);
    }
    if (attributes.IS_REPARSE_POINT) {
      log.info(`Unsyncing: ${file}`);
      const succeeded = await fswin.setAttributesAsync(file, {
        IS_UNPINNED: true,
        IS_PINNED: false,
      });
      if (!succeeded) {
        throw new Error(`Failed to set attributes for file: ${file}`);
      }
    }
  });
}

export type ClipboardText = {
  text?: string;
  html?: string;
  rtf?: string;
};

export function isClipboardTextEquals(
  text1: ClipboardText,
  text2: ClipboardText,
): boolean {
  if (text1 === undefined || text2 === undefined) {
    return false;
  }
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
}

export function isClipboardTextEmpty(text: ClipboardText): boolean {
  return !text.text && !text.html && !text.rtf;
}
