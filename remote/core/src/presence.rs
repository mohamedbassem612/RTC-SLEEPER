//! What the phone shows about the laptop. Pure functions so they are tested on any host.
use serde_json::{json, Value};
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
/// Who caused a sleep, in words. `by` is the actor stored with the schedule. The actor is
/// declared by the caller (the GUI and phone say "user", agents are expected to say "agent"),
/// so this is what the caller claimed, not something the laptop can prove.
pub fn who(by: &Value) -> String {
    let kind = by["kind"].as_str().unwrap_or("");
    let name = by["name"].as_str().unwrap_or("").trim();
    let model = by["model"].as_str().unwrap_or("").trim();
    match kind {
        "user" => {
            if let Some(device) = name.strip_prefix("Android:") {
                format!("the user, from the phone ({})", device.trim())
            } else if by["source"] == "gui" {
                "the user, from the laptop app".into()
            } else {
                "the user, from the command line".into()
            }
        }
        "agent" => {
            let parts: Vec<&str> = [name, model].into_iter().filter(|p| !p.is_empty()).collect();
            if parts.is_empty() {
                "an agent".into()
            } else {
                format!("an agent ({})", parts.join(", "))
            }
        }
        _ => "an unknown source".into(),
    }
}
/// The laptop speaking while it sleeps (or just before). First person, plain wording.
pub fn sleep_live(rec: &Value) -> String {
    let wake = rec["expected_wake_text"].as_str();
    if rec["source"] == "system" {
        match wake {
            Some(w) => format!("A user or an agent put me to sleep from the system, outside RTC Sleeper. A wake alarm is set for {w}."),
            None => "A user or an agent put me to sleep from the system, outside RTC Sleeper. I do not know when I will wake up.".into(),
        }
    } else {
        let when = wake.map_or("at an unknown time".to_string(), |w| format!("at {w}"));
        format!("I am sleeping now and will wake {when}. I was put to sleep by {}.", who(&rec["by"]))
    }
}
/// One line per fact for the history list in Settings: (when it slept, how it ended, who).
pub fn sleep_entry(rec: &Value) -> (String, String, String) {
    let slept = format!("Slept: {}", rec["at_text"].as_str().unwrap_or("unknown time"));
    let woke = match (rec["woke_text"].as_str(), rec["wake"].as_str()) {
        (Some(t), Some("alarm")) => format!("Woke: {t} (wake alarm)"),
        (Some(t), Some("failed")) => format!("Did not sleep (suspend failed or was cancelled), noticed {t}"),
        (Some(t), _) => format!("Woke: {t} (woken manually or by another event)"),
        _ => match rec["expected_wake_text"].as_str() {
            Some(w) => format!("Still asleep or not yet confirmed. Expected wake: {w}"),
            None => "Still asleep or not yet confirmed. Wake time unknown".into(),
        },
    };
    let by = if rec["source"] == "system" {
        "By: a user or an agent from the system, outside RTC Sleeper".to_string()
    } else {
        format!("By: {}", who(&rec["by"]))
    };
    (slept, woke, by)
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
        // Prefer the sleep the laptop itself recorded and left open; otherwise describe the
        // scheduled sleep that has just started.
        let open = state["history"]
            .as_array()
            .and_then(|h| h.last())
            .filter(|r| r["woke_at"].is_null() && r["source"] == "scheduler");
        let rec = open.cloned().unwrap_or_else(|| {
            json!({"source":"scheduler","by":state["actor"],
                   "expected_wake":state["next_wake"],"expected_wake_text":state["next_wake_text"]})
        });
        return match state["next_wake"].as_u64() {
            Some(w) if now < w => (
                "Laptop sleeping".into(),
                format!("{} That is in {}.", sleep_live(&rec), dur(w - now)),
            ),
            Some(w) => (
                "Laptop not reachable".into(),
                format!("It should have woken {} ago but is not answering. It may be off or without internet.", dur(now - w)),
            ),
            None => ("Laptop sleeping".into(), sleep_live(&rec)),
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
        assert!(d.contains("That is in 7 h 46 min"), "{d}");
        // Silent past the wake time: it should be awake by now.
        let (h, d) = presence(&s, false, true, 31000);
        assert_eq!(h, "Laptop not reachable");
        assert!(d.contains("should have woken"), "{d}");
        // An idle laptop (no schedule) that goes silent is never claimed to be sleeping.
        let idle = json!({"status":"idle","next_sleep":null,"next_wake":null});
        assert_eq!(presence(&idle, false, true, 99999).0, "Laptop not reachable");
    }
    #[test]
    fn who_names_user_agent_and_phone() {
        assert_eq!(who(&json!({"kind":"user","name":"User","source":"gui"})), "the user, from the laptop app");
        assert_eq!(who(&json!({"kind":"user","name":"User","source":"cli"})), "the user, from the command line");
        assert_eq!(who(&json!({"kind":"user","name":"Android: Pixel 8","source":"cli"})), "the user, from the phone (Pixel 8)");
        assert_eq!(who(&json!({"kind":"agent","name":"Claude","model":"opus"})), "an agent (Claude, opus)");
        assert_eq!(who(&json!({"kind":"agent","name":"","model":""})), "an agent");
        assert_eq!(who(&json!(null)), "an unknown source");
    }
    #[test]
    fn live_messages() {
        let by_user = json!({"source":"scheduler","by":{"kind":"user","name":"User","source":"gui"},
                             "expected_wake_text":"Sun 22 Nov, 7:00 AM"});
        assert_eq!(
            sleep_live(&by_user),
            "I am sleeping now and will wake at Sun 22 Nov, 7:00 AM. I was put to sleep by the user, from the laptop app."
        );
        let sys = json!({"source":"system","by":null,"expected_wake_text":null});
        assert_eq!(
            sleep_live(&sys),
            "A user or an agent put me to sleep from the system, outside RTC Sleeper. I do not know when I will wake up."
        );
        let sys_alarm = json!({"source":"system","expected_wake_text":"7:00 AM"});
        assert!(sleep_live(&sys_alarm).contains("A wake alarm is set for 7:00 AM."));
    }
    #[test]
    fn history_entries() {
        let rec = json!({"at_text":"Sat 21 Nov, 11:00 PM","source":"scheduler",
            "by":{"kind":"agent","name":"Bot","model":""},"woke_text":"Sun 22 Nov, 7:00 AM","wake":"alarm"});
        let (a, b, c) = sleep_entry(&rec);
        assert_eq!(a, "Slept: Sat 21 Nov, 11:00 PM");
        assert_eq!(b, "Woke: Sun 22 Nov, 7:00 AM (wake alarm)");
        assert_eq!(c, "By: an agent (Bot)");
        let open = json!({"at_text":"x","source":"system","woke_text":null,"wake":null,"expected_wake_text":null});
        let (_, b, c) = sleep_entry(&open);
        assert!(b.contains("Wake time unknown"));
        assert!(c.contains("outside RTC Sleeper"));
    }
    #[test]
    fn sleeping_message_names_who_and_when() {
        let s = json!({"status":"scheduled","next_sleep":1000,"next_wake":30000,
            "next_wake_text":"Mon 23 Nov, 7:00 AM","actor":{"kind":"user","name":"Android: Pixel","source":"cli"}});
        let (h, d) = presence(&s, false, true, 2000);
        assert_eq!(h, "Laptop sleeping");
        assert!(d.contains("will wake at Mon 23 Nov, 7:00 AM"), "{d}");
        assert!(d.contains("the user, from the phone (Pixel)"), "{d}");
    }
}
