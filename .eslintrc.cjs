// @ts-check

/** @type {import("@typescript-eslint/utils").TSESLint.ClassicConfig.Config} */
const config = {
  extends: [
    "eslint:recommended",
    "@electron-toolkit/eslint-config-ts/recommended",
    "eslint-config-prettier",
  ],
  ignorePatterns: ["dist/", "out/"],
};

// eslint-disable-next-line no-undef
module.exports = config;
