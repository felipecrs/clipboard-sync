import * as fs from "node:fs";
import * as path from "node:path";

import { https } from "follow-redirects";
import { RequestOptions } from "https";
import { promisify } from "util";
import { createHash } from "crypto";

export const isArrayEquals = (arr1?: any[], arr2?: any[]) => {
  if (arr1 && arr2 && arr1.length == arr2.length) {
    arr1 = arr1.sort();
    arr2 = arr2.sort();
    return arr1.every((u: any, i: number) => u === arr2[i]);
  }
  return false;
};

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
export const deleteFolderRecursive = (directoryPath: string) => {
  if (fs.existsSync(directoryPath)) {
    const files = fs.readdirSync(directoryPath);
    for (const file of files) {
      const curPath = path.join(directoryPath, file);
      if (fs.lstatSync(curPath).isDirectory()) {
        // recurse
        deleteFolderRecursive(curPath);
      } else {
        // delete file
        fs.unlinkSync(curPath);
      }
    }
    fs.rmdirSync(directoryPath);
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
