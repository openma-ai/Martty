import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { Homepage } from "./Homepage";

describe("homepage product hierarchy", () => {
  it("presents the TUI as the sole hero product and ACP only under Other products", () => {
    render(<Homepage />);

    // Exactly one H1 on the page, and it names the TUI, not ACP.
    expect(screen.getAllByRole("heading", { level: 1 })).toHaveLength(1);
    expect(
      screen.getByRole("heading", {
        level: 1,
        name: /the deepseek harness terminal, reimagined/i,
      }),
    ).toBeInTheDocument();

    const hero = screen.getByRole("region", { name: /deepseek harness tui/i });
    expect(within(hero).getByText(/npm install -g @deepseek-ai\/dsh/i)).toBeInTheDocument();
    expect(
      within(hero).getByText(
        /dsh plugin --profile tui add @openma\/deepseek-harness-tui@latest/i,
      ),
    ).toBeInTheDocument();
    expect(within(hero).getByText(/dsh --profile tui/i)).toBeInTheDocument();
    expect(within(hero).queryByText(/dsh-tui --demo/i)).not.toBeInTheDocument();
    expect(within(hero).queryByText(/deepseek harness acp/i)).not.toBeInTheDocument();

    const otherProducts = screen.getByRole("region", { name: /other products/i });
    expect(within(otherProducts).getByText(/deepseek harness acp/i)).toBeInTheDocument();

    // ACP's link lives only in the secondary section, never the hero CTAs.
    expect(within(hero).queryByRole("link", { name: /deepseek harness acp/i })).not.toBeInTheDocument();
    expect(within(otherProducts).getByRole("link", { name: /npm/i })).toBeInTheDocument();
  });

  it("links the primary install flow to the real npm package and GitHub repo", () => {
    render(<Homepage />);
    const hero = screen.getByRole("region", { name: /deepseek harness tui/i });

    expect(within(hero).getByRole("link", { name: /view on npm/i })).toHaveAttribute(
      "href",
      "https://www.npmjs.com/package/@openma/deepseek-harness-tui",
    );
    expect(within(hero).getByRole("link", { name: /view on github/i })).toHaveAttribute(
      "href",
      "https://github.com/openma-ai/deepseek-harness-tui",
    );
  });

  it("uses the README whale SVG as the site brand instead of the placeholder mark", () => {
    const { container } = render(<Homepage />);

    expect(screen.getByRole("link", { name: /deepseek harness tui home/i })).toBeInTheDocument();
    expect(screen.getAllByText("DeepSeek Harness TUI").length).toBeGreaterThan(0);
    expect(container.querySelector(".brand__mark")).toHaveAttribute("src", "/tui-whale.svg?v=3");
    expect(container).not.toHaveTextContent("▟▙");
  });
});

describe("copy-to-clipboard interaction", () => {
  it("copies the recommended plugin install when the hero copy button is pressed", async () => {
    const user = userEvent.setup();
    const writeText = vi.spyOn(navigator.clipboard, "writeText");
    render(<Homepage />);

    const hero = screen.getByRole("region", { name: /deepseek harness tui/i });
    const copyButton = within(hero).getAllByRole("button", { name: /copy/i })[0];

    await user.click(copyButton);

    expect(writeText).toHaveBeenCalledWith(
      "npm install -g @deepseek-ai/dsh\n" +
        "dsh plugin --profile tui add @openma/deepseek-harness-tui@latest\n" +
        "dsh --profile tui",
    );
    expect(await navigator.clipboard.readText()).toBe(
      "npm install -g @deepseek-ai/dsh\n" +
        "dsh plugin --profile tui add @openma/deepseek-harness-tui@latest\n" +
        "dsh --profile tui",
    );
    expect(await within(hero).findByRole("button", { name: /copied/i })).toBeInTheDocument();
  });
});
