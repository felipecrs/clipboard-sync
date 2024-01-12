import fs from "node:fs/promises";
import path from "node:path";
import { RequestOptions } from "node:https";
import { promisify } from "node:util";
import { createHash } from "node:crypto";
import followRedirects from "follow-redirects";

export const isArrayEquals = (arr1?: any[], arr2?: any[]) => {
  if (arr1 && arr2 && arr1.length == arr2.length) {
    arr1 = arr1.sort();
    arr2 = arr2.sort();
    return arr1.every((u: any, i: number) => u === arr2[i]);
  }
  return false;
};

export const iterateThroughFilesRecursively = async (
  paths: string[],
  fn: (arg0: string) => unknown
): Promise<unknown[]> => {
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
      console.error(err);
    }
  }
  return results;
};

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
export const deleteFileOrFolderRecursively = async (fileOrFolder: string) => {
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
    console.error(err);
  }
};

export const copyFolderRecursive = async (
  source: string,
  destination: string
) => {
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
};

export const calculateSha256 = (data: Buffer) => {
  return createHash("sha256").update(data).digest("hex");
};

export const getRedirectedUrl = async (requestOptions: RequestOptions) => {
  return await promisify(
    (requestOptions: RequestOptions, callback: Function) => {
      const request = followRedirects.https.request(
        requestOptions,
        (response) => {
          callback(null, response.responseUrl);
        }
      );
      request.end();
    }
  )(requestOptions);
};
