// @ts-check

import path from "node:path";

import { MakerDMG } from "@electron-forge/maker-dmg";
import { MakerSquirrel } from "@electron-forge/maker-squirrel";
import { MakerFlatpak } from "@electron-forge/maker-flatpak";

const iconExtension =
  process.platform === "win32"
    ? "ico"
    : process.platform === "darwin"
      ? "icns"
      : "png";
const iconRelative = `resources/appicons/${iconExtension}/icon.${iconExtension}`;
const icon = path.join(import.meta.dirname, iconRelative);

/** @type {import("@electron-forge/shared-types").ForgeConfig} */
const config = {
  packagerConfig: {
    icon,
    ignore: [
      /^\/(src)|(tools)|(.github)|(.vscode)/,
      /\/(.gitignore)|(.gitattributes)|(electron.vite.config.ts)|(eslint.config.js)|(forge.config.js)|(prettier.config.js)|(renovate.json)|(tsconfig.json)|(README.md)$/,
    ],
    // Prevents the app from showing up in the dock on macOS
    extendInfo: {
      LSUIElement: true,
    },
    // https://github.com/electron/forge/issues/2805#issuecomment-3193871995
    ...(process.platform === "linux"
      ? { executableName: "clipboard-sync" }
      : {}),
  },
  rebuildConfig: {},
  makers: [
    new MakerSquirrel({
      name: "clipboard_sync",
      setupIcon: icon,
      iconUrl: `https://raw.githubusercontent.com/felipecrs/clipboard-sync/refs/heads/master/${iconRelative}`,
    }),
    new MakerDMG({
      icon,
    }),
    new MakerFlatpak({
      options: {
        id: "io.github.felipecrs.ClipboardSync",
        // @ts-expect-error - this is the correct way
        icon: {
          "512x512": icon,
        },
        categories: ["Utility"],
        sdk: "org.freedesktop.Sdk",
        runtime: "org.freedesktop.Platform",
        runtimeVersion: "24.08",
        base: "org.electronjs.Electron2.BaseApp",
        baseVersion: "24.08",
        finishArgs: [
          // Needed to monitor clipboard
          "--socket=system-bus",
          // Default permissions
          "--socket=x11",
          "--share=ipc",
          "--device=dri",
          "--socket=pulseaudio",
          "--filesystem=home",
          "--env=TMPDIR=/var/tmp",
          "--share=network",
          "--talk-name=org.freedesktop.Notifications",
        ],
        // https://github.com/electron/forge/issues/2805
        modules: [
          {
            name: "zypak",
            sources: [
              {
                type: "git",
                url: "https://github.com/refi64/zypak",
                // https://github.com/refi64/zypak/releases
                tag: "v2024.01.17",
              },
            ],
          },
        ],
      },
    }),
  ],
};

export default config;
