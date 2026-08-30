import { describe, expect, it } from "vitest";

import { GET } from "./sitemap.xml";

describe("sitemap", () => {
  it("publishes both localized documentation trees", async () => {
    const response = await GET({
      site: new URL("https://martty.sh"),
      url: new URL("https://martty.sh/sitemap.xml"),
    } as never);
    const xml = await response.text();

    expect(xml).toContain("https://martty.sh/docs/plugin-systems");
    expect(xml).toContain("https://martty.sh/docs/architecture");
    expect(xml).toContain("https://martty.sh/en/docs/plugin-systems");
    expect(xml).toContain("https://martty.sh/en/docs/plugins");
  });

  it("declares reciprocal language alternates for every documentation topic", async () => {
    const response = await GET({
      site: new URL("https://martty.sh"),
      url: new URL("https://martty.sh/sitemap.xml"),
    } as never);
    const xml = await response.text();

    for (const slug of ["plugin-systems", "architecture", "plugins", "migration"]) {
      expect(xml).toContain(`hreflang="zh-CN" href="https://martty.sh/docs/${slug}"`);
      expect(xml).toContain(`hreflang="en" href="https://martty.sh/en/docs/${slug}"`);
    }
  });
});
