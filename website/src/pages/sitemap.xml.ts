import type { APIRoute } from "astro";

export const GET: APIRoute = ({ site, url }) => {
  const origin = site?.origin ?? url.origin;
  const pages = [
    "/",
    "/en",
    "/deepseek-harness-tui",
    "/acp-terminal-client",
    "/ratatui-agent-tui",
    "/plugins",
    "/guides/migrate-to-martty",
    "/blog",
    "/blog/codex-claude-code-one-terminal",
    "/en/blog/codex-claude-code-one-terminal",
    "/blog/plug-in-and-be-plugged-into",
    "/en/blog/plug-in-and-be-plugged-into",
    "/blog/using-deepseek-harness-in-martty",
    "/blog/connect-acp-agent-to-martty",
    "/blog/building-cordis-tui-plugins",
    "/docs/plugin-systems",
    "/docs/architecture",
    "/docs/plugins",
    "/docs/migration",
    "/en/docs/plugin-systems",
    "/en/docs/architecture",
    "/en/docs/plugins",
    "/en/docs/migration",
    "/docs/harness-management",
    "/docs/sessions",
    "/en/docs/harness-management",
    "/en/docs/sessions",
  ];
  const translated = new Map<string, { zh: string; en: string }>([
    ["/blog/codex-claude-code-one-terminal", { zh: "/blog/codex-claude-code-one-terminal", en: "/en/blog/codex-claude-code-one-terminal" }],
    ["/en/blog/codex-claude-code-one-terminal", { zh: "/blog/codex-claude-code-one-terminal", en: "/en/blog/codex-claude-code-one-terminal" }],
    ["/", { zh: "/", en: "/en" }],
    ["/en", { zh: "/", en: "/en" }],
    ["/blog/plug-in-and-be-plugged-into", { zh: "/blog/plug-in-and-be-plugged-into", en: "/en/blog/plug-in-and-be-plugged-into" }],
    ["/en/blog/plug-in-and-be-plugged-into", { zh: "/blog/plug-in-and-be-plugged-into", en: "/en/blog/plug-in-and-be-plugged-into" }],
  ]);
  for (const slug of ["plugin-systems", "architecture", "plugins", "migration", "harness-management", "sessions"]) {
    const pair = { zh: `/docs/${slug}`, en: `/en/docs/${slug}` };
    translated.set(pair.zh, pair);
    translated.set(pair.en, pair);
  }
  const urls = pages.map((path) => {
    const pair = translated.get(path);
    return pair ? `  <url>
    <loc>${origin}${path}</loc>
    <xhtml:link rel="alternate" hreflang="zh-CN" href="${origin}${pair.zh}" />
    <xhtml:link rel="alternate" hreflang="en" href="${origin}${pair.en}" />
    <xhtml:link rel="alternate" hreflang="x-default" href="${origin}${pair.en}" />
  </url>` : `  <url><loc>${origin}${path}</loc></url>`;
  }).join("\n");
  const body = `<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9" xmlns:xhtml="http://www.w3.org/1999/xhtml">
${urls}
</urlset>`;

  return new Response(body, {
    headers: { "Content-Type": "application/xml; charset=utf-8" },
  });
};
