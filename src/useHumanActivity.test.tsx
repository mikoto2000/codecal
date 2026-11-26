import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { api } from "./api";
import { useHumanActivity } from "./useHumanActivity";
import type { HumanActivityResponse, HumanSyncStatus } from "./types";
vi.mock("./api", () => ({
  api: {
    humanActivity: vi.fn(),
    humanStatus: vi.fn(),
    humanEvents: vi.fn(),
    humanSync: vi.fn(),
  },
}));
const status: HumanSyncStatus = {
  generation: 2,
  busy: false,
  revision: 1,
  error: null,
  last_success: 1000,
};
const empty: HumanActivityResponse = { records: [], diagnostics: [] };
const listeners: ((status: HumanSyncStatus) => void)[] = [];
beforeEach(() => {
  vi.resetAllMocks();
  listeners.length = 0;
  vi.mocked(api.humanStatus).mockResolvedValue(status);
  vi.mocked(api.humanActivity).mockResolvedValue(empty);
  vi.mocked(api.humanSync).mockResolvedValue(undefined);
  vi.mocked(api.humanEvents).mockImplementation(async (listener) => {
    listeners.push(listener);
    return () => {};
  });
});
const options = {
  enabled: true,
  root: "/synthetic-one",
  from: 0,
  to: 1000,
  zone: "UTC",
};
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

it("does not fetch periods, status or events while disabled", async () => {
  const { result } = renderHook(() =>
    useHumanActivity({ ...options, enabled: false }),
  );
  await act(async () => {});
  expect(result.current.records).toEqual([]);
  expect(api.humanActivity).not.toHaveBeenCalled();
  expect(api.humanStatus).not.toHaveBeenCalled();
  expect(api.humanEvents).not.toHaveBeenCalled();
});
it("does not revive a previous enabled response during a later ON transition", async () => {
  const late = deferred<HumanActivityResponse>();
  vi.mocked(api.humanActivity)
    .mockResolvedValueOnce({
      records: [{ start: 10, end: 20, kind: "unknown" }],
      diagnostics: [],
    })
    .mockReturnValueOnce(late.promise);
  const { result, rerender } = renderHook((p) => useHumanActivity(p), {
    initialProps: options,
  });
  await waitFor(() => expect(result.current.records).toHaveLength(1));
  rerender({ ...options, enabled: false });
  expect(result.current.records).toEqual([]);
  rerender({ ...options, enabled: true });
  expect(result.current.records).toEqual([]);
  await act(async () => late.resolve(empty));
});
it("does not revive an old source after returning before event registration finishes", async () => {
  const pending = deferred<() => void>();
  vi.mocked(api.humanEvents)
    .mockResolvedValueOnce(() => {})
    .mockReturnValue(pending.promise);
  vi.mocked(api.humanActivity).mockResolvedValue({
    records: [{ start: 10, end: 20, kind: "unknown" }],
    diagnostics: [],
  });
  const { result, rerender } = renderHook((p) => useHumanActivity(p), {
    initialProps: options,
  });
  await waitFor(() => expect(result.current.records).toHaveLength(1));
  rerender({ ...options, root: "/other" });
  expect(result.current.records).toEqual([]);
  rerender(options);
  expect(result.current.records).toEqual([]);
  await act(async () => pending.resolve(() => {}));
});

it("discards older source responses and immediately hides data on OFF", async () => {
  const first = deferred<HumanActivityResponse>(),
    second = deferred<HumanActivityResponse>();
  vi.mocked(api.humanActivity)
    .mockReturnValueOnce(first.promise)
    .mockReturnValueOnce(second.promise);
  const { result, rerender } = renderHook((p) => useHumanActivity(p), {
    initialProps: options,
  });
  await waitFor(() => expect(api.humanActivity).toHaveBeenCalledTimes(1));
  rerender({ ...options, root: "/synthetic-two" });
  await waitFor(() => expect(api.humanActivity).toHaveBeenCalledTimes(2));
  const record = { start: 10, end: 20, kind: "entertainment" as const };
  await act(async () => second.resolve({ records: [record], diagnostics: [] }));
  await act(async () =>
    first.resolve({
      records: [{ start: 1, end: 2, kind: "unknown" }],
      diagnostics: [],
    }),
  );
  expect(result.current.records).toEqual([record]);
  rerender({ ...options, root: "/synthetic-two", enabled: false });
  expect(result.current.records).toEqual([]);
  expect(api.humanActivity).toHaveBeenCalledTimes(2);
});

