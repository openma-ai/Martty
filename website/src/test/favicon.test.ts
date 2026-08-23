import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

const assetPath = (name: string) => resolve(process.cwd(), "public", name);
const readIfPresent = (name: string) => {
  const path = assetPath(name);
  return existsSync(path) ? readFileSync(path, "utf8") : "";
};

describe("Martty favicon", () => {
  it("uses the canonical OpenMA mark with the Martty blue gradient", () => {
    const svg = readIfPresent("favicon.svg");

    expect(svg).toContain('viewBox="240 244 548 454"');
    expect(svg.match(/<path /g)).toHaveLength(3);
    expect(svg).toContain('<circle cx="535" cy="520" r="42"');
    expect(svg).toContain('stop-color="#4176e6"');
    expect(svg).toContain('stop-color="#d3e2ff"');
  });

  it("ships browser and home-screen bitmap fallbacks", () => {
    expect(existsSync(assetPath("favicon.png"))).toBe(true);
    expect(existsSync(assetPath("favicon.ico"))).toBe(true);
    expect(existsSync(assetPath("apple-touch-icon.png"))).toBe(true);
  });

  it("advertises the vector favicon and Apple touch icon", () => {
    const seoHeadPath = resolve(process.cwd(), "src/components/SeoHead.astro");
    const seoHead = readFileSync(seoHeadPath, "utf8");

    expect(seoHead).toContain('<link rel="icon" type="image/svg+xml" href="/favicon.svg" />');
    expect(seoHead).toContain('<link rel="apple-touch-icon" sizes="180x180" href="/apple-touch-icon.png" />');
  });
});
