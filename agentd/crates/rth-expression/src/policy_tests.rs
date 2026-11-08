

use rth_self::{ClockTime, DailyCap, SelfSpec};

use super::*;

const OFF: i32 = -180;

const DAY0_MS: u64 = 1_792_972_800_000;
const JID: &str = "5500000000000@s.whatsapp.net";

fn t(hh: i64, mm: i64, off: i32) -> u64 {
    ((DAY0_MS / 60_000) as i64 + hh * 60 + mm - i64::from(off)) as u64 * 60_000
}

fn present() -> Presence {
    Presence { locked: false, display_asleep: false, idle_s: 0, output_busy: false, last_origin: "mac".into(), frontmost: true, notify_ok: true }
}

fn cand(kind: Kind) -> Candidate {
    Candidate { kind, subject_id: "s-1".into(), egress: Egress::None, held: false, reason_days: 0, data: vec![], memory_refs: vec![] }
}

fn grant(channel: Channel, recipient: Option<&str>) -> Grant {
    Grant { channel, recipient: recipient.map(String::from), copy_version: 1, granted_utc_ms: 1 }
}

struct Fx {
    now: u64,
    off: i32,
    prefs: ExpressionPrefs,
    presence: Option<Presence>,
    history: Vec<Attempt>,
    turn: TurnState,
    consents: Vec<Grant>,
    recipient: Option<String>,
    egress: bool,
}

impl Fx {
    fn new() -> Fx {
        Fx {
            now: t(12, 0, OFF),
            off: OFF,
            prefs: SelfSpec::release_default("t").expression,
            presence: Some(present()),
            history: vec![],
            turn: TurnState::default(),
            consents: vec![],
            recipient: None,
            egress: false,
        }
    }

    fn at(mut self, hh: i64, mm: i64) -> Fx {
        self.now = t(hh, mm, self.off);
        self
    }

    fn attempt(&mut self, kind: Kind, subject: &str, utc_ms: u64) {
        self.history.push(Attempt { kind, subject_id: subject.into(), utc_ms, local_day: local_day(utc_ms, self.off) });
    }

    fn v(&self, c: &Candidate) -> Verdict {
        let f = Facts {
            now_utc_ms: self.now,
            utc_offset_min: self.off,
            prefs: &self.prefs,
            presence: self.presence.as_ref(),
            history: &self.history,
            turn: self.turn,
            consents: &self.consents,
            omni_recipient: self.recipient.as_deref(),
            omni_egress_granted: self.egress,
        };
        decide(&f, c)
    }
}

const CHAT: Verdict = Verdict::Send(Destination::Chat);

fn hold(r: HoldReason) -> Verdict {
    Verdict::Hold(r)
}

fn drop_(r: DropReason) -> Verdict {
    Verdict::Drop(r)
}

#[test]
fn b3_18_no_presence_frame_yet_holds_every_kind() {
    let mut fx = Fx::new();
    fx.presence = None;
    for k in Kind::ALL {
        assert_eq!(fx.v(&cand(k)), hold(HoldReason::NoPresence), "{k:?}");
    }
}

#[test]
fn b3_19_20_quiet_hours_start_at_22_and_end_at_08_exclusive() {
    let c = cand(Kind::Curiosity);
    assert_eq!(Fx::new().at(21, 59).v(&c), CHAT);
    assert_eq!(Fx::new().at(22, 0).v(&c), hold(HoldReason::QuietHours));
    assert_eq!(Fx::new().at(0, 30).v(&c), hold(HoldReason::QuietHours), "the window wraps midnight");
    assert_eq!(Fx::new().at(7, 59).v(&c), hold(HoldReason::QuietHours));
    assert_eq!(Fx::new().at(8, 0).v(&c), CHAT);
}

#[test]
fn b3_21_a_window_that_does_not_wrap() {
    let mut fx = Fx::new();
    fx.prefs.quiet_hours.start = ClockTime::new("13:00").unwrap();
    fx.prefs.quiet_hours.end = ClockTime::new("14:00").unwrap();
    let c = cand(Kind::Curiosity);
    assert_eq!(fx.at(12, 59).v(&c), CHAT);
    let mut fx = Fx::new();
    fx.prefs.quiet_hours.start = ClockTime::new("13:00").unwrap();
    fx.prefs.quiet_hours.end = ClockTime::new("14:00").unwrap();
    assert_eq!(fx.at(13, 30).v(&c), hold(HoldReason::QuietHours));
    let mut fx = Fx::new();
    fx.prefs.quiet_hours.start = ClockTime::new("13:00").unwrap();
    fx.prefs.quiet_hours.end = ClockTime::new("14:00").unwrap();
    assert_eq!(fx.at(14, 0).v(&c), CHAT);
    assert_eq!(Fx::new().at(23, 0).v(&c), hold(HoldReason::QuietHours), "the default still wraps");
}

