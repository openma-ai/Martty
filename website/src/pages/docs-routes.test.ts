import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

describe("documentation routes", () => {
  it.each([
    "src/pages/docs/index.astro",
    "src/pages/docs/[slug].astro",
    "src/pages/en/docs/index.astro",
    "src/pages/en/docs/[slug].astro",
  ])("ships %s", (route) => {
    expect(existsSync(resolve(process.cwd(), route))).toBe(true);
  });

  it.each([
    "src/pages/docs/[slug].astro",
    "src/pages/en/docs/[slug].astro",
  ])("returns a real 404 for an unknown slug in %s", (route) => {
    const source = readFileSync(resolve(process.cwd(), route), "utf8");
    expect(source).toContain("status: 404");
    expect(source).not.toContain("Astro.redirect");
  });

  it("uses the shared SEO head for documentation pages", () => {
    const source = readFileSync(resolve(process.cwd(), "src/layouts/DocsLayout.astro"), "utf8");
    expect(source).toContain('import SeoHead from "../components/SeoHead.astro"');
    expect(source).toContain("<SeoHead");
  });
});
