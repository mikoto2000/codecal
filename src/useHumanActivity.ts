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
}: HumanQueryOptions & { grouped?: boolean }) {
  const fetchData = useCallback(
    () => api.humanActivity(from, to, grouped, zone),
    [from, to, grouped, zone],
  );
  const { response, ...state } = useHumanQuery({
    enabled,
    queryKey: JSON.stringify([enabled, root, from, to, zone, grouped]),
    fetchData,
    empty,
  });
  return {
    ...state,
    records: response.records,
    diagnostics: response.diagnostics,
  };
}
