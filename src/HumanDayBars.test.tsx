import { render, screen, cleanup } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { HumanDayBars } from "./HumanDayBars";
afterEach(cleanup);
it.each([23, 24, 25])(
  "scales an hour against an actual %s-hour day without stacking different kinds",
  (hours) => {
    render(
      <HumanDayBars
        visible={["entertainment", "non_entertainment"]}
        day={{
          date: "2026-11-01",
          day_length_ms: hours * 3600000,
          total_ms: 3600000,
          kinds: {
            entertainment: 3600000,
            non_entertainment: 3600000,
            unknown: 0,
          },
          observed: ["entertainment", "non_entertainment"],
        }}
      />,
    );
    for (const label of ["娯楽 推定 1h 0m", "非娯楽 推定 1h 0m"]) {
      const bar = screen.getByRole("img", { name: label });
      expect(
        parseFloat((bar.firstElementChild as HTMLElement).style.width),
      ).toBeCloseTo(100 / hours);
    }
    expect(
      screen.queryByRole("img", { name: /^不明 / }),
    ).not.toBeInTheDocument();
  },
);
