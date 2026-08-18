import { useEffect, useState } from "react";

const QUERY = "(prefers-reduced-motion: reduce)";

function readPreference(): boolean {
  if (typeof window === "undefined" || typeof window.matchMedia !== "function") {
    // No browser context (SSR) or no matchMedia support: default to the
    // still, calm state rather than assume motion is safe.
    return true;
  }
  return window.matchMedia(QUERY).matches;
}

/**
 * Tracks `prefers-reduced-motion`, defaulting to "reduced" when the API is
 * unavailable (SSR, old browsers, jsdom). Never throws.
 */
export function usePrefersReducedMotion(): boolean {
  const [reduced, setReduced] = useState<boolean>(() => readPreference());

  useEffect(() => {
    if (typeof window === "undefined" || typeof window.matchMedia !== "function") {
      return;
    }
    const mediaQuery = window.matchMedia(QUERY);
    const onChange = () => setReduced(mediaQuery.matches);
    onChange();
    mediaQuery.addEventListener("change", onChange);
    return () => mediaQuery.removeEventListener("change", onChange);
  }, []);

  return reduced;
}
