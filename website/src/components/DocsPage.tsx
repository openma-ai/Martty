import { getDoc } from "../data/docs";
import marttyLogoUrl from "../../../assets/martty-lockup.svg?url";

type Locale = "zh" | "en";

interface DocsPageProps {
  locale: Locale;
  slug: string;
}

const COPY = {
  zh: {
    docs: "文档",
    navLabel: "文档导航",
    systems: "插件体系",
    architecture: "架构",
    plugins: "插件 API",
    migration: "迁移状态",
    language: "English",
    languageCode: "EN",
    onThisPage: "本页目录",
    eyebrow: "PLUGIN MODEL / 01",
    title: "两套插件体系",
    lede: "Martty 明确区分静态 Loader Plugin 与动态 Cordis Plugin。它们来源不同、生命周期不同，也由不同的管理入口展示。",
    staticTitle: "静态 Loader Plugin",
    staticBody:
      "来自 Profile 与 Loader 配置，跟随 Host 或 Client 进程启动。Loader entry 使用 enabled 表达配置意图，fiberPhase 报告实际 Fiber 状态。",
    staticSurface: "只读入口",
    staticLifecycle: "生命周期",
    staticLifecycleValue: "Profile composition → Loader → Fiber",
    dynamicTitle: "动态 Cordis Plugin",
    dynamicBody:
      "由当前 Agent 的 Cordis registry 管理，通过 define/run 创建临时运行，并以 run 状态表达生命周期。",
    dynamicSurface: "运行入口",
    dynamicLifecycle: "生命周期",
    dynamicLifecycleValue: "start → stop → retract",
    boundaryTitle: "不要合并状态机",
    boundaryBody:
      "Theme、UI、Slot 与 Command 是 Plugin 挂载后注册的 contribution，不是新的 Plugin 类型。静态 contribution 可以来自常驻 composition；动态 contribution 随 run 一起撤销。",
  },
  en: {
    docs: "Docs",
    navLabel: "Documentation",
    systems: "Plugin systems",
    architecture: "Architecture",
    plugins: "Plugin API",
    migration: "Migration status",
    language: "中文",
    languageCode: "中文",
    onThisPage: "On this page",
    eyebrow: "PLUGIN MODEL / 01",
    title: "Two plugin systems",
    lede: "Martty keeps static Loader Plugins and dynamic Cordis Plugins separate. They have different sources, lifecycles, and management surfaces.",
    staticTitle: "Static Loader Plugins",
    staticBody:
      "These come from Profile and Loader configuration and start with the Host or Client process. enabled records configuration intent; fiberPhase reports the realized Fiber state.",
    staticSurface: "Read-only surface",
    staticLifecycle: "Lifecycle",
    staticLifecycleValue: "Profile composition → Loader → Fiber",
    dynamicTitle: "Dynamic Cordis Plugins",
    dynamicBody:
      "These belong to the current Agent's Cordis registry, are created through define/run, and express lifecycle through run status.",
    dynamicSurface: "Runtime surface",
    dynamicLifecycle: "Lifecycle",
    dynamicLifecycleValue: "start → stop → retract",
    boundaryTitle: "Do not merge the state machines",
    boundaryBody:
      "Themes, UI, slots, and commands are contributions registered by a mounted Plugin, not additional Plugin types. Static contributions may come from resident composition; dynamic contributions disappear with their run.",
  },
} as const;

function docsPath(locale: Locale, slug: string) {
  const prefix = locale === "zh" ? "/docs" : "/en/docs";
  return `${prefix}/${slug}`;
}

