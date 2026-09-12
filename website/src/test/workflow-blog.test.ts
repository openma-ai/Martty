import { existsSync, readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { GET } from "../pages/sitemap.xml";

const slug = "codex-claude-code-one-terminal";

describe("user-workflow blog entry points", () => {
  it.each(["zh", "en"])("publishes a localized article with a counterpart and separate command reference: %s", (locale) => {
    const prefix = locale === "en" ? "/en" : "";
    const file = `src/pages${prefix}/blog/${slug}.astro`;
    expect(existsSync(file)).toBe(true);
    const source = readFileSync(file, "utf8");
    expect(source).toContain('import ContentPage');
    expect(source).toContain(`path="${prefix}/blog/${slug}"`);
    expect(source).toContain(`alternatePath="${locale === "en" ? "" : "/en"}/blog/${slug}"`);
    expect(source).toContain(`href="${prefix}/docs/harness-management"`);
    expect(source).toContain("showDescription={false}");
  });

  it("makes the article reachable from both homepages and the blog index", () => {
    for (const [file, prefix] of [["index.astro", ""], ["en.astro", "/en"], ["blog/index.astro", "/en"]]) {
      expect(readFileSync(`src/pages/${file}`, "utf8")).toContain(`${prefix}/blog/${slug}`);
    }
  });

  it("publishes reciprocal sitemap language alternates", async () => {
    const response = await GET({site: new URL("https://martty.sh"), url: new URL("https://martty.sh/sitemap.xml")} as never);
    const xml = await response.text();
    for (const prefix of ["", "/en"]) {
      expect(xml).toContain(`<loc>https://martty.sh${prefix}/blog/${slug}</loc>`);
    }
    expect(xml).toContain(`hreflang="zh-CN" href="https://martty.sh/blog/${slug}"`);
    expect(xml).toContain(`hreflang="en" href="https://martty.sh/en/blog/${slug}"`);
  });
});
