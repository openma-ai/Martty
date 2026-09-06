import { readdirSync } from "node:fs";
import { expect, it } from "vitest";

it("keeps test modules out of Astro's public route directory", () => {
  const routes = readdirSync("src/pages", { recursive: true });
  expect(routes.filter((path) => /\.(test|spec)\.[cm]?[jt]sx?$/.test(String(path)))).toEqual([]);
});