export function DocsPage({ locale, slug }: DocsPageProps) {
  const copy = COPY[locale];
  const otherLocale: Locale = locale === "zh" ? "en" : "zh";
  const referenceDoc = slug === "plugin-systems" ? undefined : getDoc(locale, slug);

  return (
    <div className="docs-shell">
      <header className="docs-header">
        <a className="docs-brand" href="/" aria-label="Martty home">
          <img src={marttyLogoUrl} alt="Martty" />
          <span>{copy.docs}</span>
        </a>
        <div className="docs-header__meta">
          <span>DSH-NATIVE AGENT TUI</span>
          <a href={docsPath(otherLocale, slug)}>{copy.language}</a>
        </div>
      </header>

      <aside className="docs-sidebar">
        <nav aria-label={copy.navLabel}>
          <p className="docs-nav__label">CORE</p>
          <a
            href={docsPath(locale, "plugin-systems")}
            aria-current={slug === "plugin-systems" ? "page" : undefined}
          >
            <span aria-hidden="true">01</span>{copy.systems}
          </a>
          <a
            href={docsPath(locale, "architecture")}
            aria-current={slug === "architecture" ? "page" : undefined}
          >
            <span aria-hidden="true">02</span>{copy.architecture}
          </a>
          <p className="docs-nav__label">REFERENCE</p>
          <a
            href={docsPath(locale, "plugins")}
            aria-current={slug === "plugins" ? "page" : undefined}
          >
            <span aria-hidden="true">03</span>{copy.plugins}
          </a>
          <a
            href={docsPath(locale, "migration")}
            aria-current={slug === "migration" ? "page" : undefined}
          >
            <span aria-hidden="true">04</span>{copy.migration}
          </a>
        </nav>
      </aside>

      <main className="docs-main" id="main">
        {referenceDoc === undefined ? <SystemsArticle copy={copy} /> : (
          <div className="docs-reference-layout">
            <article
              className="docs-article docs-markdown"
              dangerouslySetInnerHTML={{ __html: referenceDoc.html }}
            />
            <nav className="docs-toc" aria-label={copy.onThisPage}>
              <p>{copy.onThisPage}</p>
              {referenceDoc.headings
                .filter((heading) => heading.depth === 2)
                .map((heading) => (
                  <a key={heading.id} href={`#${heading.id}`}>{heading.text}</a>
                ))}
            </nav>
          </div>
        )}
      </main>
    </div>
  );
}

function SystemsArticle({ copy }: { copy: (typeof COPY)[Locale] }) {
  return (
    <article className="docs-article">
          <p className="docs-eyebrow">{copy.eyebrow}</p>
          <h1>{copy.title}</h1>
          <p className="docs-lede">{copy.lede}</p>

          <div className="plugin-system-grid">
            <section aria-labelledby="static-plugin-heading" className="plugin-system">
              <p className="plugin-system__kind">STATIC / LOADER</p>
              <h2 id="static-plugin-heading">{copy.staticTitle}</h2>
              <p>{copy.staticBody}</p>
              <dl>
                <div>
                  <dt>{copy.staticSurface}</dt>
                  <dd><code>/plugins</code></dd>
                </div>
                <div>
                  <dt>{copy.staticLifecycle}</dt>
                  <dd>{copy.staticLifecycleValue}</dd>
                </div>
              </dl>
            </section>

            <section aria-labelledby="dynamic-plugin-heading" className="plugin-system">
              <p className="plugin-system__kind">DYNAMIC / CORDIS</p>
              <h2 id="dynamic-plugin-heading">{copy.dynamicTitle}</h2>
              <p>{copy.dynamicBody}</p>
              <dl>
                <div>
                  <dt>{copy.dynamicSurface}</dt>
                  <dd><code>/cordis-plugins</code></dd>
                </div>
                <div>
                  <dt>{copy.dynamicLifecycle}</dt>
                  <dd>{copy.dynamicLifecycleValue}</dd>
                </div>
              </dl>
            </section>
          </div>

          <section className="docs-callout" aria-labelledby="boundary-heading">
            <p>BOUNDARY</p>
            <div>
              <h2 id="boundary-heading">{copy.boundaryTitle}</h2>
              <p>{copy.boundaryBody}</p>
            </div>
          </section>
    </article>
  );
}
