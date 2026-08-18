// Shared copy + links for the marketing site. Centralized so the hero, the
// profile-flow section, and tests all agree on the exact install strings
// from README.md — never re-typed ad hoc in JSX.

export const TUI_PACKAGE = "@openma/deepseek-harness-tui";
export const ACP_PACKAGE = "@openma/deepseek-harness-acp";

export const TUI_NPM_URL = `https://www.npmjs.com/package/${TUI_PACKAGE}`;
export const TUI_GITHUB_URL = "https://github.com/openma-ai/deepseek-harness-tui";
export const ACP_NPM_URL = `https://www.npmjs.com/package/${ACP_PACKAGE}`;
export const ACP_GITHUB_URL = "https://github.com/openma-ai/deepseek-harness-acp";

/** The zero-setup demo path, verbatim from README.md's "先看 Demo" / "Try the demo first". */
export const DEMO_STEPS = [
  `npm install --global ${TUI_PACKAGE}`,
  "dsh-tui --demo",
] as const;

/** The recommended dsh profile path, verbatim from README.md's "推荐：作为 dsh 的 TUI surface plugin". */
export const PROFILE_STEPS = [
  "npm install -g @deepseek-ai/dsh",
  `dsh plugin --profile tui add ${TUI_PACKAGE}@latest`,
  "dsh --profile tui",
] as const;

export interface Feature {
  index: string;
  title: string;
  body: string;
}

export const FEATURES: readonly Feature[] = [
  {
    index: "01",
    title: "Full agent timeline",
    body: "Streamed reasoning, tool calls and results, subagent lifecycles, and token/cache metrics, with a live status line for phase, elapsed time, and queue depth.",
  },
  {
    index: "02",
    title: "Native ACP capabilities",
    body: "Reads the agent's models, permissions, and authentication methods directly. Skills and built-in commands share one searchable slash menu.",
  },
  {
    index: "03",
    title: "Multi-image prompts",
    body: "Stage up to eight images from files, clipboard, or paste. Editable [image n] chips preview name, dimensions, size, and type inline.",
  },
  {
    index: "04",
    title: "Terminal-aware Markdown",
    body: "Headings, lists, quotes, fenced and inline code, emphasis, and links render cleanly, preserving CJK/Latin mixing and soft wraps.",
  },
  {
    index: "05",
    title: "Durable, long-turn sessions",
    body: "Queue follow-ups or steer the active turn. Persisted JSONL sessions resume with /new, /resume, and --session-id.",
  },
  {
    index: "06",
    title: "A terminal-native surface",
    body: "Dark/light themes, narrow layouts, native/tmux/OSC 52 clipboard, kitty image previews, and a small, composable plugin surface.",
  },
] as const;
