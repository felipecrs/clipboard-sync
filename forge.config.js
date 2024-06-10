// @ts-check

import { MakerDMG } from "@electron-forge/maker-dmg";
import { MakerSquirrel } from "@electron-forge/maker-squirrel";
import path from "node:path";
import process from "node:process";

const getAppIcon = () => {
  const iconExtension =
    process.platform === "win32"
      ? "ico"
      : process.platform === "darwin"
      ? "icns"
      : "png";

  return path.resolve(
    import.meta.dirname,
    `./resources/appicons/${iconExtension}/icon.${iconExtension}`
  );
};

/** @type {import("@electron-forge/shared-types").ForgeConfig} */
const config = {
  packagerConfig: {
    icon: getAppIcon(),
    ignore: [
      /^\/(src)|(tools)|(.github)|(.vscode)/,
      /\/(.eslintrc.json)|(.gitignore)|(.gitattributes)|(electron.vite.config.ts)|(forge.config.cjs)|(tsconfig.json)|(bindl.config.js)|(bindl.config.js)|(README.md)$/,
    ],
    // Prevents the app from showing up in the dock on macOS
    extendInfo: {
      LSUIElement: true,
    },
  },
  rebuildConfig: {},
  makers: [
    new MakerSquirrel({
      name: "clipboard_sync",
      setupIcon: getAppIcon(),
      iconUrl: getAppIcon(),
    }),
    new MakerDMG({
      icon: getAppIcon(),
    }),
  ],
};

export default config;
