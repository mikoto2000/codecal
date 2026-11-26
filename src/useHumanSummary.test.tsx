import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { api } from "./api";
import { useHumanSummary } from "./useHumanSummary";
import type { HumanSummaryResponse } from "./types";

vi.mock("./api", () => ({
  api: {
    humanSummary: vi.fn(),
    humanStatus: vi.fn(),
    humanEvents: vi.fn(),
    humanSync: vi.fn(),
  },
}));
beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(api.humanStatus).mockResolvedValue({
    generation: 3,
    revision: 1,
    busy: false,
    error: null,
    last_success: 1000,
    next_auto_sync: null,
  });
  vi.mocked(api.humanEvents).mockResolvedValue(() => {});
  vi.mocked(api.humanSummary).mockResolvedValue({ days: [], diagnostics: [] });
});
const options = {
  enabled: true,
  root: "/synthetic",
  from: 0,
  to: 86_400_000,
  zone: "UTC",
  visible: ["entertainment" as const],
};
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((yes) => {
    resolve = yes;
  });
  return { promise, resolve };
}
it("keys daily summaries by kind filter and discards older filter or disabled responses", async () => {
  const old = deferred<HumanSummaryResponse>(),
    current = deferred<HumanSummaryResponse>();
  vi.mocked(api.humanSummary)
    .mockReturnValueOnce(old.promise)
    .mockReturnValueOnce(current.promise);
  const { result, rerender } = renderHook((p) => useHumanSummary(p), {
    initialProps: options,
  });
  await waitFor(() => expect(api.humanSummary).toHaveBeenCalledTimes(1));
  rerender({ ...options, visible: [] });
  await waitFor(() => expect(api.humanSummary).toHaveBeenCalledTimes(2));
  expect(api.humanSummary).toHaveBeenLastCalledWith(0, 86_400_000, "UTC", []);
  await act(async () => current.resolve({ days: [], diagnostics: [] }));
  await act(async () =>
    old.resolve({
      days: [
        {
          date: "1970-01-01",
          day_length_ms: 86_400_000,
          other_ms: 0,
          total_ms: 1000,
          kinds: { entertainment: 1000, non_entertainment: 0, unknown: 0 },
          observed: ["entertainment"],
        },
      ],
      diagnostics: [],
    }),
  );
  expect(result.current.days).toEqual([]);
  rerender({ ...options, enabled: false });
  expect(result.current.days).toEqual([]);
  expect(api.humanSummary).toHaveBeenCalledTimes(2);
});
it("retains prefilter hours during a filter request but removes them on source change or OFF", async () => {
  const response: HumanSummaryResponse = {
    days: [
      {
        date: "1970-01-01",
        day_length_ms: 86_400_000,
        other_ms: 0,
        total_ms: 1000,
        kinds: { entertainment: 1000, non_entertainment: 0, unknown: 0 },
        observed: ["entertainment"],
      },
    ],
    diagnostics: [],
  };
  const pending = deferred<HumanSummaryResponse>();
  vi.mocked(api.humanSummary)
    .mockResolvedValueOnce(response)
    .mockReturnValue(pending.promise);
  const { result, rerender } = renderHook((p) => useHumanSummary(p), {
    initialProps: options,
  });
  await waitFor(() => expect(result.current.days).toHaveLength(1));
  rerender({ ...options, visible: [] });
  expect(result.current.unfilteredDays).toEqual(response.days);
  rerender({ ...options, root: "/different-source" });
  expect(result.current.unfilteredDays).toEqual([]);
  rerender(options);
  expect(result.current.unfilteredDays).toEqual([]);
  rerender({ ...options, enabled: false });
  expect(result.current.unfilteredDays).toEqual([]);
  rerender(options);
  expect(result.current.unfilteredDays).toEqual([]);
  await act(async () => pending.resolve({ days: [], diagnostics: [] }));
});
it("discards successful prefilter days after a failed current-source fetch and does not revive them on retry", async () => {
  let changed!: Parameters<typeof api.humanEvents>[0];
  vi.mocked(api.humanEvents).mockImplementation(async (listener) => {
    changed = listener;
    return () => {};
  });
  vi.mocked(api.humanSync).mockResolvedValue(undefined);
  const response: HumanSummaryResponse = {
    days: [
      {
        date: "1970-01-01",
        day_length_ms: 86_400_000,
        other_ms: 0,
        total_ms: 1000,
        kinds: { entertainment: 1000, non_entertainment: 0, unknown: 0 },
        observed: ["entertainment"],
      },
    ],
    diagnostics: [],
  };
  const pending = deferred<HumanSummaryResponse>();
  vi.mocked(api.humanSummary)
    .mockResolvedValueOnce(response)
    .mockRejectedValueOnce("source_unavailable")
    .mockReturnValue(pending.promise);
  const { result } = renderHook(() => useHumanSummary(options));
  await waitFor(() => expect(result.current.unfilteredDays).toHaveLength(1));
  await act(async () =>
    changed({
      generation: 3,
      revision: 1,
      busy: false,
      error: "source_unavailable",
      last_success: 1000,
      next_auto_sync: null,
    }),
  );
  await waitFor(() => expect(result.current.error).toBe("source_unavailable"));
  expect(result.current.unfilteredDays).toEqual([]);
  act(() => result.current.retry());
  expect(result.current.unfilteredDays).toEqual([]);
  await act(async () => pending.resolve({ days: [], diagnostics: [] }));
});