it("rejects delayed status events from an older settings generation", async () => {
  vi.mocked(api.humanStatus)
    .mockResolvedValueOnce(status)
    .mockResolvedValueOnce({ ...status, generation: 3 });
  const { rerender } = renderHook((p) => useHumanActivity(p), {
    initialProps: options,
  });
  await waitFor(() => expect(api.humanActivity).toHaveBeenCalledTimes(1));
  rerender({ ...options, root: "/synthetic-two" });
  await waitFor(() => expect(api.humanActivity).toHaveBeenCalledTimes(2));
  await act(async () => {
    listeners[0]({ ...status, revision: 99 });
    listeners[1]({ ...status, revision: 99 });
  });
  expect(api.humanActivity).toHaveBeenCalledTimes(2);
  await act(async () =>
    listeners[1]({ ...status, generation: 3, revision: 2 }),
  );
  await waitFor(() => expect(api.humanActivity).toHaveBeenCalledTimes(3));
});

it("clears a failed period and returns only a finite error code", async () => {
  vi.mocked(api.humanActivity)
    .mockResolvedValueOnce({
      records: [{ start: 1, end: 2, kind: "unknown" }],
      diagnostics: [],
    })
    .mockRejectedValueOnce("PRIVATE_HOOK_ERROR_MARKER");
  const { result, rerender } = renderHook((p) => useHumanActivity(p), {
    initialProps: options,
  });
  await waitFor(() => expect(result.current.records).toHaveLength(1));
  rerender({ ...options, from: 1000, to: 2000, zone: "Asia/Tokyo" });
  expect(result.current.records).toEqual([]);
  await waitFor(() =>
    expect(result.current.error).toBe("human_request_failed"),
  );
  expect(JSON.stringify(result.current)).not.toContain(
    "PRIVATE_HOOK_ERROR_MARKER",
  );
});
it("reports a failed manual sync without exposing its error value", async () => {
  const { result } = renderHook(() => useHumanActivity(options));
  await waitFor(() => expect(api.humanActivity).toHaveBeenCalledTimes(1));
  vi.mocked(api.humanSync).mockRejectedValueOnce("PRIVATE_MANUAL_ERROR");
  await act(async () => result.current.retry());
  await waitFor(() =>
    expect(result.current.error).toBe("human_request_failed"),
  );
  expect(JSON.stringify(result.current)).not.toContain("PRIVATE_MANUAL_ERROR");
});
it("keeps another period hidden while the new response is pending", async () => {
  const later = deferred<HumanActivityResponse>();
  vi.mocked(api.humanActivity)
    .mockResolvedValueOnce({
      records: [{ start: 1, end: 2, kind: "unknown" }],
      diagnostics: [],
    })
    .mockReturnValueOnce(later.promise);
  const { result, rerender } = renderHook((p) => useHumanActivity(p), {
    initialProps: options,
  });
  await waitFor(() => expect(result.current.records).toHaveLength(1));
  rerender({ ...options, from: 1000, to: 2000 });
  await waitFor(() => expect(api.humanActivity).toHaveBeenCalledTimes(2));
  expect(result.current.records).toEqual([]);
  await act(async () => later.resolve(empty));
});
it("requests grouping in the configured timezone and refetches on display-unit changes", async () => {
  const { rerender } = renderHook(
    ({ grouped }) => useHumanActivity({ ...options, grouped }),
    { initialProps: { grouped: true } },
  );
  await waitFor(() => expect(api.humanActivity).toHaveBeenCalledTimes(1));
  expect(api.humanActivity).toHaveBeenCalledWith(0, 1000, true, "UTC");
  rerender({ grouped: false });
  await waitFor(() => expect(api.humanActivity).toHaveBeenCalledTimes(2));
  expect(api.humanActivity).toHaveBeenLastCalledWith(0, 1000, false, "UTC");
});
it("rechecks the selected physical source after a failed sync even when the revision is unchanged", async () => {
  vi.mocked(api.humanActivity)
    .mockResolvedValueOnce({
      records: [{ start: 1, end: 2, kind: "unknown" }],
      diagnostics: [],
    })
    .mockResolvedValue(empty);
  const { result } = renderHook(() => useHumanActivity(options));
  await waitFor(() => expect(result.current.records).toHaveLength(1));
  await act(async () =>
    listeners[0]({ ...status, error: "unsupported_source" }),
  );
  expect(api.humanActivity).toHaveBeenCalledTimes(2);
  await waitFor(() => expect(result.current.records).toEqual([]));
});
