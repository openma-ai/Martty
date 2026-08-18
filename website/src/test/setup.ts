import "@testing-library/jest-dom/vitest";

import { cleanup } from "@testing-library/react";
import { afterEach } from "vitest";

// Vitest is not configured with `globals: true`, so @testing-library/react's
// automatic cleanup (which hooks into a global `afterEach`) never registers.
// Unmount explicitly so each test starts from an empty document.
afterEach(() => {
  cleanup();
});
