import * as fs from "node:fs";
import * as path from "node:path";

import { clipboard } from "electron";
import * as clipboardEx from "electron-clipboard-ex";
import { hostName, isReceivingFileNameSuffix } from "./global";
import {
  calculateSha256,
  deleteFileOrFolderRecursively,
  getTotalNumberOfFiles,
  unsyncFileOrFolderRecursively,
} from "./utils";
import { shallowEqualArrays } from "shallow-equal";

export type ClipboardType = "text" | "image" | "files";

export const getNextWriteTime = (syncFolder: string) => {
  const numbers: number[] = [];
  const files = fs.readdirSync(syncFolder);
  for (const file of files) {
    const filePath = path.join(syncFolder, file);
    const clipboardFile = ClipboardFile.fromFileName(filePath);
    if (clipboardFile) {
      numbers.push(clipboardFile.number);
    }
  }
  if (numbers.length > 0) {
    const lastNumber = Math.max(...numbers);

    return lastNumber + 1;
  }
  return 1;
};

export const isThereMoreThanOneClipboardFile = (syncFolder: string) => {
  let found = 0;
  const files = fs.readdirSync(syncFolder);
  for (const file of files) {
    const filePath = path.join(syncFolder, file);
    const clipboardFile = ClipboardFile.fromFileName(filePath);
    if (clipboardFile) {
      found++;
      if (found > 1) {
        return;
      }
    }
  }
  return found > 1;
};

export const isIsReceivingFile = (file: string) => {
  return file.endsWith(isReceivingFileNameSuffix);
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

    const clipboardFile = ClipboardFile.fromFileName(filePath);

    if (!clipboardFile) {
      // These files will be deleted at application finish.
      if (isIsReceivingFile(filePath)) {
        continue;
      }

      // Check if it's a file used by previous versions.
      const match = path
        .parse(filePath)
        .base.match(
          /^((0|[1-9][0-9]*)|(receiving))-([0-9a-zA-Z-]+)(\.is-reading)?\.txt$/
        );
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
      clipboardFile.from === "others" &&
      fileStat.ctime.getTime() <= currentTimeMinus1Min
    ) {
      console.log(`Unsyncing: ${filePath}`);
      unsyncFileOrFolderRecursively(filePath);
    }
  }
};

abstract class ClipboardContent {
  abstract equals(other: ClipboardContent): boolean;
  abstract isEmpty(): boolean;
  abstract loadFromFileOrFolder(file: string): void;
}

class ClipboardText extends ClipboardContent {
  constructor(
    private text?: string,
    private html?: string,
    private rtf?: string
  ) {
    super();
  }

  isEmpty = () => {
    return !this.text && !this.html && !this.rtf;
  };

  equals = (other: ClipboardContent) => {
    if (!(other instanceof ClipboardText)) {
      return false;
    }

    // This is intentionally loose.
    if (this.text && other.text) {
      return this.text === other.text;
    }
    if (this.html && other.html) {
      return this.html === other.html;
    }
    if (this.rtf && other.rtf) {
      return this.rtf === other.rtf;
    }
    return false;
  };

  loadFromFileOrFolder = (file: string) => {
    const fileContent = fs.readFileSync(file, "utf8");
    const parsedFileContent = JSON.parse(fileContent);
    this.text = parsedFileContent.text ?? undefined;
    this.html = parsedFileContent.html ?? undefined;
    this.rtf = parsedFileContent.rtf ?? undefined;
  };
}

export class ClipboardImage extends ClipboardContent {
  private constructor(
    private image: Buffer,
    private sha256: string
  ) {
    super();
  }

  static fromPng = (png: Buffer) => {
    return new ClipboardImage(png, calculateSha256(png));
  };

  static fromNothing = () => {
    return new ClipboardImage(Buffer.from([]), "");
  };

  loadFromFileOrFolder = (file: string) => {
    this.image = fs.readFileSync(file);
    this.sha256 = calculateSha256(this.image);
  };

  equals = (other: ClipboardContent) => {
    if (!(other instanceof ClipboardImage)) {
      return false;
    }

    return this.sha256 === other.sha256;
  };

  isEmpty = () => {
    return this.sha256 === "";
  };
}

