import { lazy, Suspense, useEffect, useState } from "react";

import { usePrefersReducedMotion } from "../lib/usePrefersReducedMotion";

// Lazy + Suspense, same loading shape as the reference component in
// ../../vendor/21st-dev/hero-dithering-card/component.tsx — but this file
// swaps its orange accent + card chrome for a single monochrome electric-blue
// texture and adds a real prefers-reduced-motion gate.
const Dithering = lazy(() =>
  import("@paper-design/shaders-react").then((mod) => ({ default: mod.Dithering })),
);

const ACCENT = "#3aa0ff";

/**
 * A faint, quantized dithering texture behind the hero. Purely decorative:
 * aria-hidden, pointer-events: none, never renders on the server, and stays
 * still when the visitor prefers reduced motion.
 */
export function SignalField() {
  const reducedMotion = usePrefersReducedMotion();
  const [mounted, setMounted] = useState(false);

  useEffect(() => {
    setMounted(true);
  }, []);

  if (!mounted) {
    // Server-render (and the pre-hydration frame) ship nothing here: the
    // hero's copy and layout never depend on this texture existing.
    return null;
  }

  return (
    <div className="signal-field" aria-hidden="true">
      <Suspense fallback={null}>
        <Dithering
          colorBack="#00000000"
          colorFront={ACCENT}
          shape="dots"
          type="4x4"
          speed={reducedMotion ? 0 : 0.25}
          minPixelRatio={1}
          style={{ width: "100%", height: "100%" }}
        />
      </Suspense>
    </div>
  );
}
