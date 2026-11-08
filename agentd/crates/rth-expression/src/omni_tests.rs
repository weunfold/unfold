

use rth_self::SelfSpec;
use rth_self::store::Lines;
use serde_json::json;

use super::*;
use crate::policy::{Candidate, Destination, Egress, Facts, HoldReason, Kind, Presence, TurnState, Verdict, decide};
use crate::reason::Reason;

const JID: &str = "5511999999999@s.whatsapp.net";
const OTHER: &str = "5511888888888@s.whatsapp.net";

const NOON: u64 = 1_792_972_800_000 + 12 * 60 * 60 * 1000;

fn away() -> Presence {
    Presence { locked: true, display_asleep: false, idle_s: 0, output_busy: false, last_origin: "mac".into(), frontmost: false, notify_ok: false }
}

fn cand(egress: Egress) -> Candidate {
    Candidate { kind: Kind::Curiosity, subject_id: "s-1".into(), egress, held: false, reason_days: 0, data: vec![], memory_refs: vec![] }
}

fn consents(lines: Vec<serde_json::Value>) -> Vec<Grant> {
    consent::fold(&Lines { records: lines, torn: false, corrupt: false })
}

fn omni_grant(recipient: &str) -> serde_json::Value {
    json!({"channel": "omni", "recipient": recipient, "copy_version": 1, "granted_utc_ms": 1, "revoked": false})
}

fn verdict(grants: &[Grant], configured: Option<&str>, omni_egress_granted: bool, c: &Candidate) -> Verdict {
    let mut prefs = SelfSpec::release_default("t").expression;
    prefs.destinations.omni = true;
    let p = away();
    let f = Facts { now_utc_ms: NOON, utc_offset_min: 0, prefs: &prefs, presence: Some(&p), history: &[], turn: TurnState::default(), consents: grants, omni_recipient: recipient(configured), omni_egress_granted };
    decide(&f, c)
}

fn msg() -> Message {
    Message::new("Lua".into(), "hi".into(), Reason::curiosity())
}

#[test]
fn b10_no_recipient_configured_keeps_omni_off_even_when_consented() {
    let grants = consents(vec![omni_grant(JID)]);
    for configured in [None, Some(""), Some("   ")] {
        assert_eq!(recipient(configured), None, "{configured:?}");
        let mut fake = FakeOmni { recipient: configured.map(String::from), grants: grants.clone(), sent: vec![] };
        assert!(!fake.available(), "{configured:?}");
        assert_eq!(verdict(&grants, configured, true, &cand(Egress::None)), Verdict::Hold(HoldReason::NoRecipient), "{configured:?}");
        assert_eq!(deliver(&mut fake, configured, &msg()), Outcome::Failed("no_recipient"));
        assert!(fake.sent.is_empty());
    }
    assert_eq!(recipient(Some(&format!(" {JID} "))), Some(JID), "trimmed");
}

#[test]
fn b10_a_recipient_other_than_the_consents_turns_omni_off() {
    let grants = consents(vec![omni_grant(JID)]);
    assert_eq!(verdict(&grants, Some(OTHER), true, &cand(Egress::None)), Verdict::Hold(HoldReason::RecipientMismatch));
    let mut fake = FakeOmni { recipient: Some(OTHER.into()), grants, sent: vec![] };
    assert!(fake.available() && !fake.consented(OTHER) && fake.consented(JID));
    assert_eq!(deliver(&mut fake, Some(OTHER), &msg()), Outcome::Failed("no_consent"));
    assert!(fake.sent.is_empty());
}

#[test]
fn b10_a_consent_revoked_after_save_or_of_another_copy_is_no_consent() {
    let revoked = consents(vec![omni_grant(JID), json!({"channel": "omni", "recipient": JID, "copy_version": 1, "granted_utc_ms": 2, "revoked": true})]);
    let copy2 = consents(vec![json!({"channel": "omni", "recipient": JID, "copy_version": 2, "granted_utc_ms": 1, "revoked": false})]);
    for (what, grants) in [("revoked", revoked), ("copy 2", copy2), ("never", vec![])] {
        assert!(grants.is_empty(), "{what}");
        assert_eq!(verdict(&grants, Some(JID), true, &cand(Egress::None)), Verdict::Hold(HoldReason::Away), "{what}");
        let mut fake = FakeOmni { recipient: Some(JID.into()), grants, sent: vec![] };
        assert_eq!(deliver(&mut fake, Some(JID), &msg()), Outcome::Failed("no_consent"), "{what}");
    }
}

#[test]
fn b10_memory_content_needs_the_egress_grant() {
    let grants = consents(vec![omni_grant(JID)]);
    assert_eq!(verdict(&grants, Some(JID), false, &cand(Egress::Memory)), Verdict::Hold(HoldReason::NoEgress));
    assert_eq!(verdict(&grants, Some(JID), true, &cand(Egress::Memory)), Verdict::Send(Destination::Omni));
}

#[test]
fn b10_away_with_a_consented_fake_omni_is_chosen_and_sends_once() {
    let grants = consents(vec![omni_grant(JID)]);
    assert_eq!(verdict(&grants, Some(JID), false, &cand(Egress::None)), Verdict::Send(Destination::Omni));
    let mut fake = FakeOmni { recipient: Some(JID.into()), grants, sent: vec![] };
    assert_eq!(deliver(&mut fake, Some(JID), &msg()), Outcome::Sent);
    assert_eq!(fake.sent, [msg()]);
}

#[test]
fn b10_deliver_rechecks_the_channel_and_a_consent_for_omni_itself() {
    let grants = consents(vec![omni_grant(JID)]);
    let mut down = FakeOmni { recipient: None, grants: grants.clone(), sent: vec![] };
    assert_eq!(deliver(&mut down, Some(JID), &msg()), Outcome::Failed("no_recipient"), "an unavailable channel sends nothing");
    let other = vec![Grant { channel: consent::Channel::Notification, recipient: Some(JID.into()), copy_version: 1, granted_utc_ms: 1 }];
    let mut fake = FakeOmni { recipient: Some(JID.into()), grants: other, sent: vec![] };
    assert_eq!(deliver(&mut fake, Some(JID), &msg()), Outcome::Failed("no_consent"), "another channel's consent is not omni's");
    assert!(down.sent.is_empty() && fake.sent.is_empty());
}
