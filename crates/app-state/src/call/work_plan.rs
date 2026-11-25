use chatsvc::{Calendar, WorkPlanDay, WorkPlanLocation};
use chrono::{Datelike, Duration, NaiveDate, NaiveTime, Weekday};
use gpui_kit::Context;

use crate::app_state::AppState;
use crate::notice::short_error;

/// The demo plan: home on Monday and Friday, office in between, nothing at the weekend.
pub fn demo_work_plan(dates: &[NaiveDate]) -> Vec<WorkPlanDay> {
    let time = |hour, minute| NaiveTime::from_hms_opt(hour, minute, 0).unwrap_or_default();
    dates
        .iter()
        .filter_map(|&date| {
            let (location, start, end) = match date.weekday() {
                Weekday::Mon => (WorkPlanLocation::Remote, time(8, 0), time(17, 0)),
                Weekday::Fri => (WorkPlanLocation::Remote, time(8, 0), time(14, 0)),
                Weekday::Sat | Weekday::Sun => return None,
                _ => (WorkPlanLocation::Office, time(7, 0), time(16, 30)),
            };
            Some(WorkPlanDay {
                id: format!("demo-{date}"),
                date,
                location,
                start,
                end,
                occurrence: serde_json::Value::Null,
            })
        })
        .collect()
}

impl AppState {
    pub(crate) fn load_work_plan(&mut self, cx: &mut Context<Self>) {
        let dates = self.calendar_dates();
        let (Some(&from), Some(&last)) = (dates.first(), dates.last()) else {
            return;
        };
        if self.mode.demo {
            self.calendar_week.work_plan = demo_work_plan(&dates);
            return;
        }
        let Some(engine) = self.engine.clone() else {
            return;
        };
        let to = last + Duration::days(1);
        let receiver = crate::runtime::spawn(async move {
            Calendar::new(engine.remote().session())
                .work_plan(from, to)
                .await
        });
        cx.spawn(async move |this, cx| {
            // A plan that does not load leaves the day headers without a location, nothing more.
            if let Ok(Ok(plan)) = receiver.await {
                this.update(cx, |state, cx| {
                    if state.calendar_dates().first() == Some(&from) {
                        state.calendar_week.work_plan = plan;
                        cx.notify();
                    }
                })
                .ok();
            }
        })
        .detach();
    }

    pub fn set_work_plan_location(
        &mut self,
        date: NaiveDate,
        location: WorkPlanLocation,
        cx: &mut Context<Self>,
    ) {
        let Some(day) = self
            .calendar_week
            .work_plan
            .iter_mut()
            .find(|day| day.date == date)
        else {
            return;
        };
        if day.location == location {
            return;
        }
        let previous = day.clone();
        day.location = location;
        cx.notify();
        if self.mode.demo {
            return;
        }
        let Some(engine) = self.engine.clone() else {
            return;
        };
        let receiver = crate::runtime::spawn(async move {
            Calendar::new(engine.remote().session())
                .set_work_plan_location(&previous, location)
                .await
        });
        cx.spawn(async move |this, cx| {
            let failure = match receiver.await {
                Ok(Ok(())) => None,
                Ok(Err(error)) => Some(short_error(&error)),
                Err(error) => Some(short_error(&error)),
            };
            this.update(cx, |state, cx| {
                if let Some(reason) = failure {
                    state.raise_notice(
                        format!(
                            "{}: {reason}",
                            app_base::ui_locale::tr("calendar.work_location.not_saved")
                        ),
                        None,
                        cx,
                    );
                }
                state.load_work_plan(cx);
            })
            .ok();
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use chatsvc::WorkPlanLocation;
    use chrono::NaiveDate;

    use super::demo_work_plan;

    #[test]
    fn the_demo_plan_is_home_monday_and_friday_and_free_at_the_weekend() {
        let monday = NaiveDate::from_ymd_opt(2026, 11, 23).unwrap();
        let week: Vec<NaiveDate> = monday.iter_days().take(7).collect();
        let plan = demo_work_plan(&week);
        let locations: Vec<WorkPlanLocation> = plan.iter().map(|day| day.location).collect();
        assert_eq!(
            locations,
            [
                WorkPlanLocation::Remote,
                WorkPlanLocation::Office,
                WorkPlanLocation::Office,
                WorkPlanLocation::Office,
                WorkPlanLocation::Remote
            ]
        );
    }
}
