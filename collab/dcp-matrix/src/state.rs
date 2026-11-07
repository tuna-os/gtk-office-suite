//! A server-authoritative view of one document room, parsed from
//! `GET /rooms/{id}/state` (not from the local store, so it needs no sync and a
//! lease read-back sees what the homeserver has right now).

use std::collections::HashMap;

use dcp_core::{Lease, Meta, Role, Stamped};
use serde_json::Value;

pub const ROOM_TYPE: &str = "org.tunaos.dcp.document";
pub const META: &str = "org.tunaos.dcp.meta";
pub const LEASE: &str = "org.tunaos.dcp.lease";
pub const VERSION: &str = "org.tunaos.dcp.version";
pub const UPDATE: &str = "org.tunaos.dcp.update";
pub const CLOCK: &str = "org.tunaos.dcp.clock";

/// `m.room.encrypted` is what the homeserver sees for every DCP timeline event
/// in an encrypted room, so it is the only timeline type a power-level table can
/// gate there.
pub const ENCRYPTED: &str = "m.room.encrypted";

pub const LEVEL_EDITOR: i64 = 50;
pub const LEVEL_OWNER: i64 = 100;

#[derive(Clone, Debug, Default)]
pub struct Power {
    pub users: HashMap<String, i64>,
    pub users_default: i64,
    pub events: HashMap<String, i64>,
    pub events_default: i64,
    pub state_default: i64,
    /// Room version 12 creators have unbounded power and are not listed in
    /// `users`.
    pub creators: Vec<String>,
}

impl Power {
    pub fn level_of(&self, user: &str) -> i64 {
        if self.creators.iter().any(|c| c == user) {
            return i64::MAX;
        }
        self.users.get(user).copied().unwrap_or(self.users_default)
    }

    pub fn role_of(&self, user: &str) -> Role {
        let l = self.level_of(user);
        if l >= LEVEL_OWNER {
            Role::Owner
        } else if l >= LEVEL_EDITOR {
            Role::Editor
        } else {
            Role::Viewer
        }
    }

    /// Whether the homeserver itself refuses DCP timeline events from members
    /// below the editor level. True only if `m.room.encrypted` is gated.
    pub fn server_gates_encrypted_timeline(&self) -> bool {
        self.events.get(ENCRYPTED).copied().unwrap_or(self.events_default) >= LEVEL_EDITOR
    }
}

#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub room_type: Option<String>,
    pub room_version: Option<String>,
    pub encrypted: bool,
    pub history_visibility: Option<String>,
    pub meta: Option<Stamped<Meta>>,
    pub lease: Option<Stamped<Lease>>,
    /// Latest lease/meta state events whose content did not validate.
    pub invalid_dcp_state: u32,
    pub power: Power,
    /// User ID to membership string (`join`, `invite`, `leave`, ...).
    pub membership: HashMap<String, String>,
    /// The raw `m.room.power_levels` content, so a role change edits it in place.
    pub power_levels_content: Value,
}

