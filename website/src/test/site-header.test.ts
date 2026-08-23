import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const pageSource = (relativePath: string) =>
  readFileSync(fileURLToPath(new URL(relativePath, import.meta.url)), "utf8");

const headerMarkup = (source: string) => source.match(/<header[\s\S]*?<\/header>/)?.[0] ?? "";
const firstSectionMarkup = (source: string) => source.match(/<section[\s\S]*?<\/section>/)?.[0] ?? "";

describe("site header", () => {
  it("does not create an overflow scroll container that disables sticky positioning", () => {
    const globalCss = pageSource("../styles/global.css");
    expect(globalCss).toContain("overflow-x: clip;");
    expect(globalCss).not.toContain("overflow-x: hidden;");
  });

  it.each(["../pages/index.astro", "../pages/en.astro", "../components/ContentPage.astro"])(
    "keeps the %s header visible while scrolling",
    (page) => {
      expect(headerMarkup(pageSource(page))).toMatch(/class="[^"]*\bsticky\b[^"]*\btop-0\b[^"]*\bz-50\b/);
    },
  );

  it.each(["../pages/index.astro", "../pages/en.astro"])(
    "puts the Martty GitHub link in the %s header instead of the hero",
    (page) => {
      const source = pageSource(page);
      expect(headerMarkup(source)).toContain('href="https://github.com/openma-ai/Martty"');
      expect(firstSectionMarkup(source)).not.toContain('href="https://github.com/openma-ai/Martty"');
    },
  );
});
