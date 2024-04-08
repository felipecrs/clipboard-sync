import { chmodSync } from "node:fs";

for (const platform of ["mac", "linux"]) {
  chmodSync(
    `./node_modules/clipboard-event/platform/clipboard-event-handler-${platform}`,
    0o755
  );
}
