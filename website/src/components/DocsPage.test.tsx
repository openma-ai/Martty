import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { DocsPage } from "./DocsPage";

describe("documentation information architecture", () => {
  it("keeps static Loader plugins and dynamic Cordis plugins as separate systems", () => {
    render(<DocsPage locale="zh" slug="plugin-systems" />);

    expect(screen.getByRole("img", { name: "Martty" })).toHaveAttribute(
      "src",
      expect.stringContaining("martty-lockup.svg"),
    );
    expect(screen.getByRole("heading", { level: 1, name: "两套插件体系" })).toBeInTheDocument();

    const staticSystem = screen.getByRole("region", { name: "静态 Loader Plugin" });
    expect(within(staticSystem).getByText("/plugins")).toBeInTheDocument();
    expect(within(staticSystem).getByText(/enabled/)).toBeInTheDocument();
    expect(within(staticSystem).getByText(/fiberPhase/)).toBeInTheDocument();

    const dynamicSystem = screen.getByRole("region", { name: "动态 Cordis Plugin" });
    expect(within(dynamicSystem).getByText("/cordis-plugins")).toBeInTheDocument();
    expect(within(dynamicSystem).getByText(/start.*stop.*retract/i)).toBeInTheDocument();
    expect(within(dynamicSystem).queryByText(/enabled/)).not.toBeInTheDocument();
  });

  it("provides persistent docs navigation and a language counterpart", () => {
    render(<DocsPage locale="zh" slug="plugin-systems" />);

    expect(screen.getByRole("link", { name: "插件体系" })).toHaveAttribute("aria-current", "page");
    expect(screen.getByRole("link", { name: "架构" })).toHaveAttribute("href", "/docs/architecture");
    expect(screen.getByRole("link", { name: "插件 API" })).toHaveAttribute("href", "/docs/plugins");
    expect(screen.getByRole("link", { name: "English" })).toHaveAttribute(
      "href",
      "/en/docs/plugin-systems",
    );
  });

  it("renders repository-backed reference pages instead of the systems landing page", () => {
    render(<DocsPage locale="zh" slug="architecture" />);

    expect(screen.getByRole("heading", { level: 1, name: "架构" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { level: 1, name: "两套插件体系" })).not.toBeInTheDocument();
    expect(screen.getByRole("navigation", { name: "本页目录" })).toBeInTheDocument();
  });
});
