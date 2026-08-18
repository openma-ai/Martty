import cloudflare from "@astrojs/cloudflare";
import react from "@astrojs/react";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "astro/config";

export default defineConfig({
  site: "https://dshsuite.dev",
  output: "server",
  adapter: cloudflare({ imageService: "passthrough" }),
  integrations: [react()],
  vite: {
    plugins: [tailwindcss()],
    // Astro's SSR optimizer and hydrated islands must share the same React
    // singleton. Without deduplication, a cold dev start can optimize a
    // second copy and fail every hook-backed island at runtime.
    resolve: {
      dedupe: ["react", "react-dom"],
    },
  },
});