export class ClipboardFiles extends ClipboardContent {
  private constructor(
    private files: string[],
    private readonly count: number
  ) {
    super();
  }

  equals = (other: ClipboardContent) => {
    if (!(other instanceof ClipboardFiles)) {
      return false;
    }

    if (this.count !== other.count) {
      return false;
    }
    return shallowEqualArrays(this.files, other.files);
  };

  isEmpty = () => {
    return this.count === 0;
  };

  getCount = () => {
    return this.count;
  };

  static fromFilePaths(filePaths: string[]): ClipboardFiles {
    return new ClipboardFiles(filePaths, filePaths.length);
  }

  static fromFilesCount(filesCount: number): ClipboardFiles {
    return new ClipboardFiles([], filesCount);
  }

  loadFromFileOrFolder = (folder: string) => {
    const filesCountInFolder = getTotalNumberOfFiles([folder]);
    if (this.count !== filesCountInFolder) {
      throw new Error(
        `Expected ${this.count} files in ${folder}, but found ${filesCountInFolder}. Skipping...`
      );
    }
  };
}

type FromHost = "myself" | "others";

export class ClipboardFile {
  private constructor(
    public number: number,
    public from: FromHost,
    public content: ClipboardText | ClipboardImage | ClipboardFiles
  ) {}

  getFileOrFolderName = () => {
    if (this.content instanceof ClipboardText) {
      return `${this.number}-${hostName}.text.json`;
    }
    if (this.content instanceof ClipboardImage) {
      return `${this.number}-${hostName}.png`;
    }
    if (this.content instanceof ClipboardFiles) {
      return `${this.number}-${hostName}.${this.content.getCount()}_files`;
    }
    // This should never happen.
    throw new Error("Invalid content type.");
  };

  static fromFileName = (file: string): ClipboardFile | null => {
    let fileStat: fs.Stats;
    let parsedFile: path.ParsedPath;
    try {
      parsedFile = path.parse(file);
      fileStat = fs.lstatSync(file);
    } catch {
      return null;
    }

    function getFrom(hostNameFrom: string): FromHost {
      return hostNameFrom === hostName ? "myself" : "others";
    }

    if (fileStat.isDirectory()) {
      const match = parsedFile.base.match(
        /^(0|[1-9][0-9]*)-([0-9a-zA-Z-]+)\.(0|[1-9][0-9]*)_files$/
      );
      if (match) {
        return new ClipboardFile(
          parseInt(match[1]),
          getFrom(match[2]),
          ClipboardFiles.fromFilesCount(parseInt(match[3]))
        );
      }
      return null;
    }

    const match = parsedFile.base.match(
      /^(0|[1-9][0-9]*)-([0-9a-zA-Z-]+)\.((text\.json)|png)$/
    );
    if (match) {
      const number = parseInt(match[1]);
      const from = getFrom(match[2]);
      const extension = match[3];
      if (extension === "text.json") {
        return new ClipboardFile(number, from, new ClipboardText());
      }
      if (extension === "png") {
        return new ClipboardFile(number, from, ClipboardImage.fromNothing());
      }
    }
    return null;
  };

  static fromCurrentClipboard = (): ClipboardFile | null => {
    const number = 0;
    const from = "myself";
    const clipboardFormats = clipboard.availableFormats();
    if (
      clipboardFormats.includes("text/plain") ||
      clipboardFormats.includes("text/html") ||
      clipboardFormats.includes("text/rtf")
    ) {
      return new ClipboardFile(
        number,
        from,
        new ClipboardText(
          clipboardFormats.includes("text/plain")
            ? clipboard.readText()
            : undefined,
          clipboardFormats.includes("text/html")
            ? clipboard.readHTML()
            : undefined,
          clipboardFormats.includes("text/rtf")
            ? clipboard.readRTF()
            : undefined
        )
      );
    } else if (clipboardFormats.includes("image/png")) {
      return new ClipboardFile(
        number,
        from,
        ClipboardImage.fromPng(clipboard.readImage().toPNG())
      );
    } else if (clipboardFormats.includes("text/uri-list")) {
      return new ClipboardFile(
        number,
        from,
        ClipboardFiles.fromFilePaths(clipboardEx.readFilePaths())
      );
    }
    return null;
  };
}
