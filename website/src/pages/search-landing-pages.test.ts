import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

import { SEARCH_LANDINGS } from "../data/search-landings";
import { GET } from "./sitemap.xml";

const LANDINGS = [
  {
    route: "deepseek-harness-tui",
    phrases: ["DeepSeek Harness TUI", "dsh --profile martty"],
  },
  {
    route: "acp-terminal-client",
    phrases: ["ACP terminal client", "Agent Client Protocol"],
  },
  {
    route: "ratatui-agent-tui",
    phrases: ["Ratatui agent TUI", "Rust TUI", "Ratatouille"],
  },
] as const;

function source(relativePath: string) {
  const path = resolve(process.cwd(), relativePath);
  return existsSync(path) ? readFileSync(path, "utf8") : "";
}

describe("search-intent landing pages", () => {
  it.each(LANDINGS)("ships a substantial $route page", ({ route, phrases }) => {
    const routeSource = source(`src/pages/${route}.astro`);
    const sharedSource = source("src/data/search-landings.ts");
    const combined = `${routeSource}\n${sharedSource}`;

    expect(routeSource).not.toBe("");
    expect(combined.length).toBeGreaterThan(5_000);
    for (const phrase of phrases) expect(combined).toContain(phrase);
  });

  it.each(LANDINGS)("writes $route as a source-grounded technical article", ({ route }) => {
    const page = SEARCH_LANDINGS[route];
    const prose = [
      page.thesis,
      page.lede,
      ...page.sections.flatMap((section) => section.paragraphs),
      ...page.faq.flatMap((item) => [item.question, item.answer]),
    ].join("\n");

    expect(page.thesis.length).toBeGreaterThan(180);
    expect(page.diagram.body).toMatch(/[│┌└]|-->/);
    expect(page.sourceMap.length).toBeGreaterThanOrEqual(4);
    expect(page.sourceMap.every((source) => source.path && source.href.includes("github.com/openma-ai/Martty"))).toBe(true);
    expect(page.sourceMap.every((source) => existsSync(resolve(process.cwd(), "..", source.path)))).toBe(true);
    expect(page.failureModes.length).toBeGreaterThanOrEqual(4);
    expect(page.verification.commands.length).toBeGreaterThanOrEqual(2);
    expect(page.verification.expected.length).toBeGreaterThanOrEqual(2);
    expect(page.sections.length).toBeGreaterThanOrEqual(7);
    expect(page.sections.every((section) => section.paragraphs.length >= 2)).toBe(true);
    expect(page.sections.some((section) => section.code && section.code.body.length > 300)).toBe(true);
    expect(prose.length).toBeGreaterThan(7_500);
  });

  it.each(LANDINGS)("gives $route a concise search result and immediate answer", ({ route, phrases }) => {
    const page = SEARCH_LANDINGS[route];
    const primaryPhrase = phrases[0];

    expect(page.title.toLowerCase().startsWith(primaryPhrase.toLowerCase())).toBe(true);
    expect(page.title.length).toBeLessThanOrEqual(65);
    expect(page.metaTitle.length).toBeGreaterThanOrEqual(45);
    expect(page.metaTitle.length).toBeLessThanOrEqual(60);
    expect(page.description.length).toBeGreaterThanOrEqual(135);
    expect(page.description.length).toBeLessThanOrEqual(165);
    expect(page.quickAnswer.title.length).toBeGreaterThan(10);
    expect(page.quickAnswer.paragraphs.length).toBeGreaterThanOrEqual(2);
    expect(page.quickAnswer.paragraphs.join(" ").length).toBeGreaterThan(260);
    expect(page.sourceRevision.commit).toMatch(/^[0-9a-f]{7}$/);
    expect(page.sourceRevision.href).toContain(page.sourceRevision.commit);
  });

  it("links every search-intent page from the homepage", () => {
    const homepage = source("src/pages/index.astro");
    for (const { route } of LANDINGS) expect(homepage).toContain(`href="/${route}"`);
  });

  it("links every search-intent page from the English home and blog hub", () => {
    const englishHome = source("src/pages/en.astro");
    const blogHub = source("src/pages/blog/index.astro");
    for (const { route } of LANDINGS) {
      expect(englishHome).toContain(`href="/${route}"`);
      expect(blogHub).toContain(`href="/${route}"`);
    }
  });

  it("publishes every search-intent page in the sitemap", async () => {
    const response = await GET({
      site: new URL("https://martty.sh"),
      url: new URL("https://martty.sh/sitemap.xml"),
    } as never);
    const xml = await response.text();

    for (const { route } of LANDINGS) expect(xml).toContain(`https://martty.sh/${route}`);
  });
});
