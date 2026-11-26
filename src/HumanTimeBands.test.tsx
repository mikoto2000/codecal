import { render, screen, fireEvent } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { HumanTimeBands } from "./HumanTimeBands";
import { buildHumanTimeBins } from "./human-time-bands";
import { humanKinds } from "./human-calendar";
it("keeps three lanes and coverage when zoomed or filtered and opens the full time slot", () => {
  const bins = buildHumanTimeBins(
    [
      { start: 0, end: 60000, kind: "entertainment" },
      { start: 0, end: 0, kind: "unknown" },
    ],
    ["1970-01-01"],
    "UTC",
  ).get("1970-01-01")!;
  const open = vi.fn();
  const { container, rerender } = render(
    <HumanTimeBands
      bins={bins}
      visible={humanKinds}
      scale={1}
      zone="UTC"
      open={open}
    />,
  );
  const button = screen.getByRole("button", { name: /^人間 娯楽 / });
  expect(container.querySelectorAll(".human-time-lane")).toHaveLength(3);
  expect(button.style.getPropertyValue("--coverage")).toBe("20%");
  expect(button.style.height).toBe("5px");
  fireEvent.click(button);
  expect(open).toHaveBeenCalledWith(bins[0]);
  rerender(
    <HumanTimeBands
      bins={bins}
      visible={["entertainment"]}
      scale={3}
      zone="UTC"
      open={open}
    />,
  );
  expect(container.querySelectorAll(".human-time-lane")).toHaveLength(3);
  expect(button.style.height).toBe("15px");
  expect(button.style.getPropertyValue("--coverage")).toBe("20%");
  expect(
    screen.queryByRole("button", { name: /^人間 不明 / }),
  ).not.toBeInTheDocument();
});
