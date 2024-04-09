// @ts-check

const MakerDmg = require("@electron-forge/maker-dmg").default;
const MakerSquirrel = require("@electron-forge/maker-squirrel").default;
const path = require("node:path");

const getAppIcon = () => {
  const iconExtension =
    process.platform === "win32"
      ? "ico"
      : process.platform === "darwin"
      ? "icns"
      : "png";

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
      setupIcon: getAppIcon(),
      iconUrl: getAppIcon(),
    }),
    new MakerDmg({
      icon: getAppIcon(),
    }),
  ],
};
