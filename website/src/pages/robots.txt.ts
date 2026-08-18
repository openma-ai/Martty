import type { APIRoute } from "astro";

export const GET: APIRoute = ({ site, url }) =>
  new Response(`User-agent: *\nAllow: /\nSitemap: ${site?.origin ?? url.origin}/sitemap.xml\n`, {
    headers: { "Content-Type": "text/plain; charset=utf-8" },
  });
