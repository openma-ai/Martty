// Shared copy + links for the marketing site. Centralized so the hero, the
// profile-flow section, and tests all agree on the exact install strings
// from README.md — never re-typed ad hoc in JSX.

export const TUI_PACKAGE = "martty";
export const ACP_PACKAGE = "@openma/deepseek-harness-acp";

export const TUI_NPM_URL = `https://www.npmjs.com/package/${TUI_PACKAGE}`;
export const TUI_GITHUB_URL = "https://github.com/openma-ai/Martty";
export const ACP_NPM_URL = `https://www.npmjs.com/package/${ACP_PACKAGE}`;
export const ACP_GITHUB_URL = "https://github.com/openma-ai/deepseek-harness-acp";

/** The zero-setup demo path, verbatim from README.md's "先看 Demo" / "Try the demo first". */
export const DEMO_STEPS = [
  `npm install --global ${TUI_PACKAGE}@latest`,
  "martty --demo",
] as const;

/** The recommended dsh profile path, verbatim from README.md's "推荐：作为 dsh 的 TUI surface plugin". */
export const PROFILE_STEPS = [
  "npm install -g @deepseek-ai/dsh",
  `dsh plugin --profile martty add ${TUI_PACKAGE}@latest`,
  "dsh --profile martty",
] as const;

export interface Feature {
  index: string;
  title: string;
  body: string;
}

export const FEATURES: readonly Feature[] = [
  {
    index: "01",
    title: "Cordis plugin runtime",
    body: "Load, inspect, update, stop, and save dynamic plugins through one observable lifecycle instead of baking every UI feature into the terminal shell.",
  },
  {
    index: "02",
    title: "Native DSH UI plugins",
    body: "Martty runs DSH UI plugin code on its own Cordis client tree while Host capabilities stay on the DSH side and communicate over negotiated ACP extensions.",
  },
  {
    index: "03",
    title: "Composable UI slots",
    body: "Plugins can contribute welcome surfaces, a persistent right rail, input context, composer stats, commands, overlays, and complete theme packs.",
  },
  {
    index: "04",
    title: "Session-aware services",
    body: "Build against structured configuration, plan, status, and usage snapshots without parsing transcript text or taking over the terminal.",
  },
  {
    index: "05",
    title: "Full agent timeline",
    body: "Streamed reasoning, tools, subagents, plans, images, token usage, and durable sessions remain native terminal interactions.",
  },
  {
    index: "06",
    title: "Safe terminal boundaries",
    body: "Plugins render validated semantic nodes. The Rust painter keeps exclusive control of the TTY, raw mode, layout, clipboard, and image protocol.",
  },
] as const;
