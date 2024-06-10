// @ts-check

import { chmodSync } from "node:fs";

// https://github.com/sudhakar3697/node-clipboard-event/blob/0879a167f5643908349ff6b70a9365f9acdb652e/README.md#usage
const platform = process.platform === "darwin" ? "mac" : process.platform;
if (platform === "mac" || platform === "linux") {
  chmodSync(
    `./node_modules/clipboard-event/platform/clipboard-event-handler-${platform}`,
    0o755,
  );
}
