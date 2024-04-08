import os from "node:os";

// on macOS, the hostname may contain a dot, so we split it and use the first part
export const hostName = os.hostname().split(".")[0];
export const isReceivingFileNameSuffix = ".is-receiving.txt";
export const hostNameIsReceivingFileName = `${hostName}${isReceivingFileNameSuffix}`;