fn as_level(v: &Value) -> Option<i64> {
    v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

fn level_map(v: &Value) -> HashMap<String, i64> {
    v.as_object()
        .map(|o| o.iter().filter_map(|(k, v)| as_level(v).map(|l| (k.clone(), l))).collect())
        .unwrap_or_default()
}

impl Snapshot {
    pub fn from_events(events: &[Value]) -> Snapshot {
        let mut s = Snapshot { power_levels_content: Value::Null, ..Default::default() };
        let mut creator: Option<String> = None;
        let mut additional: Vec<String> = Vec::new();
        for ev in events {
            let ty = ev["type"].as_str().unwrap_or("");
            let key = ev["state_key"].as_str().unwrap_or("");
            let content = &ev["content"];
            match (ty, key) {
                ("m.room.create", "") => {
                    s.room_type = content["type"].as_str().map(str::to_owned);
                    s.room_version = content["room_version"].as_str().map(str::to_owned);
                    creator = ev["sender"].as_str().map(str::to_owned);
                    if let Some(a) = content["additional_creators"].as_array() {
                        additional = a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect();
                    }
                }
                ("m.room.encryption", "") => s.encrypted = true,
                ("m.room.history_visibility", "") => {
                    s.history_visibility = content["history_visibility"].as_str().map(str::to_owned)
                }
                ("m.room.power_levels", "") => {
                    s.power_levels_content = content.clone();
                    s.power.users = level_map(&content["users"]);
                    s.power.events = level_map(&content["events"]);
                    s.power.users_default = as_level(&content["users_default"]).unwrap_or(0);
                    s.power.events_default = as_level(&content["events_default"]).unwrap_or(0);
                    s.power.state_default = as_level(&content["state_default"]).unwrap_or(50);
                }
                ("m.room.member", user) if !user.is_empty() => {
                    if let Some(m) = content["membership"].as_str() {
                        s.membership.insert(user.to_owned(), m.to_owned());
                    }
                }
                (META, "") => match dcp_core::schema::parse::<Meta>(dcp_core::schema::Kind::Meta, content) {
                    Ok(m) => s.meta = stamp(ev, m),
                    Err(_) => s.invalid_dcp_state += 1,
                },
                (LEASE, "") => match dcp_core::schema::parse::<Lease>(dcp_core::schema::Kind::Lease, content) {
                    Ok(l) => s.lease = stamp(ev, l),
                    Err(_) => s.invalid_dcp_state += 1,
                },
                _ => {}
            }
        }
        // Room versions 12 and later: creators are not in `users`.
        let v12 = s.room_version.as_deref().and_then(|v| v.parse::<u32>().ok()).is_some_and(|v| v >= 12);
        if v12 {
            s.power.creators.extend(creator);
            s.power.creators.extend(additional);
        }
        s
    }
}

fn stamp<T>(ev: &Value, body: T) -> Option<Stamped<T>> {
    Some(Stamped::new(
        ev["event_id"].as_str()?,
        ev["origin_server_ts"].as_u64()?,
        ev["sender"].as_str()?,
        body,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn create(version: &str) -> Value {
        json!({"type": "m.room.create", "state_key": "", "sender": "@o:s", "content": {"type": ROOM_TYPE, "room_version": version}})
    }

    fn pl(users: Value) -> Value {
        json!({"type": "m.room.power_levels", "state_key": "", "sender": "@o:s", "content": {
            "users": users, "users_default": 0, "events": {"m.room.encrypted": 50, VERSION: 50}, "state_default": 100}})
    }

    #[test]
    fn roles_follow_power_levels() {
        let s = Snapshot::from_events(&[create("11"), pl(json!({"@o:s": 100, "@e:s": 50}))]);
        assert_eq!(s.room_type.as_deref(), Some(ROOM_TYPE));
        assert_eq!(s.power.role_of("@o:s"), Role::Owner);
        assert_eq!(s.power.role_of("@e:s"), Role::Editor);
        assert_eq!(s.power.role_of("@v:s"), Role::Viewer);
        assert!(s.power.server_gates_encrypted_timeline());
    }

    #[test]
    fn room_v12_creators_are_owners_without_a_users_entry() {
        let s = Snapshot::from_events(&[create("12"), pl(json!({}))]);
        assert_eq!(s.power.role_of("@o:s"), Role::Owner);
        let s11 = Snapshot::from_events(&[create("11"), pl(json!({}))]);
        assert_eq!(s11.power.role_of("@o:s"), Role::Viewer, "before v12 only the users table counts");
    }

    #[test]
    fn spec_literal_table_does_not_gate_the_encrypted_timeline() {
        let literal = json!({"type": "m.room.power_levels", "state_key": "", "sender": "@o:s", "content": {
            "users": {"@o:s": 100}, "users_default": 0, "events": {VERSION: 50, UPDATE: 50, LEASE: 50, META: 100}}});
        let s = Snapshot::from_events(&[create("11"), literal]);
        assert!(!s.power.server_gates_encrypted_timeline());
    }

    #[test]
    fn invalid_dcp_state_is_ignored_and_counted() {
        let bad = json!({"type": LEASE, "state_key": "", "event_id": "$l", "origin_server_ts": 5, "sender": "@o:s", "content": {"dcp": 1, "state": "held", "surprise": 1}});
        let s = Snapshot::from_events(&[create("11"), bad]);
        assert!(s.lease.is_none());
        assert_eq!(s.invalid_dcp_state, 1);
    }

    fn lease_event(content: Value) -> Value {
        json!({"type": LEASE, "state_key": "", "event_id": "$l", "origin_server_ts": 5, "sender": "@o:s", "content": content})
    }

    #[test]
    fn held_lease_without_renewal_is_invalid() {
        let held = json!({"dcp": 1, "state": "held", "holder": {"principal": "@o:s", "session": "s"}, "token": "t", "base_version": "$v", "duration_ms": 60000});
        let s = Snapshot::from_events(&[create("11"), lease_event(held)]);
        assert!(s.lease.is_none());
        assert_eq!(s.invalid_dcp_state, 1);
    }

    #[test]
    fn held_lease_with_renewal_is_stamped_and_base_version_is_optional() {
        let held = json!({"dcp": 1, "state": "held", "holder": {"principal": "@o:s", "session": "s"}, "token": "t", "renewal": 0, "duration_ms": 60000});
        let s = Snapshot::from_events(&[create("11"), lease_event(held)]);
        let l = s.lease.unwrap();
        assert_eq!((l.body.renewal, l.body.base_version), (Some(0), None));
    }

    #[test]
    fn held_lease_without_holder_is_invalid() {
        let held = json!({"dcp": 1, "state": "held", "token": "t", "renewal": 0, "duration_ms": 60000});
        let s = Snapshot::from_events(&[create("11"), lease_event(held)]);
        assert!(s.lease.is_none());
        assert_eq!(s.invalid_dcp_state, 1);
    }

    #[test]
    fn valid_lease_and_meta_are_stamped_from_the_event() {
        let lease = json!({"type": LEASE, "state_key": "", "event_id": "$l", "origin_server_ts": 1234, "sender": "@o:s", "content": {"dcp": 1, "state": "released"}});
        let s = Snapshot::from_events(&[create("11"), lease]);
        let l = s.lease.unwrap();
        assert_eq!((l.id.as_str(), l.ts_ms, l.sender.as_str()), ("$l", 1234, "@o:s"));
    }
}
