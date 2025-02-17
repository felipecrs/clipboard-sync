// @ts-check

import neostandard, { resolveIgnoresFromGitignore } from "neostandard";
import eslintPluginUnicorn from "eslint-plugin-unicorn";

export default [
  ...neostandard({
    ignores: resolveIgnoresFromGitignore(),
    noStyle: true,
    ts: true,
  }),
  eslintPluginUnicorn.configs["flat/recommended"],
  {
    rules: {
      "unicorn/prefer-ternary": ["error", "only-single-line"],
      "unicorn/no-nested-ternary": "off",
      "unicorn/no-useless-undefined": ["error", { checkArguments: false }],
    },
  },
];
