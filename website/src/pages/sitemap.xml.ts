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
    "/blog/plug-in-and-be-plugged-into",
    "/blog/using-deepseek-harness-in-martty",
    "/blog/connect-acp-agent-to-martty",
    "/blog/building-cordis-tui-plugins",
  ];
  const urls = pages.map((path) => path === "/" || path === "/en" ? `  <url>
    <loc>${origin}${path}</loc>
    <xhtml:link rel="alternate" hreflang="zh-CN" href="${origin}/" />
    <xhtml:link rel="alternate" hreflang="en" href="${origin}/en" />
    <xhtml:link rel="alternate" hreflang="x-default" href="${origin}/en" />
  </url>` : `  <url><loc>${origin}${path}</loc></url>`).join("\n");
  const body = `<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9" xmlns:xhtml="http://www.w3.org/1999/xhtml">
${urls}
</urlset>`;

  return new Response(body, {
    headers: { "Content-Type": "application/xml; charset=utf-8" },
  });
};
