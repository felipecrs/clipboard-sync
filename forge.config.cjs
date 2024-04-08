// @ts-check

const MakerDmg = require("@electron-forge/maker-dmg").default;
const MakerSquirrel = require("@electron-forge/maker-squirrel").default;
const path = require("node:path");

const getAppIcon = (/** @type {NodeJS.Platform} */ platform) => {
  const iconExtension =
    platform === "win32" ? "ico" : platform === "darwin" ? "icns" : "png";

  return path.resolve(
    __dirname,
    `./resources/appicons/${iconExtension}/icon.${iconExtension}`
  );
};

/** @type {import("@electron-forge/shared-types").ForgeConfig} */
module.exports = {
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
      setupIcon: getAppIcon("win32"),
      iconUrl: getAppIcon("win32"),
    }),
    new MakerDmg({
      icon: getAppIcon("darwin"),
    }),
  ],
};
