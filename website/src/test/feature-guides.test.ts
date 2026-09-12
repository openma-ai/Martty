import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

describe("recent feature discovery", () => {
  it.each(["index", "en"])("links to localized guides and documents the current steer shortcut on %s", (page) => {
    const source = readFileSync(`src/pages/${page}.astro`, "utf8");
    const prefix = page === "en" ? "/en" : "";
    expect(source).toContain(`href="${prefix}/docs/harness-management"`);
    expect(source).toContain(`href="${prefix}/docs/sessions"`);
    expect(source).toContain("ACP Registry");
    expect(source).toContain("<code>ctrl+enter</code>");
    expect(source).not.toContain("<code>ctrl+x</code>");
  });

  it("connects the older ACP article to the Registry workflow", () => {
    const source = readFileSync("src/pages/blog/connect-acp-agent-to-martty.astro", "utf8");
    expect(source).toContain('href="/en/docs/harness-management"');
    expect(source).toContain("martty harness find");
  });
});
