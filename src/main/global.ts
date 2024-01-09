import os from "node:os";

export const hostName = os.hostname();
export const isReceivingFileNameSuffix = ".is-receiving.txt";
export const hostNameIsReceivingFileName = `${hostName}${isReceivingFileNameSuffix}`;