#[test]
fn b3_22_the_offset_decides_the_local_time_so_dst_is_just_another_offset() {
    let instant = t(21, 30, -180);
    assert_eq!(instant, t(22, 30, -120), "one UTC instant");
    let c = cand(Kind::Curiosity);
    let mut fx = Fx::new();
    fx.now = instant;
    assert_eq!(fx.v(&c), CHAT, "21:30 at −180");
    fx.off = -120;
    assert_eq!(fx.v(&c), hold(HoldReason::QuietHours), "22:30 at −120");
}

#[test]
fn b3_23_the_local_day_starts_at_04_00() {
    assert_eq!(t(4, 0, -180) % (24 * 3_600_000), 7 * 3_600_000, "04:00 at −180 is 07:00 UTC");
    assert_eq!(local_day(t(3, 59, OFF), OFF) + 1, local_day(t(4, 0, OFF), OFF), "03:59 belongs to the day before");
    assert_eq!(local_day(t(4, 0, OFF), OFF), local_day(t(23, 59, OFF), OFF));
    assert_eq!(local_day(t(27, 59, OFF), OFF), local_day(t(4, 0, OFF), OFF), "03:59 the next morning is still today");
}

#[test]
fn b3_24_the_fourth_attempt_of_a_day_drops_and_the_next_day_sends() {
    let mut fx = Fx::new();
    for (i, h) in [9, 10, 11].into_iter().enumerate() {
        fx.attempt(Kind::Curiosity, &format!("c-{i}"), t(h, 0, OFF));
    }
    let c = Candidate { subject_id: "c-new".into(), ..cand(Kind::Curiosity) };
    assert_eq!(fx.v(&c), drop_(DropReason::Cap));
    fx.now = t(27, 59, OFF);
    assert_eq!(fx.v(&c), drop_(DropReason::Cap), "03:59 next morning is the same day");
    fx.now = t(28, 0, OFF);
    assert_eq!(fx.v(&c), hold(HoldReason::QuietHours), "a new day: the cap no longer drops it");
    fx.now = t(32, 0, OFF);
    assert_eq!(fx.v(&c), CHAT);
}

#[test]
fn b3_25_held_messages_count_toward_the_cap_and_surfacing_one_does_not_count_again() {
    let mut fx = Fx::new();
    fx.attempt(Kind::Curiosity, "c-0", t(9, 0, OFF));
    fx.attempt(Kind::OpenThreads, "h-1", t(10, 0, OFF));
    fx.attempt(Kind::NameWish, "p-1", t(11, 0, OFF));
    assert_eq!(fx.v(&Candidate { subject_id: "c-new".into(), ..cand(Kind::Curiosity) }), drop_(DropReason::Cap));
    let surfacing = Candidate { subject_id: "p-1".into(), held: true, ..cand(Kind::NameWish) };
    assert_eq!(fx.v(&surfacing), CHAT, "neither the cap nor its own offer drops it");
    let fresh = Candidate { held: false, ..surfacing };
    assert_eq!(fx.v(&fresh), drop_(DropReason::Duplicate));
}

#[test]
fn b3_26_a_cap_of_zero_drops_everything() {
    let mut fx = Fx::new();
    fx.prefs.daily_cap = DailyCap::new(0).unwrap();
    for k in Kind::ALL {
        assert_eq!(fx.v(&cand(k)), drop_(DropReason::Cap), "{k:?}");
    }
}

#[test]
fn b3_27_each_kind_can_be_turned_off_alone() {
    for off in Kind::ALL {
        let mut fx = Fx::new();
        match off {
            Kind::NameWish => fx.prefs.kinds.name_wish = false,
            Kind::Overnight => fx.prefs.kinds.overnight = false,
            Kind::Curiosity => fx.prefs.kinds.curiosity = false,
            Kind::OpenThreads => fx.prefs.kinds.open_threads = false,
        }
        for k in Kind::ALL {
            assert_eq!(fx.v(&cand(k)), if k == off { drop_(DropReason::KindOff) } else { CHAT }, "{off:?} off, {k:?}");
        }
    }
}

