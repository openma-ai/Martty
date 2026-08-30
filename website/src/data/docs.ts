import { Marked, Renderer, type Token, type Tokens } from "marked";

import architectureEn from "../../../docs/architecture.en.md?raw";
import architectureZh from "../../../docs/architecture.md?raw";
import migrationEn from "../../../docs/migration.en.md?raw";
import migrationZh from "../../../docs/migration.md?raw";
import pluginsEn from "../../../docs/plugins.en.md?raw";
import pluginsZh from "../../../docs/plugins.md?raw";

export type DocsLocale = "zh" | "en";
export type ReferenceDocSlug = "architecture" | "plugins" | "migration";
export type DocsSlug = "plugin-systems" | ReferenceDocSlug;

export interface DocSeo {
  title: string;
  description: string;
}

export interface DocsHeading {
  depth: number;
  id: string;
  text: string;
}

export interface RenderedDoc {
  title: string;
  html: string;
  headings: DocsHeading[];
}

const SOURCES: Record<DocsLocale, Record<ReferenceDocSlug, string>> = {
  zh: { architecture: architectureZh, plugins: pluginsZh, migration: migrationZh },
  en: { architecture: architectureEn, plugins: pluginsEn, migration: migrationEn },
};

const DOCS_SEO: Record<DocsLocale, Record<DocsSlug, DocSeo>> = {
  zh: {
    "plugin-systems": {
      title: "Martty 两套插件体系：Loader 与 Cordis — 文档",
      description: "了解 Martty 如何区分静态 Loader Plugin 与动态 Cordis Plugin，包括来源、进程归属、生命周期和对应管理入口。",
    },
    architecture: {
      title: "Martty 运行架构：Host、Client、ACP 与 Painter — 文档",
      description: "理解 Martty 的 DSH Host、独立 Client、ACP 标准管道与 Rust painter 边界，以及 Cordis 能力如何经过协议投影。",
    },
    plugins: {
      title: "Martty TUI Plugin API：主题、Slots 与生命周期",
      description: "查阅 Martty Client Plugin API，包括主题、UI、Slots、Commands、Overlay、Session 服务、Host RPC 与卸载生命周期。",
    },
    migration: {
      title: "Martty 插件化迁移状态与后续阶段 — 文档",
      description: "查看 Martty 从内置终端功能迁移到 Cordis Plugin 的当前状态、已经开放的 TUI 能力、协议边界与后续实施阶段。",
    },
  },
  en: {
    "plugin-systems": {
      title: "Martty Loader and Cordis Plugin Systems — Docs",
      description: "Compare Martty's static Loader Plugins with dynamic Cordis Plugins, including their sources, process ownership, lifecycle states, and management surfaces.",
    },
    architecture: {
      title: "Martty Host, Client, ACP, and Painter Architecture",
      description: "Trace Martty's DSH Host, independent Client process, standard ACP pipes, private Rust painter channel, and negotiated Cordis capability projection.",
    },
    plugins: {
      title: "Martty TUI Plugin API and Lifecycle — Docs",
      description: "Reference Martty's Client Plugin API for themes, UI presets, slots, commands, overlays, session services, Host RPC, validation, and lifecycle cleanup.",
    },
    migration: {
      title: "Martty Plugin Migration Status and Roadmap — Docs",
      description: "Review Martty's migration from built-in terminal features to Cordis Plugins, including completed TUI capabilities, protocol boundaries, and remaining phases.",
    },
  },
};

function tokenText(token: Token): string {
  if ("tokens" in token && Array.isArray(token.tokens)) return token.tokens.map(tokenText).join("");
  return "text" in token && typeof token.text === "string" ? token.text : "";
}

function headingId(text: string) {
  return text
    .trim()
    .toLowerCase()
    .replace(/[`'"“”‘’]/g, "")
    .replace(/[^\p{Letter}\p{Number}]+/gu, "-")
    .replace(/^-+|-+$/g, "");
}

function escapeAttribute(value: string) {
  return value.replace(/&/g, "&amp;").replace(/"/g, "&quot;");
}

function docsHref(href: string, locale: DocsLocale) {
  if (/^(?:[a-z]+:|\/|#)/i.test(href)) return href;

  const [path, suffix = ""] = href.split(/(?=[?#])/u, 2);
  const filename = path.split("/").at(-1)?.replace(/\.en(?=\.md$)/u, "") ?? path;
  const referenceRoutes: Record<string, ReferenceDocSlug> = {
    "architecture.md": "architecture",
    "plugins.md": "plugins",
    "migration.md": "migration",
  };
  const slug = referenceRoutes[filename];
  if (slug) return `${locale === "en" ? "/en" : ""}/docs/${slug}${suffix}`;

  const repositoryPath = new URL(path, "https://github.com/openma-ai/Martty/blob/main/docs/").pathname;
  return `https://github.com${repositoryPath}${suffix}`;
}

function renderMarkdown(source: string, locale: DocsLocale): RenderedDoc {
  const headings: DocsHeading[] = [];
  const seen = new Map<string, number>();
  const renderer = new Renderer();

  renderer.heading = function ({ tokens, depth }: Tokens.Heading) {
    const text = tokens.map(tokenText).join("").trim();
    const base = headingId(text) || "section";
    const count = seen.get(base) ?? 0;
    seen.set(base, count + 1);
    const id = count === 0 ? base : `${base}-${count + 1}`;
    headings.push({ depth, id, text });
    return `<h${depth} id="${id}">${this.parser.parseInline(tokens)}</h${depth}>\n`;
  };

  renderer.link = function ({ href, title, tokens }: Tokens.Link) {
    const target = docsHref(href, locale);
    const titleAttribute = title ? ` title="${escapeAttribute(title)}"` : "";
    return `<a href="${escapeAttribute(target)}"${titleAttribute}>${this.parser.parseInline(tokens)}</a>`;
  };

  const html = new Marked({ gfm: true, renderer }).parse(source) as string;
  const title = headings.find((heading) => heading.depth === 1)?.text ?? "Martty Docs";
  return {
    title,
    html,
    headings: headings.filter((heading) => heading.depth === 2 || heading.depth === 3),
  };
}

const DOCS = Object.fromEntries(
  Object.entries(SOURCES).map(([locale, pages]) => [
    locale,
    Object.fromEntries(
      Object.entries(pages).map(([slug, source]) => [slug, renderMarkdown(source, locale as DocsLocale)]),
    ),
  ]),
) as Record<DocsLocale, Record<ReferenceDocSlug, RenderedDoc>>;

export function getDoc(locale: DocsLocale, slug: string): RenderedDoc | undefined {
  if (!(slug in DOCS[locale])) return undefined;
  return DOCS[locale][slug as ReferenceDocSlug];
}

export function getDocSeo(locale: DocsLocale, slug: string): DocSeo | undefined {
  if (!(slug in DOCS_SEO[locale])) return undefined;
  return DOCS_SEO[locale][slug as DocsSlug];
}
