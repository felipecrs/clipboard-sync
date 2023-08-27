const version = "2023.08.07.00";

module.exports = {
  binaries: [
    {
      platform: "win32",
      arch: "x64",
      url: `https://github.com/facebook/watchman/releases/download/v${version}/watchman-v${version}-windows.zip`,
    },
  ],
};
