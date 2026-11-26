import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "./api";
import type { HumanSyncStatus } from "./types";

const idle: HumanSyncStatus = {
  generation: 0,
  busy: false,
  revision: 0,
  error: null,
  last_success: null,
};
const allowedErrors = new Set([
  "human_activity_disabled",
  "invalid_period",
  "invalid_timezone",
  "period_too_dense",
  "source_unavailable",
  "source_replaced",
  "unsupported_source",
  "source_read_failed",
  "staging_failed",
  "sync_cancelled",
  "cache_read_failed",
  "cache_write_failed",
  "cache_open_failed",
]);
export function humanErrorCode(error: unknown) {
  return typeof error === "string" && allowedErrors.has(error)
    ? error
    : "human_request_failed";
}
export function useHumanQuery<T>({
  enabled,
  queryKey,
  fetchData,
  empty,
}: {
  enabled: boolean;
  queryKey: string;
  fetchData: () => Promise<T>;
  empty: T;
}) {
  const [attempt, setAttempt] = useState(0);
  const manualAttempt = useRef(0);
  const [context, setContext] = useState({ queryKey, serial: 0 });
  if (context.queryKey !== queryKey) {
    setContext({ queryKey, serial: context.serial + 1 });
  }
  const key = JSON.stringify([queryKey, context.serial, attempt]);
  const [view, setView] = useState({
    key: "",
    response: empty,
    status: idle,
    loading: false,
    error: "",
  });
  // OFF must also forget the old ON key, so a later ON cannot revive it.
  if (!enabled && view.key !== "") {
    setView({
      key: "",
      response: empty,
      status: idle,
      loading: false,
      error: "",
    });
  }
  useEffect(() => {
    if (!enabled) return;
    let alive = true,
      request = 0;
    let expected: number | null = null,
      revision = -1;
    let pending: HumanSyncStatus | null = null;
    let unsubscribe: (() => void) | undefined;
    const fetchPeriod = async () => {
      const n = ++request;
      if (alive)
        setView((v) => ({
          ...v,
          key,
          response: v.key === key ? v.response : empty,
          loading: true,
          error: "",
        }));
      try {
        if (attempt > manualAttempt.current) {
          manualAttempt.current = attempt;
          await api.humanSync();
          if (!alive) return;
        }
        const response = await fetchData();
        if (alive && n === request)
          setView((v) => ({ ...v, key, response, loading: false, error: "" }));
      } catch (error) {
        if (alive && n === request)
          setView((v) => ({
            ...v,
            key,
            response: empty,
            loading: false,
            error: humanErrorCode(error),
          }));
      }
    };
    const changed = (status: HumanSyncStatus) => {
      if (!alive) return;
      if (expected === null) {
        if (
          !pending ||
          status.generation > pending.generation ||
          (status.generation === pending.generation &&
            status.revision >= pending.revision)
        )
          pending = status;
        return;
      }
      if (status.generation !== expected || status.revision < revision) return;
      setView((v) => ({
        ...v,
        key,
        response: v.key === key ? v.response : empty,
        error: v.key === key ? v.error : "",
        status: {
          ...status,
          error: status.error ? humanErrorCode(status.error) : null,
        },
      }));
      if (
        revision !== status.revision ||
        (!status.busy && status.error !== null)
      ) {
        revision = status.revision;
        void fetchPeriod();
      }
    };
    const initialize = async () => {
      try {
        const off = await api.humanEvents(changed);
        if (!alive) {
          off();
          return;
        }
        unsubscribe = off;
        const current = await api.humanStatus();
        if (!alive) return;
        expected = current.generation;
        const buffered = pending as HumanSyncStatus | null;
        changed(
          buffered?.generation === expected &&
            buffered.revision >= current.revision
            ? buffered
            : current,
        );
      } catch (error) {
        if (alive)
          setView({
            key,
            response: empty,
            status: idle,
            loading: false,
            error: humanErrorCode(error),
          });
      }
    };
    void initialize();
    return () => {
      alive = false;
      ++request;
      unsubscribe?.();
    };
  }, [enabled, key, attempt, fetchData, empty]);
  const retry = useCallback(() => {
    if (!enabled) return;
    setAttempt((n) => n + 1);
  }, [enabled]);
  const current =
    enabled && view.key === key
      ? view
      : { response: empty, status: idle, loading: enabled, error: "" };
  return {
    response: current.response,
    status: current.status,
    loading: current.loading,
    error: current.error,
    retry,
  };
}
