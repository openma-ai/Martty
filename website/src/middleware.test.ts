import { describe, expect, it } from "vitest";

import { canonicalLocation, withSeoHeaders } from "./middleware";

describe("site URL canonicalization", () => {
  it("redirects HTTP and removes a non-root trailing slash in one hop", () => {
    expect(canonicalLocation(new URL("http://martty.sh/en/?source=test"))?.href)
      .toBe("https://martty.sh/en?source=test");
  });

  it("keeps the canonical root and canonical HTTPS paths unchanged", () => {
    expect(canonicalLocation(new URL("https://martty.sh/"))).toBeUndefined();
    expect(canonicalLocation(new URL("https://martty.sh/docs/plugins"))).toBeUndefined();
  });

  it("does not force HTTPS on a local development host", () => {
    expect(canonicalLocation(new URL("http://127.0.0.1:4321/"))).toBeUndefined();
    expect(canonicalLocation(new URL("http://127.0.0.1:4321/en/"))?.href)
      .toBe("http://127.0.0.1:4321/en");
  });

  it("adds stable transport and MIME-sniffing headers", () => {
    const response = withSeoHeaders(new Response("ok", { status: 200 }));
    expect(response.headers.get("strict-transport-security")).toBe("max-age=31536000");
    expect(response.headers.get("x-content-type-options")).toBe("nosniff");
    expect(response.headers.get("referrer-policy")).toBe("strict-origin-when-cross-origin");
  });
});
