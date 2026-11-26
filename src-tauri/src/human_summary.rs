//! Bounded daily summaries of minimized observations.
use crate::rei_human::{HumanActivity, HumanError, HumanKind};
use chrono::{DateTime, Duration, TimeZone};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

const KINDS: [HumanKind; 3] = [
    HumanKind::Entertainment,
    HumanKind::NonEntertainment,
    HumanKind::Unknown,
];

#[derive(Serialize)]
pub struct HumanDaySummary {
    pub date: String,
    pub day_length_ms: i64,
    pub total_ms: i64,
    pub kinds: BTreeMap<HumanKind, i64>,
    pub observed: BTreeSet<HumanKind>,
}
#[derive(Default)]
struct Union {
    end: Option<i64>,
    total: i64,
}
impl Union {
    fn add(&mut self, start: i64, end: i64) {
        if end > start && self.end.is_none_or(|prior| end > prior) {
            self.total += end - self.end.map_or(start, |prior| start.max(prior));
            self.end = Some(end);
        }
    }
}
struct Day {
    start: i64,
    end: i64,
    summary: HumanDaySummary,
    unions: [Union; 4],
}
/// Records must arrive in nondecreasing start order. Storage streams its index.
pub(crate) struct Accumulator {
    days: Vec<Day>,
    visible: BTreeSet<HumanKind>,
}
impl Accumulator {
    pub(crate) fn new(
        from: i64,
        to: i64,
        timezone: &str,
        visible: &[HumanKind],
    ) -> Result<Self, HumanError> {
        crate::rei_human::validate_period(from, to)?;
        let tz: chrono_tz::Tz = timezone
            .parse()
            .map_err(|_| HumanError("invalid_timezone"))?;
        let mut days = vec![];
        let mut start = from;
        while start < to {
            let local = DateTime::from_timestamp_millis(start)
                .ok_or(HumanError("invalid_period"))?
                .with_timezone(&tz);
            let midnight = local
                .date_naive()
                .and_hms_opt(0, 0, 0)
                .ok_or(HumanError("invalid_period"))?;
            let day_start = (0..=180)
                .find_map(|minute| {
                    tz.from_local_datetime(&(midnight + Duration::minutes(minute)))
                        .earliest()
                })
                .ok_or(HumanError("invalid_timezone"))?
                .timestamp_millis();
            let boundary = crate::activity::next_day_boundary(&local)
                .map_err(|_| HumanError("invalid_period"))?;
            let end = boundary.min(to);
            days.push(Day {
                start,
                end,
                summary: HumanDaySummary {
                    date: local.format("%Y-%m-%d").to_string(),
                    day_length_ms: boundary - day_start,
                    total_ms: 0,
                    kinds: KINDS.into_iter().map(|k| (k, 0)).collect(),
                    observed: BTreeSet::new(),
                },
                unions: std::array::from_fn(|_| Union::default()),
            });
            start = end;
        }
        Ok(Self {
            days,
            visible: visible.iter().copied().collect(),
        })
    }
    pub(crate) fn add(&mut self, record: HumanActivity) -> Result<(), HumanError> {
        let point = record.start == record.end;
        let first = self.days.partition_point(|day| day.end <= record.start);
        for day in &mut self.days[first..] {
            if if point {
                day.start > record.start
            } else {
                day.start >= record.end
            } {
                break;
            }
            let start = record.start.max(day.start);
            let end = record.end.min(day.end);
            day.summary.observed.insert(record.kind);
            let index = KINDS
                .iter()
                .position(|k| *k == record.kind)
                .expect("all normalized kinds");
            day.unions[index].add(start, end);
            if self.visible.contains(&record.kind) {
                day.unions[3].add(start, end);
            }
        }
        Ok(())
    }
    pub(crate) fn finish(self) -> Vec<HumanDaySummary> {
        self.days
            .into_iter()
            .filter(|day| !day.summary.observed.is_empty())
            .map(|mut day| {
                for (i, kind) in KINDS.into_iter().enumerate() {
                    day.summary.kinds.insert(kind, day.unions[i].total);
                }
                day.summary.total_ms = day.unions[3].total;
                day.summary
            })
            .collect()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::rei_human::{HumanActivity, HumanKind};
    #[test]
    fn overlapping_kinds_have_independent_unions_and_filtered_total() {
        let mut summary = Accumulator::new(
            0,
            86_400_000,
            "UTC",
            &[HumanKind::Entertainment, HumanKind::Unknown],
        )
        .unwrap();
        for (start, end, kind) in [
            (0, 6000, HumanKind::Entertainment),
            (2000, 8000, HumanKind::Entertainment),
            (3000, 10000, HumanKind::NonEntertainment),
            (12000, 12000, HumanKind::Unknown),
        ] {
            summary.add(HumanActivity { start, end, kind }).unwrap();
        }
        let days = summary.finish();
        assert_eq!(days.len(), 1);
        assert_eq!(days[0].date, "1970-01-01");
        assert_eq!(days[0].day_length_ms, 86_400_000);
        assert_eq!(days[0].kinds[&HumanKind::Entertainment], 8000);
        assert_eq!(days[0].kinds[&HumanKind::NonEntertainment], 7000);
        assert_eq!(days[0].kinds[&HumanKind::Unknown], 0);
        assert_eq!(days[0].total_ms, 8000);
        assert!(days[0].observed.contains(&HumanKind::Unknown));
    }
}
