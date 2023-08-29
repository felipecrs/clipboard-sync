const version = "2023.08.07.00";

module.exports = {
  downloadDirectory: "./resources/binaries",
  binaries: [
    {
      platform: "win32",
      arch: "x64",
      url: `https://github.com/facebook/watchman/releases/download/v${version}/watchman-v${version}-windows.zip`,
      files: [
        {
          source: `watchman-v${version}-windows/bin/`,
          target: "watchman/",
        },
      ],
    },
  ],
};
