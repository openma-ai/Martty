import { describe, expect, it } from "vitest";
import { getDoc, getDocSeo } from "./docs";

describe("repository-backed documentation", () => {
  it("renders the Chinese architecture source with stable heading anchors", () => {
    const page = getDoc("zh", "architecture");
    expect(page?.title).toBe("架构");
    expect(page?.html).toContain("Host 进程");
    expect(page?.html).toContain('id="plugin-package-与视图"');
    expect(page?.headings).toContainEqual({
      depth: 2,
      id: "plugin-package-与视图",
      text: "Plugin Package 与视图",
    });
  });

  it("maps the English routes to the English repository sources", () => {
    const page = getDoc("en", "plugins");
    const chinesePage = getDoc("zh", "plugins");
    expect(page?.title).toMatch(/plugin api/i);
    expect(page?.html).toContain('<h1 id="plugin-api">Plugin API</h1>');
    expect(page?.html).not.toBe(chinesePage?.html);
  });

  it("rewrites repository-relative links to working docs or GitHub destinations", () => {
    const architecture = getDoc("zh", "architecture");
    const plugins = getDoc("en", "plugins");

    expect(architecture?.html).toContain('href="/docs/plugins"');
    expect(plugins?.html).toContain(
      'href="https://github.com/openma-ai/Martty/blob/main/docs/tui-palette.v0.schema.json"',
    );
  });

  it("gives every localized documentation page unique search metadata", () => {
    const slugs = ["plugin-systems", "architecture", "plugins", "migration", "harness-management", "sessions"];
    for (const locale of ["zh", "en"] as const) {
      const entries = slugs.map((slug) => getDocSeo(locale, slug));
      expect(entries.every(Boolean)).toBe(true);
      expect(new Set(entries.map((entry) => entry?.title)).size).toBe(slugs.length);
      expect(new Set(entries.map((entry) => entry?.description)).size).toBe(slugs.length);
      for (const entry of entries) {
        expect(entry?.title.length).toBeGreaterThanOrEqual(18);
        expect(entry?.title.length).toBeLessThanOrEqual(60);
        expect(entry?.description.length).toBeGreaterThanOrEqual(locale === "en" ? 135 : 45);
        expect(entry?.description.length).toBeLessThanOrEqual(165);
      }
    }
  });

  it.each(["zh", "en"] as const)("renders practical, linked workflow guides in %s", (locale) => {
    const harness = getDoc(locale, "harness-management");
    const sessions = getDoc(locale, "sessions");
    const prefix = locale === "en" ? "/en" : "";
    expect(harness?.html).toContain("martty harness find");
    expect(harness?.html).toContain("--cleanup --dry-run");
    expect(harness?.html).toContain("authenticate");
    expect(harness?.html).toContain(`href="${prefix}/docs/sessions"`);
    expect(sessions?.html).toContain("/session next");
    expect(sessions?.html).toContain("/resume 10");
    expect(sessions?.html).toContain("/close");
    expect(sessions?.html).toContain("Ctrl+Enter");
    expect(sessions?.html).toContain(`href="${prefix}/docs/harness-management"`);
    for (const page of [harness, sessions]) {
      expect(page?.headings.length).toBeGreaterThanOrEqual(4);
      expect(page?.html.match(/<h1 /g)).toHaveLength(1);
    }
    expect(harness?.html).not.toBe(getDoc(locale === "en" ? "zh" : "en", "harness-management")?.html);
  });
});
