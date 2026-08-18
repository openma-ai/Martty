import { ACP_GITHUB_URL, ACP_NPM_URL, ACP_PACKAGE, DEMO_STEPS, FEATURES, PROFILE_STEPS, TUI_GITHUB_URL, TUI_NPM_URL } from "../data/content";
import { WHALE_LG } from "../data/whale";
import { CopyCommand } from "./CopyCommand";
import { SignalField } from "./SignalField";

/**
 * The whole homepage as one server-renderable React component. Every word of
 * copy below is plain JSX text, so it is present in the very first byte of
 * HTML the server sends — nothing here waits on client JS to become
 * readable. The only client-side behavior lives in <CopyCommand> (clipboard)
 * and <SignalField> (a decorative, reduced-motion-aware background texture).
 */
export function Homepage() {
  return (
    <>
      <a href="#main" className="skip-link">
        Skip to content
      </a>
      <div className="shell">
        <SiteHeader />
        <main id="main">
          <Hero />
          <Features />
          <ProfileFlow />
          <OtherProducts />
        </main>
        <SiteFooter />
      </div>
    </>
  );
}

function SiteHeader() {
  return (
    <header className="site-header">
      <div className="wrap site-header__row">
        <a className="brand" href="#main" aria-label="DeepSeek Harness TUI home">
          <img className="brand__mark" src="/tui-whale.svg?v=3" width="48" height="36" alt="" />
          <span className="brand__name">DeepSeek Harness TUI</span>
          <span className="brand__tag">Terminal-native agent interface</span>
        </a>
        <nav className="site-nav" aria-label="Primary">
          <a href="#features">Features</a>
          <a href="#profile">Install</a>
          <a href="#other-products">Other products</a>
          <a href={TUI_GITHUB_URL} className="site-nav__accent">
            GitHub
          </a>
        </nav>
      </div>
    </header>
  );
}

function Hero() {
  return (
    <section className="hero" aria-label="DeepSeek Harness TUI">
      <SignalField />
      <div className="wrap hero__inner">
        <div>
          <p className="hero__eyebrow">
            <span className="hero__eyebrow-dot" aria-hidden="true" />
            dsh-tui · terminal-native ACP client
          </p>
          <h1 className="hero__title">
            The DeepSeek Harness terminal, <span className="hero__title-accent">reimagined</span>
          </h1>
          <p className="hero__lede">
            A Rust terminal on a Cordis client tree: streamed reasoning, tool calls, subagents,
            token usage, and durable sessions — plus a small plugin surface for themes, commands,
            and views instead of a growing pile of built-in features.
          </p>
          <div className="hero__ctas">
            <a className="btn btn--accent" href="#profile">
              Install with dsh
            </a>
            <a className="btn" href={TUI_GITHUB_URL}>
              View on GitHub
            </a>
            <a className="btn" href={TUI_NPM_URL}>
              View on npm
            </a>
          </div>
          <CopyCommand lines={PROFILE_STEPS} label="Recommended dsh profile install command" />
        </div>
        <Whale />
      </div>
    </section>
  );
}

function Whale() {
  return (
    <div className="hero__whale-wrap">
      <pre className="hero__whale" aria-hidden="true">
        {WHALE_LG.join("\n")}
      </pre>
      <span className="sr-only">DeepSeek Harness whale mark, rendered as quantized block ASCII.</span>
      <p className="hero__whale-caption">src/logo_data.rs · block ascii</p>
    </div>
  );
}

function Features() {
  return (
    <section className="section" id="features" aria-labelledby="features-heading">
      <div className="wrap">
        <div className="section__head">
          <h2 className="section__title" id="features-heading">
            What dsh-tui does
          </h2>
          <span className="section__index">01 / 06 — 06 / 06</span>
        </div>
        <div className="feature-log">
          {FEATURES.map((feature) => (
            <div className="feature-row" key={feature.index}>
              <span className="feature-row__index" aria-hidden="true">
                {feature.index}
              </span>
              <span className="feature-row__title">{feature.title}</span>
              <span className="feature-row__body">{feature.body}</span>
            </div>
          ))}
        </div>
      </div>
    </section>
  );
}

function ProfileFlow() {
  return (
    <section className="section" id="profile" aria-labelledby="profile-heading">
      <div className="wrap">
        <div className="section__head">
          <h2 className="section__title" id="profile-heading">
            Two ways to run it
          </h2>
        </div>
        <p className="section__lede">
          Install dsh-tui as a profile plugin for normal use. The standalone demo is available
          only when you want to preview the interface without a runtime or API key.
        </p>
        <div className="flow">
          <div>
            <h3 className="feature-row__title" style={{ display: "block", marginBottom: "0.75rem" }}>
              Recommended: dsh profile plugin
            </h3>
            <CopyCommand lines={PROFILE_STEPS} label="Recommended dsh profile install command, repeated" />
          </div>
          <div>
            <h3 className="feature-row__title" style={{ display: "block", marginBottom: "0.75rem" }}>
              Optional: standalone demo
            </h3>
            <CopyCommand lines={DEMO_STEPS} label="Demo install command" />
          </div>
        </div>
        <p className="flow__note" style={{ marginTop: "1.5rem" }}>
          The Host process mounts the ACP plugin on its Base Cordis tree; a separate TUI Client
          process speaks ACP to it over standard stdin/stdout and never spawns a second agent. The
          standalone entry point can instead attach to any ACP agent directly, e.g.{" "}
          <code>dsh-tui --agent dsh-acp</code>.
        </p>
      </div>
    </section>
  );
}

function OtherProducts() {
  return (
    <section className="section other-products" id="other-products" aria-label="Other products">
      <div className="wrap">
        <div className="section__head">
          <h2 className="section__title">Other products</h2>
          <span className="section__index">secondary</span>
        </div>
        <p className="section__lede">
          dsh-tui is the terminal. The projects below are the protocol layer underneath it —
          useful if you are building your own ACP client or agent, not required to run the TUI.
        </p>
        <div className="product-row">
          <span className="product-row__name">DeepSeek Harness ACP</span>
          <span className="product-row__body">
            {ACP_PACKAGE} adapts DeepSeek Harness to Agent Client Protocol: session lifecycle,
            permissions, and authentication, independent of any one terminal.
          </span>
          <span className="product-row__links">
            <a href={ACP_NPM_URL}>npm</a>
            <a href={ACP_GITHUB_URL}>GitHub</a>
          </span>
        </div>
      </div>
    </section>
  );
}

function SiteFooter() {
  const year = new Date().getFullYear();
  return (
    <footer className="site-footer">
      <div className="wrap site-footer__row">
        <span>
          © {year} DeepSeek Harness TUI · MIT
        </span>
        <nav className="site-footer__links" aria-label="Footer">
          <a href={TUI_NPM_URL}>npm</a>
          <a href={TUI_GITHUB_URL}>GitHub</a>
          <a href="#other-products">Other products</a>
        </nav>
      </div>
    </footer>
  );
}
