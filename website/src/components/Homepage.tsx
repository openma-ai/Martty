import { ACP_GITHUB_URL, ACP_NPM_URL, ACP_PACKAGE, DEMO_STEPS, FEATURES, PROFILE_STEPS, TUI_GITHUB_URL, TUI_NPM_URL } from "../data/content";
import { CopyCommand } from "./CopyCommand";

/**
 * The whole homepage as one server-renderable React component. Every word of
 * copy below is plain JSX text, so it is present in the very first byte of
 * HTML the server sends — nothing here waits on client JS to become
 * readable. The only client-side behavior lives in <CopyCommand>.
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
        <a className="brand" href="#main" aria-label="Martty home">
          <span className="brand__name">MARTTY</span>
          <span className="brand__tag">DSH-native Agent TUI</span>
        </a>
        <nav className="site-nav" aria-label="Primary">
          <a href="/docs">Docs</a>
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
    <section className="hero" aria-label="Martty plugin launch">
      <div className="wrap hero__inner">
        <div className="hero__copy">
          <p className="hero__eyebrow">
            <span className="hero__eyebrow-dot" aria-hidden="true" />
            CORDIS PLUGINS · LIVE
          </p>
          <h1 className="hero__title">
            The first TUI with <span className="hero__title-accent">native DSH UI plugins.</span>
          </h1>
          <p className="hero__lede">
            Martty brings web-style UI extensibility to the terminal. Cordis plugins can add
            themes, slots, commands, overlays, and session-aware views without taking over the TTY.
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
        <div className="hero__visual-card hero__visual-card--launch">
          <img
            className="hero__launch-image"
            src="/martty-dsh-ui-plugin-live-16x9.png"
            width="1664"
            height="936"
            alt="Martty Cordis plugin launch"
          />
        </div>
      </div>
    </section>
  );
}

function Features() {
  return (
    <section className="section" id="features" aria-labelledby="features-heading">
      <div className="wrap">
        <div className="section__head">
          <h2 className="section__title" id="features-heading">
            What Martty plugins can do
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
          Run Martty directly, or install it into a dedicated DSH profile so DSH manages the
          plugin package and upgrades. The standalone demo previews the interface without a key.
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
          <code>martty --agent dsh-acp</code>.
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
          Martty is the terminal. The project below is the protocol layer underneath it —
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
          © {year} MARTTY · MIT
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
