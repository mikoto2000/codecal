use chrono::{DateTime, Duration, TimeZone, Utc};
use chrono_tz::Tz;
use serde::Serialize;
use std::collections::BTreeMap;
pub struct Interval {
    pub project_id: String,
    pub start: i64,
    pub end: Option<i64>,
}
#[derive(Serialize)]
pub struct Day {
    pub date: String,
    pub elapsed_ms: i64,
    pub total_ms: i64,
    pub projects: BTreeMap<String, i64>,
}
pub fn union_ms(ranges: &mut [(i64, i64)]) -> i64 {
    ranges.sort_unstable();
    let mut total = 0;
    let mut current: Option<(i64, i64)> = None;
    for &(s, e) in ranges.iter().filter(|(s, e)| e > s) {
        match current {
            Some((a, b)) if s <= b => current = Some((a, b.max(e))),
            Some((a, b)) => {
                total += b - a;
                current = Some((s, e));
            }
            None => current = Some((s, e)),
        }
    }
    total + current.map_or(0, |(s, e)| e - s)
}
pub fn aggregate(intervals: &[Interval], timezone: &str) -> Result<Vec<Day>, String> {
    let tz: Tz = timezone.parse().map_err(|_| "invalid timezone")?;
    let mut days: BTreeMap<String, (Day, Vec<(i64, i64)>)> = BTreeMap::new();
    for interval in intervals {
        let Some(end) = interval.end else { continue };
        let mut start = interval.start;
        while start < end {
            let local = DateTime::<Utc>::from_timestamp_millis(start)
                .ok_or("invalid timestamp")?
                .with_timezone(&tz);
            let next_date = local.date_naive().succ_opt().ok_or("date overflow")?;
            let midnight = next_date.and_hms_opt(0, 0, 0).ok_or("invalid midnight")?;
            // Some zones skip midnight; find the first valid instant of the following date.
            let boundary = (0..=180)
                .find_map(|m| {
                    tz.from_local_datetime(&(midnight + Duration::minutes(m)))
                        .earliest()
                })
                .ok_or("unresolvable date boundary")?
                .timestamp_millis();
            let stop = end.min(boundary);
            if stop <= start {
                return Err("invalid date boundary".into());
            }
            let date = local.format("%Y-%m-%d").to_string();
            let (day, ranges) = days.entry(date.clone()).or_insert_with(|| {
                (
                    Day {
                        date,
                        elapsed_ms: 0,
                        total_ms: 0,
                        projects: BTreeMap::new(),
                    },
                    vec![],
                )
            });
            day.total_ms += stop - start;
            *day.projects.entry(interval.project_id.clone()).or_default() += stop - start;
            ranges.push((start, stop));
            start = stop;
        }
    }
    Ok(days
        .into_values()
        .map(|(mut d, mut r)| {
            d.elapsed_ms = union_ms(&mut r);
            d
        })
        .collect())
}
