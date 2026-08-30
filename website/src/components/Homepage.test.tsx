import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { Homepage } from "./Homepage";

describe("homepage product hierarchy", () => {
  it("launches Martty as the first TUI with native DSH UI plugins", () => {
    render(<Homepage />);

    expect(screen.getAllByRole("heading", { level: 1 })).toHaveLength(1);
    expect(
      screen.getByRole("heading", {
        level: 1,
        name: /the first tui with native dsh ui plugins/i,
      }),
    ).toBeInTheDocument();

    const hero = screen.getByRole("region", { name: /martty plugin launch/i });
    expect(within(hero).getByText(/cordis plugins · live/i)).toBeInTheDocument();
    expect(within(hero).getByText(/web-style ui extensibility/i)).toBeInTheDocument();
    expect(within(hero).getByText(/npm install -g @deepseek-ai\/dsh/i)).toBeInTheDocument();
    expect(
      within(hero).getByText(/dsh plugin --profile martty add martty@latest/i),
    ).toBeInTheDocument();
    expect(within(hero).getByText(/dsh --profile martty/i)).toBeInTheDocument();
    expect(within(hero).queryByText(/martty --demo/i)).not.toBeInTheDocument();
    expect(within(hero).queryByText(/deepseek harness acp/i)).not.toBeInTheDocument();

    const otherProducts = screen.getByRole("region", { name: /other products/i });
    expect(within(otherProducts).getByText(/deepseek harness acp/i)).toBeInTheDocument();

    // ACP's link lives only in the secondary section, never the hero CTAs.
    expect(within(hero).queryByRole("link", { name: /deepseek harness acp/i })).not.toBeInTheDocument();
    expect(within(otherProducts).getByRole("link", { name: /npm/i })).toBeInTheDocument();
  });

  it("links the primary install flow to the real npm package and GitHub repo", () => {
    render(<Homepage />);
    const hero = screen.getByRole("region", { name: /martty plugin launch/i });

    expect(screen.getByRole("link", { name: /docs/i })).toHaveAttribute("href", "/docs");

    expect(within(hero).getByRole("link", { name: /view on npm/i })).toHaveAttribute(
      "href",
      "https://www.npmjs.com/package/martty",
    );
    expect(within(hero).getByRole("link", { name: /view on github/i })).toHaveAttribute(
      "href",
      "https://github.com/openma-ai/Martty",
    );
  });

  it("uses the Martty launch artwork without whale branding", () => {
    const { container } = render(<Homepage />);

    expect(screen.getByRole("link", { name: /martty home/i })).toBeInTheDocument();
    expect(screen.getAllByText("MARTTY").length).toBeGreaterThan(0);
    expect(screen.getByRole("img", { name: /martty cordis plugin launch/i })).toHaveAttribute(
      "src",
      "/martty-dsh-ui-plugin-live-16x9.png",
    );
    expect(container.querySelector('[src*="whale"]')).not.toBeInTheDocument();
    expect(container).not.toHaveTextContent("whale");
  });
});

describe("copy-to-clipboard interaction", () => {
  it("copies the recommended plugin install when the hero copy button is pressed", async () => {
    const user = userEvent.setup();
    const writeText = vi.spyOn(navigator.clipboard, "writeText");
    render(<Homepage />);

    const hero = screen.getByRole("region", { name: /martty plugin launch/i });
    const copyButton = within(hero).getAllByRole("button", { name: /copy/i })[0];

    await user.click(copyButton);

    expect(writeText).toHaveBeenCalledWith(
      "npm install -g @deepseek-ai/dsh\n" +
        "dsh plugin --profile martty add martty@latest\n" +
        "dsh --profile martty",
    );
    expect(await navigator.clipboard.readText()).toBe(
      "npm install -g @deepseek-ai/dsh\n" +
        "dsh plugin --profile martty add martty@latest\n" +
        "dsh --profile martty",
    );
    expect(await within(hero).findByRole("button", { name: /copied/i })).toBeInTheDocument();
  });
});
