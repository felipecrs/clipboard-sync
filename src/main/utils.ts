import fs from "node:fs/promises";
import path from "node:path";
import { RequestOptions } from "node:https";
import { promisify } from "node:util";
import { createHash } from "node:crypto";
import followRedirects from "follow-redirects";
import log from "electron-log";

export function isArrayEquals(arr1?: unknown[], arr2?: unknown[]): boolean {
  if (arr1 && arr2 && arr1.length == arr2.length) {
    arr1 = arr1.sort();
    arr2 = arr2.sort();
    return arr1.every((u: unknown, i: number) => u === arr2[i]);
  }
  return false;
}

export async function iterateThroughFilesRecursively(
  paths: string[],
  fn: (arg0: string) => unknown
): Promise<unknown[]> {
  const results: unknown[] = [];
  for (const fileOrFolder of paths) {
    try {
      if ((await fs.lstat(fileOrFolder)).isDirectory()) {
        const files = await fs.readdir(fileOrFolder);
        for (const file of files) {
          const filePath = path.join(fileOrFolder, file);
          const subResults = await iterateThroughFilesRecursively(
            [filePath],
            fn
          );
          for (const result of subResults) {
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
    } catch (err) {
      log.error(`Error while iterating through ${fileOrFolder}:\n${err}`);
    }
  }
  return results;
}

export const getTotalNumberOfFiles = async (
  paths: string[]
): Promise<number> => {
  let totalNumberOfFiles = 0;
  await iterateThroughFilesRecursively(paths, () => {
    totalNumberOfFiles++;
  });
  return totalNumberOfFiles;
};

export const getFilesSizeInMb = async (paths: string[]): Promise<number> => {
  let totalSize = 0;
  const results = await iterateThroughFilesRecursively(paths, async (file) => {
    return (await fs.lstat(file)).size / (1024 * 1024);
  });
  for (const size of results) {
    if (typeof size === "number") {
      totalSize += size;
    }
  }

  return totalSize;
};

// https://stackoverflow.com/a/32197381/12156188
export async function deleteFileOrFolderRecursively(
  fileOrFolder: string
): Promise<void> {
  try {
    if ((await fs.lstat(fileOrFolder)).isDirectory()) {
      const files = await fs.readdir(fileOrFolder);
      for (const file of files) {
        const filePath = path.join(fileOrFolder, file);
        // recurse
        await deleteFileOrFolderRecursively(filePath);
      }
      await fs.rmdir(fileOrFolder);
    } else {
      // delete file
      await fs.unlink(fileOrFolder);
    }
  } catch (err) {
    log.error(`Error deleting ${fileOrFolder}:\n${err}`);
  }
}

export async function copyFolderRecursive(
  source: string,
  destination: string
): Promise<void> {
  await fs.mkdir(destination, { recursive: true });
  const files = await fs.readdir(source);
  for (const file of files) {
    const curPath = path.join(source, file);
    const fullDestination = path.join(destination, path.basename(curPath));
    if ((await fs.lstat(curPath)).isDirectory()) {
      // recurse
      await copyFolderRecursive(curPath, fullDestination);
    } else {
      // copy file
      await fs.copyFile(curPath, fullDestination);
    }
  }
}

export function calculateSha256(data: Buffer): string {
  return createHash("sha256").update(data).digest("hex");
}

export async function getRedirectedUrl(
  requestOptions: RequestOptions
): Promise<string> {
  const result = await promisify(
    (
      requestOptions: RequestOptions,
      callback: (arg0: unknown, arg1: string) => void
    ) => {
      const requestObj = followRedirects.https.request(
        requestOptions,
        (response) => {
          callback(null, response.responseUrl);
        }
      );
      requestObj.end();
    }
  )(requestOptions);
  if (typeof result === "string") {
    return result;
  }
  throw new Error("Failed to get redirected URL");
}
