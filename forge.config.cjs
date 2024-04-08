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

module.exports = {
  packagerConfig: {
    icon: getAppIcon(),
    ignore: [
      /^\/(src)|(tools)|(.github)|(.vscode)/,
      /\/(.eslintrc.json)|(.gitignore)|(.gitattributes)|(electron.vite.config.ts)|(forge.config.cjs)|(tsconfig.json)|(bindl.config.js)|(bindl.config.js)|(README.md)$/,
    ],
    extendInfo: {
      LSUIElement: true,
    },
  },
  rebuildConfig: {},
  makers: [
    {
      name: "@electron-forge/maker-squirrel",
      config: {
        name: "clipboard_sync",
        setupIcon: getAppIcon(),
        iconUrl: getAppIcon(),
      },
      platforms: ["win32", "darwin"],
    },
  ],
};
