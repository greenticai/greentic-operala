pub mod schedule;

#[cfg(test)]
mod tests {
    use super::schedule::{TimeOfDay, TriggerSchedule};

    #[test]
    fn vendored_schedule_serde_matches_expected_shape() {
        let s = TriggerSchedule::Daily {
            at: TimeOfDay { hour: 6, minute: 0 },
        };
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["kind"], "daily");
        assert_eq!(v["at"]["hour"], 6);
        let back: TriggerSchedule = serde_json::from_value(v).unwrap();
        assert_eq!(back, s);
    }
}