#[test]
fn b3_28_each_away_cause_holds_without_omni() {
    let causes: [fn(&mut Presence); 4] = [|p| p.locked = true, |p| p.display_asleep = true, |p| p.idle_s = 301, |p| p.last_origin = "omni".into()];
    for (i, cause) in causes.iter().enumerate() {
        let mut fx = Fx::new();
        cause(fx.presence.as_mut().unwrap());
        assert_eq!(fx.v(&cand(Kind::Curiosity)), hold(HoldReason::Away), "cause {i}");
    }
    let mut fx = Fx::new();
    fx.presence.as_mut().unwrap().idle_s = 300;
    assert_eq!(fx.v(&cand(Kind::Curiosity)), CHAT, "300 s idle is still present");
}

#[test]
fn b3_29_the_destination_follows_focus_and_the_notification_consent() {
    let c = cand(Kind::Curiosity);
    let notifying = || {
        let mut fx = Fx::new();
        fx.prefs.destinations.notification = true;
        fx.consents = vec![grant(Channel::Notification, None)];
        fx.presence.as_mut().unwrap().frontmost = false;
        fx
    };
    let mut fx = notifying();
    fx.presence.as_mut().unwrap().frontmost = true;
    assert_eq!(fx.v(&c), CHAT, "present and frontmost");
    assert_eq!(notifying().v(&c), Verdict::Send(Destination::Notification), "not frontmost, consented, notify_ok");
    let mut fx = notifying();
    fx.presence.as_mut().unwrap().notify_ok = false;
    assert_eq!(fx.v(&c), CHAT, "macOS hasn't allowed notifications");
    let mut fx = notifying();
    fx.prefs.destinations.notification = false;
    assert_eq!(fx.v(&c), CHAT, "the notification destination is off");
    let mut fx = notifying();
    fx.consents.clear();
    assert_eq!(fx.v(&c), CHAT, "a revoke wins over a true left in the spec");
}

#[test]
fn b3_30_chat_off_and_no_notification_holds() {
    let mut fx = Fx::new();
    fx.prefs.destinations.chat = false;
    fx.presence.as_mut().unwrap().frontmost = false;
    assert_eq!(fx.v(&cand(Kind::Curiosity)), hold(HoldReason::NoDestination));
}

#[test]
fn b3_31_away_omni_needs_consent_the_recipient_and_for_memory_the_egress_grant() {
    let away = || {
        let mut fx = Fx::new();
        fx.presence.as_mut().unwrap().locked = true;
        fx.prefs.destinations.omni = true;
        fx.consents = vec![grant(Channel::Omni, Some(JID))];
        fx.recipient = Some(JID.into());
        fx
    };
    let plain = cand(Kind::Curiosity);
    let memory = Candidate { egress: Egress::Memory, ..cand(Kind::Curiosity) };
    assert_eq!(away().v(&plain), Verdict::Send(Destination::Omni));
    let mut fx = away();
    fx.recipient = None;
    assert_eq!(fx.v(&plain), hold(HoldReason::NoRecipient), "no configured number: the channel is off");
    let mut fx = away();
    fx.recipient = Some("5511111111111@s.whatsapp.net".into());
    assert_eq!(fx.v(&plain), hold(HoldReason::RecipientMismatch));
    assert_eq!(away().v(&memory), hold(HoldReason::NoEgress));
    let mut fx = away();
    fx.egress = true;
    assert_eq!(fx.v(&memory), Verdict::Send(Destination::Omni));
    let mut fx = away();
    fx.consents.clear();
    assert_eq!(fx.v(&plain), hold(HoldReason::Away), "no consent");
    let mut fx = away();
    fx.prefs.destinations.omni = false;
    assert_eq!(fx.v(&plain), hold(HoldReason::Away), "omni off in the spec");
}

#[test]
fn b3_32_never_while_a_turn_streams() {
    let mut fx = Fx::new();
    fx.turn.streaming = true;
    assert_eq!(fx.v(&cand(Kind::Curiosity)), hold(HoldReason::TurnRunning));
}

#[test]
fn b3_33_the_quiet_gap_after_the_persons_last_message() {
    let mut fx = Fx::new();
    fx.turn.last_person_utc_ms = Some(fx.now - QUIET_GAP_MS + 1_000);
    assert_eq!(fx.v(&cand(Kind::Curiosity)), hold(HoldReason::QuietGap), "9 min 59 s");
    fx.turn.last_person_utc_ms = Some(fx.now - QUIET_GAP_MS);
    assert_eq!(fx.v(&cand(Kind::Curiosity)), CHAT, "10 min");
}

