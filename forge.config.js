const path = require("path");

const getAppIcon = () => {
  const iconExtension = process.platform === 'win32' ? 'ico' : process.platform === 'darwin' ? 'icns' : 'png'

  return path.resolve(__dirname, `./assets/appicons/${iconExtension}/icon.${iconExtension}`);
}

module.exports = {
  packagerConfig: {
    icon: getAppIcon(),
  },
  makers: [
    {
      name: "@electron-forge/maker-squirrel",
      config: {
        name: "clipboard_sync",
        setupIcon: getAppIcon(),
        iconUrl: getAppIcon(),
      },
    },
    {
      name: "@electron-forge/maker-zip",
      platforms: ["darwin"],
    },
    {
      name: "@electron-forge/maker-deb",
      config: {},
    },
    {
      name: "@electron-forge/maker-rpm",
      config: {},
    },
  ],
};
