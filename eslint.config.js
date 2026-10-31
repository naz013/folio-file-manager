// ESLint flat config: recommended rules for JS, TypeScript and Svelte 5, plus a few
// clean-code rules. Formatting is Prettier's job, so eslint-config-prettier turns off
// every stylistic rule that would conflict with it. See docs/CODE_STYLE.md.
import js from "@eslint/js";
import prettier from "eslint-config-prettier";
import svelte from "eslint-plugin-svelte";
import globals from "globals";
import ts from "typescript-eslint";
import svelteConfig from "./svelte.config.js";

export default ts.config(
  {
    ignores: ["build/", ".svelte-kit/", "node_modules/", "src-tauri/", "static/"],
  },
  js.configs.recommended,
  ...ts.configs.recommended,
  ...svelte.configs.recommended,
  prettier,
  ...svelte.configs.prettier,
  {
    languageOptions: {
      globals: { ...globals.browser, ...globals.node },
    },
  },
  {
    files: ["**/*.svelte", "**/*.svelte.ts"],
    languageOptions: {
      parserOptions: {
        projectService: true,
        extraFileExtensions: [".svelte"],
        parser: ts.parser,
        svelteConfig,
      },
    },
  },
  {
    rules: {
      // `x == null` is the one idiomatic loose comparison (null or undefined).
      eqeqeq: ["error", "always", { null: "ignore" }],
      "no-console": ["warn", { allow: ["warn", "error"] }],
      "no-sequences": "error",
      "no-unused-expressions": "off",
      "@typescript-eslint/no-unused-expressions": "error",
      "@typescript-eslint/consistent-type-imports": ["error", { fixStyle: "inline-type-imports" }],
      "@typescript-eslint/no-unused-vars": ["error", { argsIgnorePattern: "^_" }],
      "prefer-const": "error",
      "object-shorthand": "error",
    },
  },
  {
    // Svelte 5 declares `$props()` and `$derived()` with `let`; the Svelte rule knows runes.
    files: ["**/*.svelte", "**/*.svelte.ts"],
    rules: {
      "prefer-const": "off",
      "svelte/prefer-const": "error",
    },
  },
  {
    // Command-line scripts report through the console.
    files: ["scripts/**"],
    rules: { "no-console": "off" },
  },
);
