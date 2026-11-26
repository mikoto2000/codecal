import { useCallback, useState } from "react";
import { api } from "./api";
import { useHumanQuery } from "./useHumanQuery";
import type { HumanQueryOptions } from "./useHumanActivity";
import type {
  HumanActivityKind,
  HumanDaySummary,
  HumanSummaryResponse,
} from "./types";
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
  const scope = JSON.stringify([enabled, root, from, to, zone]);
  const [prefilter, setPrefilter] = useState<{
    scope: string;
    days: HumanDaySummary[];
  }>({ scope: "", days: [] });
  if (!enabled && prefilter.scope !== "") setPrefilter({ scope: "", days: [] });
  else if (enabled && prefilter.scope !== scope)
    setPrefilter({
      scope,
      days: !state.loading && !state.error ? response.days : [],
    });
  else if (state.error && prefilter.days.length > 0)
    setPrefilter({ scope, days: [] });
  else if (
    enabled &&
    !state.loading &&
    !state.error &&
    prefilter.days !== response.days
  )
    setPrefilter({ scope, days: response.days });
  return {
    ...state,
    days: response.days,
    unfilteredDays:
      enabled && !state.error && prefilter.scope === scope
        ? prefilter.days
        : [],
    diagnostics: response.diagnostics,
  };
}
