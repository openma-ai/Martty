export function canonicalLocation(url: URL): URL | undefined {
  const canonical = new URL(url);
  let changed = false;

  if (canonical.hostname === "martty.sh" && canonical.protocol === "http:") {
    canonical.protocol = "https:";
    changed = true;
  }
  if (canonical.pathname !== "/" && canonical.pathname.endsWith("/")) {
    canonical.pathname = canonical.pathname.replace(/\/+$/u, "");
    changed = true;
  }

  return changed ? canonical : undefined;
}

export function withSeoHeaders(response: Response): Response {
  const headers = new Headers(response.headers);
  headers.set("Strict-Transport-Security", "max-age=31536000");
  headers.set("X-Content-Type-Options", "nosniff");
  headers.set("Referrer-Policy", "strict-origin-when-cross-origin");
  return new Response(response.body, {
    status: response.status,
    statusText: response.statusText,
    headers,
  });
}

export const onRequest = async (
  { request }: { request: Request },
  next: () => Promise<Response>,
): Promise<Response> => {
  const location = canonicalLocation(new URL(request.url));
  if (location) return withSeoHeaders(Response.redirect(location, 308));
  return withSeoHeaders(await next());
};
