import type { APIRoute } from "astro";

export const GET: APIRoute = ({ site, url }) => {
  const location = `${site?.origin ?? url.origin}/`;
  const body = `<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
  <url><loc>${location}</loc></url>
</urlset>`;

  return new Response(body, {
    headers: { "Content-Type": "application/xml; charset=utf-8" },
  });
};
