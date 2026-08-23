import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import sharp from "sharp";
import { describe, expect, it } from "vitest";

const assetPath = (name: string) => resolve(process.cwd(), "public", name);
const readIfPresent = (name: string) => {
  const path = assetPath(name);
  return existsSync(path) ? readFileSync(path, "utf8") : "";
};

describe("Martty favicon", () => {
  it("renders the canonical OpenMA mark in six hard terminal-like color bands", async () => {
    const svg = readIfPresent("favicon.svg");
    const palette = new Set([
      "65,118,230",
      "94,140,235",
      "123,161,240",
      "153,183,245",
      "182,204,250",
      "211,226,255",
    ]);
    const { data } = await sharp(Buffer.from(svg)).raw().toBuffer({ resolveWithObject: true });
    const opaqueColors = new Set<string>();

    for (let index = 0; index < data.length; index += 4) {
      if (data[index + 3] === 255) {
        opaqueColors.add(`${data[index]},${data[index + 1]},${data[index + 2]}`);
      }
    }

    expect(svg).toContain('viewBox="240 244 548 454"');
    expect(svg.match(/<path /g)).toHaveLength(3);
    expect(svg).toContain('<circle cx="535" cy="520" r="42"');
    expect(opaqueColors).toEqual(palette);
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
