import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import { expect, it } from "vitest";

const stylesheet = readFileSync(resolve(process.cwd(), "src/styles/global.css"), "utf8");

it("allows the launch copy column to shrink on narrow viewports", () => {
  expect(stylesheet).toMatch(/\.hero__copy\s*{[^}]*min-width:\s*0;/s);
});
