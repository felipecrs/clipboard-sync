import * as fs from "node:fs";
import * as path from "node:path";

import * as fswin from "fswin";
import { https } from "follow-redirects";
import { RequestOptions } from "https";
import { promisify } from "util";
import { createHash } from "crypto";

export const iterateThroughFilesRecursively = (
  paths: string[],
  fn: (arg0: string) => unknown
): unknown[] => {
  const results: unknown[] = [];
  for (const fileOrFolder of paths) {
    if (fs.existsSync(fileOrFolder)) {
      if (fs.statSync(fileOrFolder).isDirectory()) {
        const files = fs.readdirSync(fileOrFolder);
        for (const file of files) {
          const filePath = path.join(fileOrFolder, file);
          const results = iterateThroughFilesRecursively([filePath], fn);
          for (const result of results) {
            if (result) {
              results.push(result);
            }
          }
        }
      } else {
        const result = fn(fileOrFolder);
        if (result) {
          results.push(result);
        }
      }
    }
  }
  return results;
};

export const unsyncFileOrFolderRecursively = (fileOrFolder: string) => {
  iterateThroughFilesRecursively([fileOrFolder], (file) => {
    fswin.setAttributesSync(file, {
      IS_UNPINNED: true,
      IS_PINNED: false,
    });
  });
};

export const getTotalNumberOfFiles = (paths: string[]): number => {
  let totalNumberOfFiles = 0;
  iterateThroughFilesRecursively(paths, (file) => {
    totalNumberOfFiles++;
  });
  return totalNumberOfFiles;
};

export const getFilesSizeInMb = (paths: string[]) => {
  let totalSize = 0;
  const results = iterateThroughFilesRecursively(paths, (file) => {
    return fs.lstatSync(file).size / (1024 * 1024);
  });
  for (const size of results) {
    if (typeof size === "number") {
      totalSize += size;
    }
  }

  return totalSize;
};

// https://stackoverflow.com/a/32197381/12156188
export const deleteFileOrFolderRecursively = (fileOrFolder: string) => {
  if (fs.existsSync(fileOrFolder)) {
    if (fs.lstatSync(fileOrFolder).isDirectory()) {
      const files = fs.readdirSync(fileOrFolder);
      for (const file of files) {
        const filePath = path.join(fileOrFolder, file);
        // recurse
        deleteFileOrFolderRecursively(filePath);
      }
      fs.rmdirSync(fileOrFolder);
    } else {
      // delete file
      fs.unlinkSync(fileOrFolder);
    }
  }
};

export const copyFolderRecursive = (source: string, destination: string) => {
  fs.mkdirSync(destination);
  const files = fs.readdirSync(source);
  for (const file of files) {
    const curPath = path.join(source, file);
    const fullDestination = path.join(destination, path.basename(curPath));
    if (fs.lstatSync(curPath).isDirectory()) {
      // recurse
      copyFolderRecursive(curPath, fullDestination);
    } else {
      // copy file
      fs.copyFileSync(curPath, fullDestination);
    }
  }
};

export const calculateSha256 = (data: Buffer) => {
  return createHash("sha256").update(data).digest("hex");
};

export const getRedirectedUrl = async (requestOptions: RequestOptions) => {
  return await promisify(
    (requestOptions: RequestOptions, callback: Function) => {
      const request = https.request(requestOptions, (response) => {
        callback(null, response.responseUrl);
      });
      request.end();
    }
  )(requestOptions);
};
