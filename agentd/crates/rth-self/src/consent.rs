

use serde::{Deserialize, Serialize};

use crate::store::Lines;

pub const KNOWN_COPY_VERSIONS: &[u32] = &[1, 2];

pub const SLEEP_COPY_VERSION: u32 = 2;

pub fn copy_version_of(channel: Channel) -> u32 {
    match channel {
        Channel::Notification | Channel::Omni => 1,
        Channel::Sleep => SLEEP_COPY_VERSION,
    }
}

pub fn copy_version_ok(channel: Channel, copy_version: u32) -> bool {
    KNOWN_COPY_VERSIONS.contains(&copy_version) && copy_version == copy_version_of(channel)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    Notification,
    Omni,

    Sleep,
}

impl Channel {
    pub fn parse(s: &str) -> Option<Channel> {
        match s {
            "notification" => Some(Channel::Notification),
            "omni" => Some(Channel::Omni),
            "sleep" => Some(Channel::Sleep),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub channel: Channel,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recipient: Option<String>,
    pub copy_version: Option<u32>,
    pub granted_utc_ms: u64,
    pub revoked: bool,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

pub const TORN_REPAIR: &str = "torn_repair";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Grant {
    pub channel: Channel,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recipient: Option<String>,

    pub copy_version: u32,
    pub granted_utc_ms: u64,
}

pub fn recipient_ok(channel: Channel, recipient: Option<&str>) -> bool {
    match channel {
        Channel::Notification | Channel::Sleep => recipient.is_none(),
        Channel::Omni => recipient.and_then(|r| r.strip_suffix("@s.whatsapp.net")).is_some_and(|d| (5..=20).contains(&d.len()) && d.bytes().all(|b| b.is_ascii_digit())),
    }
}

pub fn fold(lines: &Lines) -> Vec<Grant> {
    if lines.torn || lines.corrupt {
        return Vec::new();
    }
    let mut latest: Vec<(Channel, Option<Entry>)> = vec![(Channel::Notification, None), (Channel::Omni, None), (Channel::Sleep, None)];
    for v in &lines.records {
        let Some(c) = v.get("channel").and_then(|c| c.as_str()).and_then(Channel::parse) else { continue };

        let e = serde_json::from_value::<Entry>(v.clone()).ok();
        latest.iter_mut().find(|(k, _)| *k == c).unwrap().1 = e;
    }
    latest
        .into_iter()
        .filter_map(|(_, e)| e)
        .filter(|e| !e.revoked && e.copy_version.is_some_and(|v| copy_version_ok(e.channel, v)) && recipient_ok(e.channel, e.recipient.as_deref()))
        .map(|e| Grant { channel: e.channel, recipient: e.recipient, copy_version: e.copy_version.unwrap_or_default(), granted_utc_ms: e.granted_utc_ms })
        .collect()
}

pub fn reset(lines: &Lines) -> bool {
    let (mut owed, mut any): (Vec<Channel>, bool) = (Vec::new(), false);
    for (i, v) in lines.records.iter().enumerate() {
        if v.get("reason").and_then(|r| r.as_str()) == Some(TORN_REPAIR) {

            let live = fold(&Lines { records: lines.records[..i].to_vec(), torn: false, corrupt: false });
            any |= live.is_empty() && owed.is_empty();
            for g in live {
                if !owed.contains(&g.channel) {
                    owed.push(g.channel);
                }
            }
        } else if let Some(e) = serde_json::from_value::<Entry>(v.clone()).ok().filter(|e| !e.revoked && e.copy_version.is_some_and(|cv| copy_version_ok(e.channel, cv)) && recipient_ok(e.channel, e.recipient.as_deref())) {
            owed.retain(|c| *c != e.channel);
            any = false;
        }
    }
    !owed.is_empty() || any
}

pub const PEOPLE_COPY_VERSIONS: &[u32] = &[2];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PeopleKind {
    Enrol,
    Revoke,
    Correct,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PeopleState {
    Pending,
    #[default]
    Done,
    Void,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PeopleChannel {
    App,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PeopleEntry {
    pub kind: PeopleKind,
    pub person_id: String,

    pub modality: Option<String>,

    pub model_id: Option<String>,
    pub copy_version: u32,
    pub at: u64,
    pub channel: PeopleChannel,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame_id: Option<String>,
    #[serde(default)]
    pub state: PeopleState,
}

pub fn people(lines: &Lines) -> Vec<PeopleEntry> {
    if lines.torn || lines.corrupt {
        return Vec::new();
    }
    lines
        .records
        .iter()
        .filter(|v| v.get("channel").and_then(|c| c.as_str()) == Some("app"))
        .filter_map(|v| serde_json::from_value::<PeopleEntry>(v.clone()).ok())
        .filter(|e| PEOPLE_COPY_VERSIONS.contains(&e.copy_version))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn lines(records: Vec<serde_json::Value>) -> Lines {
        Lines { records, torn: false, corrupt: false }
    }
    const JID: &str = "5511999999999@s.whatsapp.net";

    #[test]
    fn a7_the_latest_entry_per_channel_wins() {
        let l = lines(vec![
            json!({"channel": "omni", "recipient": JID, "copy_version": 1, "granted_utc_ms": 1, "revoked": false}),
            json!({"channel": "notification", "copy_version": 1, "granted_utc_ms": 2, "revoked": false}),
            json!({"channel": "omni", "recipient": JID, "copy_version": 1, "granted_utc_ms": 3, "revoked": true}),
        ]);
        assert_eq!(fold(&l), vec![Grant { channel: Channel::Notification, recipient: None, copy_version: 1, granted_utc_ms: 2 }]);
    }

    #[test]
    fn a7_an_unknown_or_missing_copy_version_reads_as_no_consent() {
        for v in [json!(2), json!(null), json!("1")] {
            let l = lines(vec![json!({"channel": "notification", "copy_version": v, "granted_utc_ms": 1, "revoked": false})]);
            assert_eq!(fold(&l), vec![], "copy_version {v}");
        }
        let l = lines(vec![json!({"channel": "notification", "granted_utc_ms": 1, "revoked": false})]);
        assert_eq!(fold(&l), vec![], "no copy_version");

        let l = lines(vec![
            json!({"channel": "notification", "copy_version": 1, "granted_utc_ms": 1, "revoked": false}),
            json!({"channel": "notification", "copy_version": 9, "granted_utc_ms": 2, "revoked": false}),
        ]);
        assert_eq!(fold(&l), vec![]);
    }

    #[test]
    fn a7_a_torn_or_corrupt_file_reads_as_no_consent() {
        let good = json!({"channel": "notification", "copy_version": 1, "granted_utc_ms": 1, "revoked": false});
        assert_eq!(fold(&lines(vec![good.clone()])).len(), 1);
        assert_eq!(fold(&Lines { records: vec![good.clone()], torn: true, corrupt: false }), vec![]);
        assert_eq!(fold(&Lines { records: vec![good], torn: false, corrupt: true }), vec![]);
    }

    #[test]
    fn a7_omni_needs_a_whatsapp_recipient_and_notification_none() {
        assert!(recipient_ok(Channel::Omni, Some(JID)));
        for bad in [None, Some("1234@s.whatsapp.net"), Some("5511999999999@g.us"), Some("55119999x9999@s.whatsapp.net"), Some("123456789012345678901@s.whatsapp.net")] {
            assert!(!recipient_ok(Channel::Omni, bad), "{bad:?}");
        }
        assert!(recipient_ok(Channel::Notification, None) && !recipient_ok(Channel::Notification, Some(JID)));
        let l = lines(vec![json!({"channel": "omni", "recipient": "nope", "copy_version": 1, "granted_utc_ms": 1, "revoked": false})]);
        assert_eq!(fold(&l), vec![]);
    }

    #[test]
    fn each_channel_takes_only_its_own_consent_text() {
        assert!(copy_version_ok(Channel::Sleep, 2) && !copy_version_ok(Channel::Sleep, 1));
        for c in [Channel::Notification, Channel::Omni] {
            assert!(copy_version_ok(c, 1) && !copy_version_ok(c, 2), "{c:?}");
        }
        assert_eq!(PEOPLE_COPY_VERSIONS, &[2], "People keeps its own list");
    }

    #[test]
    fn s1_sleep_is_a_consent_channel_with_no_recipient() {
        let on = json!({"channel": "sleep", "copy_version": 2, "granted_utc_ms": 4, "revoked": false});
        assert_eq!(serde_json::to_value(fold(&lines(vec![on.clone()]))).unwrap(), json!([{"channel": "sleep", "copy_version": 2, "granted_utc_ms": 4}]));
        let off = json!({"channel": "sleep", "copy_version": 2, "granted_utc_ms": 5, "revoked": true});
        assert_eq!(fold(&lines(vec![on.clone(), off])), vec![]);
        let with_recipient = json!({"channel": "sleep", "recipient": JID, "copy_version": 2, "granted_utc_ms": 6, "revoked": false});
        assert_eq!(fold(&lines(vec![with_recipient])), vec![]);
        let outward_text = json!({"channel": "sleep", "copy_version": 1, "granted_utc_ms": 7, "revoked": false});
        assert_eq!(fold(&lines(vec![outward_text])), vec![], "the night's consent is its own disclosure's, never the outward text's");
    }
}

#[cfg(test)]
mod pv05_g6 {
    use super::*;
    use serde_json::json;

    fn lines(records: Vec<serde_json::Value>) -> Lines {
        Lines { records, torn: false, corrupt: false }
    }

    #[test]
    fn a_people_entry_of_copy_version_2_is_accepted_and_known_copy_versions_is_1_and_2() {
        assert_eq!(KNOWN_COPY_VERSIONS, &[1, 2], "the outward consent texts are unchanged; 2 is the night's");
        assert_eq!(PEOPLE_COPY_VERSIONS, &[2], "every voice-only People text is version 2; version 1 never shipped");
        let enrol = json!({"kind": "enrol", "person_id": "p-1", "modality": "face", "model_id": null, "copy_version": 2, "at": 5, "channel": "app"});
        let revoke = json!({"kind": "revoke", "person_id": "p-1", "modality": null, "model_id": null, "copy_version": 2, "at": 6, "channel": "app"});
        let unknown = json!({"kind": "enrol", "person_id": "p-2", "modality": "face", "model_id": null, "copy_version": 1, "at": 7, "channel": "app"});
        let outward = json!({"channel": "notification", "copy_version": 1, "granted_utc_ms": 1, "revoked": false});
        let l = lines(vec![enrol, revoke, unknown, outward]);
        let got: Vec<(PeopleKind, String)> = people(&l).into_iter().map(|e| (e.kind, e.person_id)).collect();
        assert_eq!(got, vec![(PeopleKind::Enrol, "p-1".into()), (PeopleKind::Revoke, "p-1".into())], "copy version 1 is not one this build knows");
        assert_eq!(fold(&l), vec![Grant { channel: Channel::Notification, recipient: None, copy_version: 1, granted_utc_ms: 1 }], "People entries are never an outward consent");
        assert!(people(&Lines { records: l.records.clone(), torn: true, corrupt: false }).is_empty(), "a torn file vouches for nothing");
    }

    #[test]
    fn a_people_entry_is_pending_then_done_or_void_and_one_without_a_state_is_done() {
        let base = json!({"kind": "revoke", "person_id": "p-1", "modality": null, "model_id": null, "copy_version": 2, "at": 5, "channel": "app", "frame_id": "f2"});
        let with = |state: &str| {
            let mut v = base.clone();
            v["state"] = json!(state);
            v
        };
        let l = lines(vec![base.clone(), with("pending"), with("done"), with("void")]);
        let got: Vec<PeopleState> = people(&l).into_iter().map(|e| e.state).collect();
        assert_eq!(got, vec![PeopleState::Done, PeopleState::Pending, PeopleState::Done, PeopleState::Void]);
        assert_eq!(serde_json::to_value(&people(&l)[1]).unwrap()["state"], "pending", "written explicitly");
    }
}
