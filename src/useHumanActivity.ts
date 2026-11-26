import { useCallback } from "react";
import { api } from "./api";
import { useHumanQuery } from "./useHumanQuery";
import type { HumanActivityResponse } from "./types";
const empty: HumanActivityResponse = { records: [], diagnostics: [] };
export type HumanQueryOptions = {
  enabled: boolean;
  root: string;
  from: number;
  to: number;
  zone: string;
};
export function useHumanActivity({
  enabled,
  root,
  from,
  to,
  zone,
  grouped = false,
  binned = false,
}: HumanQueryOptions & { grouped?: boolean; binned?: boolean }) {
  const fetchData = useCallback(
    () =>
      binned
        ? api.humanActivity(from, to, grouped, zone, true)
        : api.humanActivity(from, to, grouped, zone),
    [from, to, grouped, zone, binned],
  );
  const { response, ...state } = useHumanQuery({
    enabled,
    queryKey: JSON.stringify([enabled, root, from, to, zone, grouped, binned]),
    fetchData,
    empty,
  });
  return {
    ...state,
    records: response.records,
    bins: response.bins,
    diagnostics: response.diagnostics,
  };
}
