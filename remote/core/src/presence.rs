//! What the phone shows about the laptop. Pure functions so they are tested on any host.
use serde_json::Value;
pub fn dur(secs: u64) -> String {
    match secs {
        0..=59 => format!("{secs} s"),
        60..=3599 => format!("{} min", secs / 60),
        _ => format!("{} h {} min", secs / 3600, secs / 60 % 60),
    }
}
/// Plain-language name for the scheduler status instead of the raw engine word.
pub fn schedule_label(status: &str) -> &str {
    match status {
        "idle" => "No schedule set",
        "scheduled" => "Sleep scheduled",
        "sleeping" => "Going to sleep now",
        "paused" => "Schedule paused",
        "conflict" => "Another wake alarm is set on the laptop",
        "error" => "Scheduler error",
        other => other,
    }
}
/// What the phone can honestly say about the laptop. While the laptop sleeps it cannot answer,
/// so "sleeping" is inferred from the last confirmed schedule: the sleep time has passed and
/// the connection stopped.
pub fn presence(state: &Value, online: bool, heard: bool, now: u64) -> (String, String) {
    if !heard {
        return ("Waiting for laptop".into(), "Laptop status has not been received yet".into());
    }
    let status = state["status"].as_str().unwrap_or("");
    if online {
        return if status == "sleeping" {
            ("Laptop going to sleep".into(), String::new())
        } else {
            ("Laptop awake".into(), String::new())
        };
    }
    let slept = matches!(status, "scheduled" | "sleeping")
        && state["next_sleep"].as_u64().is_some_and(|n| now + 5 >= n);
    if slept {
        return match state["next_wake"].as_u64() {
            Some(w) if now < w => (
                "Laptop sleeping".into(),
                format!("It stopped answering at the scheduled sleep time. Expected to wake in {}.", dur(w - now)),
            ),
            Some(w) => (
                "Laptop not reachable".into(),
                format!("It should have woken {} ago but is not answering. It may be off or without internet.", dur(now - w)),
            ),
            None => ("Laptop sleeping".into(), "It stopped answering at the scheduled sleep time.".into()),
        };
    }
    (
        "Laptop not reachable".into(),
        "Connection lost. It may be asleep, off or without internet. Showing the last confirmed status.".into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn durations() {
        assert_eq!(dur(5), "5 s");
        assert_eq!(dur(125), "2 min");
        assert_eq!(dur(3 * 3600 + 7 * 60), "3 h 7 min");
    }
    #[test]
    fn friendly_labels() {
        assert_eq!(schedule_label("idle"), "No schedule set");
        assert_eq!(schedule_label("scheduled"), "Sleep scheduled");
        assert_eq!(schedule_label("whatever"), "whatever");
    }
    #[test]
    fn presence_states() {
        let s = json!({"status":"scheduled","next_sleep":1000,"next_wake":30000});
        // Never heard from the laptop.
        assert_eq!(presence(&json!({}), false, false, 0).0, "Waiting for laptop");
        // Answering: awake.
        assert_eq!(presence(&s, true, true, 500).0, "Laptop awake");
        // Silent before the sleep time: just unreachable, not "sleeping".
        assert_eq!(presence(&s, false, true, 500).0, "Laptop not reachable");
        // Silent after the sleep time and before the wake time: sleeping.
        let (h, d) = presence(&s, false, true, 2000);
        assert_eq!(h, "Laptop sleeping");
        assert!(d.contains("Expected to wake in 7 h 46 min"), "{d}");
        // Silent past the wake time: it should be awake by now.
        let (h, d) = presence(&s, false, true, 31000);
        assert_eq!(h, "Laptop not reachable");
        assert!(d.contains("should have woken"), "{d}");
        // An idle laptop (no schedule) that goes silent is never claimed to be sleeping.
        let idle = json!({"status":"idle","next_sleep":null,"next_wake":null});
        assert_eq!(presence(&idle, false, true, 99999).0, "Laptop not reachable");
    }
}