#[test]
fn b3_34_a_subject_is_offered_once_per_window() {
    const D: u64 = 24 * 3_600_000;
    let cases = [(Kind::OpenThreads, 3 * D), (Kind::Curiosity, 7 * D)];
    for (kind, window) in cases {
        let mut fx = Fx::new();
        fx.attempt(kind, "s-1", fx.now - window + 60_000);
        assert_eq!(fx.v(&cand(kind)), drop_(DropReason::Duplicate), "{kind:?} inside its window");
        assert_eq!(fx.v(&Candidate { subject_id: "s-2".into(), ..cand(kind) }), CHAT, "another subject");
        let mut fx = Fx::new();
        fx.attempt(kind, "s-1", fx.now - window);
        assert_eq!(fx.v(&cand(kind)), CHAT, "{kind:?} past its window");
    }
    for kind in [Kind::NameWish, Kind::Overnight] {
        let mut fx = Fx::new();
        fx.attempt(kind, "s-1", fx.now - 365 * D);
        assert_eq!(fx.v(&cand(kind)), drop_(DropReason::Duplicate), "{kind:?} is offered once");
    }
    let mut fx = Fx::new();
    fx.attempt(Kind::Curiosity, "s-1", fx.now - 60_000);
    assert_eq!(fx.v(&cand(Kind::OpenThreads)), CHAT, "the key is per kind");
}

#[test]
fn b3_37_the_send_time_check_sees_presence_flip() {
    let mut fx = Fx::new();
    let c = cand(Kind::OpenThreads);
    assert_eq!(fx.v(&c), CHAT, "at the tick");
    fx.presence.as_mut().unwrap().locked = true;
    assert_eq!(fx.v(&c), hold(HoldReason::Away), "after the draft, locked");
}

#[test]
fn b3_the_first_rule_that_applies_wins_in_one_fixed_order() {

    let mut fx = Fx::new().at(23, 0);
    fx.prefs.kinds.curiosity = false;
    fx.presence = None;
    for (i, h) in [9, 10, 11].into_iter().enumerate() {
        fx.attempt(Kind::OpenThreads, &format!("h-{i}"), t(h, 0, OFF));
    }
    fx.attempt(Kind::Curiosity, "s-1", fx.now - 60_000);
    fx.turn = TurnState { streaming: true, last_person_utc_ms: Some(fx.now - 1_000) };
    let c = cand(Kind::Curiosity);
    assert_eq!(fx.v(&c), drop_(DropReason::KindOff));
    fx.prefs.kinds.curiosity = true;
    assert_eq!(fx.v(&c), hold(HoldReason::NoPresence));
    let mut away = present();
    away.locked = true;
    fx.presence = Some(away);
    assert_eq!(fx.v(&c), drop_(DropReason::Duplicate));
    fx.history.retain(|a| a.kind != Kind::Curiosity);
    assert_eq!(fx.v(&c), drop_(DropReason::Cap));
    fx.history.clear();
    assert_eq!(fx.v(&c), hold(HoldReason::TurnRunning));
    fx.turn.streaming = false;
    assert_eq!(fx.v(&c), hold(HoldReason::QuietGap));
    fx.turn.last_person_utc_ms = None;
    assert_eq!(fx.v(&c), hold(HoldReason::QuietHours));
    fx.now = t(12, 0, OFF);
    assert_eq!(fx.v(&c), hold(HoldReason::Away));
}

#[test]
fn b3_kind_names_are_the_specs_keys_and_b1s_reach_kind() {
    let prefs = SelfSpec::release_default("t").expression;
    let mut keys: Vec<String> = serde_json::to_value(prefs.kinds).unwrap().as_object().unwrap().keys().cloned().collect();
    keys.sort();
    let mut ours: Vec<String> = Kind::ALL.iter().map(|k| k.as_str().to_string()).collect();
    ours.sort();
    assert_eq!(keys, ours);
    for k in Kind::ALL {
        assert_eq!(serde_json::to_value(k).unwrap(), k.as_str());
    }
    assert_eq!(serde_json::to_value(Verdict::Hold(HoldReason::RecipientMismatch)).unwrap(), serde_json::json!({"hold": "recipient_mismatch"}));
}
