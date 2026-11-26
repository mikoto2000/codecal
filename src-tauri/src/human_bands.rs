//! Bounded five-minute summaries; no observation identities or raw contents.
use crate::rei_human::{HumanActivity, HumanError, HumanKind};
use chrono::{DateTime, Duration, TimeZone};
use serde::Serialize;
use std::collections::BTreeMap;
const KINDS: [HumanKind; 3] = [
    HumanKind::Entertainment,
    HumanKind::NonEntertainment,
    HumanKind::Unknown,
];
#[derive(Debug, Serialize)]
pub struct HumanTimeBin {
    pub date: String,
    pub start: i64,
    pub end: i64,
    pub day_start: i64,
    pub day_end: i64,
    pub total_ms: i64,
    /// Union of non-entertainment and unknown, independent of raw-kind totals.
    pub other_ms: i64,
    pub kinds: BTreeMap<HumanKind, i64>,
    pub points: BTreeMap<HumanKind, bool>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn other_group_unions_non_entertainment_with_unknown() {
        let mut a = Accumulator::new(0, 300000, "UTC").unwrap();
        for (start, end, kind) in [
            (0, 120000, HumanKind::NonEntertainment),
            (60000, 180000, HumanKind::Unknown),
        ] {
            a.add(HumanActivity { start, end, kind }).unwrap();
        }
        let bins = a.finish();
        assert_eq!(bins[0].other_ms, 180000);
        assert_eq!(bins[0].kinds[&HumanKind::NonEntertainment], 120000);
        assert_eq!(bins[0].kinds[&HumanKind::Unknown], 120000);
    }
    #[test]
    fn dense_overlaps_gaps_points_and_request_clipping() {
        let mut a = Accumulator::new(1000, 600000, "UTC").unwrap();
        for _ in 0..100_000 {
            a.add(HumanActivity {
                start: 0,
                end: 180000,
                kind: HumanKind::Entertainment,
            })
            .unwrap();
        }
        a.add(HumanActivity {
            start: 60000,
            end: 240000,
            kind: HumanKind::NonEntertainment,
        })
        .unwrap();
        a.add(HumanActivity {
            start: 300000,
            end: 300000,
            kind: HumanKind::Unknown,
        })
        .unwrap();
        a.add(HumanActivity {
            start: 600000,
            end: 600000,
            kind: HumanKind::Unknown,
        })
        .unwrap();
        let bins = a.finish();
        assert_eq!(bins.len(), 2);
        assert_eq!(bins[0].kinds[&HumanKind::Entertainment], 179000);
        assert_eq!(bins[0].kinds[&HumanKind::NonEntertainment], 180000);
        assert_eq!(bins[0].total_ms, 239000);
        assert_eq!(bins[1].total_ms, 0);
        assert!(bins[1].points[&HumanKind::Unknown]);
    }
    #[test]
    fn actual_dst_day_lengths_and_bin_boundaries() {
        for (from, to, count) in [
            (1772946000000, 1773028800000, 276),
            (1793505600000, 1793595600000, 300),
        ] {
            let mut a = Accumulator::new(from, to, "America/New_York").unwrap();
            a.add(HumanActivity {
                start: from - 1000,
                end: to + 1000,
                kind: HumanKind::Unknown,
            })
            .unwrap();
            let bins = a.finish();
            assert_eq!(bins.len(), count);
            assert_eq!(bins.iter().map(|b| b.total_ms).sum::<i64>(), to - from);
            assert_eq!(bins[0].start, from);
            assert_eq!(bins.last().unwrap().end, to);
        }
    }
    #[test]
    fn empty_and_invalid_requests() {
        assert!(Accumulator::new(0, 86400000, "UTC")
            .unwrap()
            .finish()
            .is_empty());
        assert!(Accumulator::new(0, 0, "UTC").is_err());
        assert!(Accumulator::new(0, 86400000, "invalid").is_err());
        assert!(Accumulator::new(0, 63 * 86400000, "UTC").is_err());
    }
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
struct Slot {
    bin: HumanTimeBin,
    unions: [Union; 5],
    observed: bool,
}
pub(crate) struct Accumulator {
    slots: Vec<Slot>,
    from: i64,
    to: i64,
}
impl Accumulator {
    pub(crate) fn new(from: i64, to: i64, timezone: &str) -> Result<Self, HumanError> {
        crate::rei_human::validate_period(from, to)?;
        let tz: chrono_tz::Tz = timezone
            .parse()
            .map_err(|_| HumanError("invalid_timezone"))?;
        let mut slots = vec![];
        let mut cursor = from;
        while cursor < to {
            let local = DateTime::from_timestamp_millis(cursor)
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
            let day_end = crate::activity::next_day_boundary(&local)
                .map_err(|_| HumanError("invalid_period"))?;
            let mut start = day_start + (cursor - day_start) / 300000 * 300000;
            while start < day_end.min(to) {
                let end = (start + 300000).min(day_end);
                slots.push(Slot {
                    bin: HumanTimeBin {
                        date: local.format("%Y-%m-%d").to_string(),
                        start,
                        end,
                        day_start,
                        day_end,
                        total_ms: 0,
                        other_ms: 0,
                        kinds: KINDS.into_iter().map(|k| (k, 0)).collect(),
                        points: KINDS.into_iter().map(|k| (k, false)).collect(),
                    },
                    unions: std::array::from_fn(|_| Union::default()),
                    observed: false,
                });
                start = end;
            }
            cursor = day_end;
        }
        Ok(Self { slots, from, to })
    }
    // Cache streams records in nondecreasing start order; unions need no retained records.
    pub(crate) fn add(&mut self, mut record: HumanActivity) -> Result<(), HumanError> {
        let point = record.start == record.end;
        if if point {
            record.start < self.from || record.start >= self.to
        } else {
            record.end <= self.from || record.start >= self.to
        } {
            return Ok(());
        }
        record.start = record.start.max(self.from);
        record.end = record.end.min(self.to);
        let first = self.slots.partition_point(|s| s.bin.end <= record.start);
        for slot in &mut self.slots[first..] {
            if if point {
                slot.bin.start > record.start
            } else {
                slot.bin.start >= record.end
            } {
                break;
            }
            slot.observed = true;
            let i = KINDS
                .iter()
                .position(|k| *k == record.kind)
                .expect("normalized kind");
            if point {
                slot.bin.points.insert(record.kind, true);
            } else {
                let start = record.start.max(slot.bin.start);
                let end = record.end.min(slot.bin.end);
                slot.unions[i].add(start, end);
                slot.unions[3].add(start, end);
                if record.kind != HumanKind::Entertainment {
                    slot.unions[4].add(start, end);
                }
            }
        }
        Ok(())
    }
    pub(crate) fn finish(self) -> Vec<HumanTimeBin> {
        self.slots
            .into_iter()
            .filter(|s| s.observed)
            .map(|mut s| {
                for (i, k) in KINDS.into_iter().enumerate() {
                    s.bin.kinds.insert(k, s.unions[i].total);
                }
                s.bin.total_ms = s.unions[3].total;
                s.bin.other_ms = s.unions[4].total;
                s.bin
            })
            .collect()
    }
}
