import { createHash } from "node:crypto";
import fs from "node:fs/promises";
import { RequestOptions } from "node:https";
import path from "node:path";
import { promisify } from "node:util";

import log from "electron-log";

import followRedirects from "follow-redirects";

export function arraysEqual(a: unknown[], b: unknown[]): boolean {
  if (a === b) return true;
  if (a === null || b === null) return false;
  if (a === undefined || b === undefined) return false;
  if (a.length !== b.length) return false;

  a = a.toSorted();
  b = b.toSorted();

  for (const [index, element] of a.entries()) {
    if (element !== b[index]) return false;
  }
  return true;
}

export async function iterateThroughFilesRecursively(
  paths: string[],
  function_: (argument0: string) => unknown,
): Promise<unknown[]> {
  const results: unknown[] = [];
  for (const fileOrFolder of paths) {
    try {
      const stat = await fs.lstat(fileOrFolder);
      if (stat.isDirectory()) {
        const files = await fs.readdir(fileOrFolder);
        for (const file of files) {
          const filePath = path.join(fileOrFolder, file);
          const subResults = await iterateThroughFilesRecursively(
            [filePath],
            function_,
          );
          for (const result of subResults) {
            if (result) {
              results.push(result);
            }
          }
        }
      } else {
        const result = function_(fileOrFolder);
        if (result) {
          results.push(result);
        }
      }
    } catch (error) {
      log.error(`Error while iterating through ${fileOrFolder}:\n${error}`);
    }
  }
  return results;
}

export const getTotalNumberOfFiles = async (
  paths: string[],
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
    const stat = await fs.lstat(file);
    return stat.size / (1024 * 1024);
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
  fileOrFolder: string,
): Promise<void> {
  try {
    const stat = await fs.lstat(fileOrFolder);
    if (stat.isDirectory()) {
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
  } catch (error) {
    log.error(`Error deleting ${fileOrFolder}:\n${error}`);
  }
}

export async function copyFolderRecursive(
  source: string,
  destination: string,
): Promise<void> {
  await fs.mkdir(destination, { recursive: true });
  const files = await fs.readdir(source);
  for (const file of files) {
    const currentPath = path.join(source, file);
    const fullDestination = path.join(destination, path.basename(currentPath));
    const stat = await fs.lstat(currentPath);
    stat.isDirectory()
      ? await copyFolderRecursive(currentPath, fullDestination)
      : await fs.copyFile(currentPath, fullDestination);
  }
}

export function calculateSha256(data: Buffer): string {
  return createHash("sha256").update(data).digest("hex");
}

export async function getRedirectedUrl(
  requestOptions: RequestOptions,
): Promise<string> {
  const result = await promisify(
    (
      requestOptions: RequestOptions,
      callback: (argument0: unknown, argument1: string) => void,
    ) => {
      const requestObject = followRedirects.https.request(
        requestOptions,
        (response) => {
          callback(undefined, response.responseUrl);
        },
      );
      requestObject.end();
    },
  )(requestOptions);
  if (typeof result === "string") {
    return result;
  }
  throw new Error("Failed to get redirected URL");
}
