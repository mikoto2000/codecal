import { useCallback } from "react";
import { api } from "./api";
import { useHumanQuery } from "./useHumanQuery";
import type { HumanQueryOptions } from "./useHumanActivity";
import type { HumanActivityKind, HumanSummaryResponse } from "./types";
const empty: HumanSummaryResponse = { days: [], diagnostics: [] };
export function useHumanSummary({
  enabled,
  root,
  from,
  to,
  zone,
  visible,
}: HumanQueryOptions & { visible: HumanActivityKind[] }) {
  const kinds = JSON.stringify([...new Set(visible)].sort());
  const fetchData = useCallback(
    () =>
      api.humanSummary(
        from,
        to,
        zone,
        JSON.parse(kinds) as HumanActivityKind[],
      ),
    [from, to, zone, kinds],
  );
  const { response, ...state } = useHumanQuery({
    enabled,
    queryKey: JSON.stringify([enabled, root, from, to, zone, kinds]),
    fetchData,
    empty,
  });
  return { ...state, days: response.days, diagnostics: response.diagnostics };
}
