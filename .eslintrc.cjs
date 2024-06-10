// @ts-check

/** @type {import("@typescript-eslint/utils").TSESLint.ClassicConfig.Config} */
const config = {
  extends: [
    "eslint:recommended",
    "@electron-toolkit/eslint-config-ts/recommended",
  ],
  ignorePatterns: ["dist/", "out/"],
};

// eslint-disable-next-line no-undef
module.exports = config;
