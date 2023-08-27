import * as os from "node:os";

export const hostName = os.hostname();
export const hostNameIsReceivingFileName = `${hostName}.is-reading.txt`;
