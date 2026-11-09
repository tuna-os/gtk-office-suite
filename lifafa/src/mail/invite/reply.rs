//! The `METHOD:REPLY` document an answer to an invitation is sent as.

use gtk::glib;

use crate::models::Invite;

use super::ics::{escape, fold};

/// The iCalendar document that answers an invitation (RFC 5546 §3.2.3): the
/// same event, `METHOD:REPLY`, with the organizer and exactly one attendee —
/// the person answering, with their new `PARTSTAT`.
///
/// `SEQUENCE` and `RECURRENCE-ID` are carried over unchanged: they are how
/// the organizer's calendar knows which version of which occurrence is being
/// answered. Without them a reply to an updated meeting is applied to the
/// original, or to every occurrence of a series.
pub(crate) fn reply_ics(invite: &Invite, name: &str, email: &str, status: &str) -> String {
    let mut out = String::new();
    let mut line = |text: String| {
        out.push_str(&fold(&text));
        out.push_str("\r\n");
    };
    line("BEGIN:VCALENDAR".into());
    line("PRODID:-//Hyprlab//Hylki//EN".into());
    line("VERSION:2.0".into());
    line("METHOD:REPLY".into());
    line("BEGIN:VEVENT".into());
    line(format!("UID:{}", escape(&invite.uid)));
    line(format!("SEQUENCE:{}", invite.sequence));
    line(format!("DTSTAMP:{}", stamp_utc(crate::datefmt::now())));
    if !invite.recurrence_id.is_empty() {
        line(format!("RECURRENCE-ID:{}", invite.recurrence_id));
    }
    if !invite.summary.is_empty() {
        line(format!("SUMMARY:{}", escape(&invite.summary)));
    }
    // The organizer's calendar matches the reply on UID, but a reply with
    // no ORGANIZER is rejected outright by Exchange.
    if let Some(o) = &invite.organizer {
        line(format!("ORGANIZER{}:mailto:{}", cn(&o.name), o.email));
    }
    line(format!(
        "ATTENDEE{}PARTSTAT={status}:mailto:{email}",
        match cn(name).as_str() {
            "" => ";".to_string(),
            c => format!("{c};"),
        }
    ));
    line("END:VEVENT".into());
    line("END:VCALENDAR".into());
    out
}

/// `;CN="Ada Lovelace"`, or nothing for someone with no name on file.
fn cn(name: &str) -> String {
    let name = name.trim();
    if name.is_empty() {
        return String::new();
    }
    format!(";CN=\"{}\"", name.replace(['"', '\\'], ""))
}

/// A unix time as iCalendar UTC: `20261102T080000Z`.
fn stamp_utc(ts: i64) -> String {
    glib::DateTime::from_unix_utc(ts)
        .ok()
        .and_then(|d| d.format("%Y%m%dT%H%M%SZ").ok())
        .map(|s| s.to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mail::invite::parse;
    use crate::mail::invite::tests::ZIMBRA;

    #[test]
    fn the_reply_carries_the_events_identity_and_one_attendee() {
        let inv = parse(ZIMBRA).expect("an invitation");
        let ics = reply_ics(&inv, "Emmanuel P", "me@example.com", "ACCEPTED");
        assert!(ics.contains("METHOD:REPLY"));
        assert!(ics.contains("UID:26a4ff0c-1b6f"));
        assert!(ics.contains("SEQUENCE:0"));
        assert!(ics.contains("ORGANIZER;CN=\"Chloé Mercier\":mailto:chloe@example.org"));
        assert!(ics.contains("ATTENDEE;CN=\"Emmanuel P\";PARTSTAT=ACCEPTED:mailto:me@example.com"));
        // Exactly one attendee: the room and the other guests are not
        // answering.
        assert_eq!(ics.matches("ATTENDEE").count(), 1);
        assert!(ics.ends_with("END:VCALENDAR\r\n"));
    }

    #[test]
    fn a_reply_to_one_occurrence_says_which() {
        let ics = "BEGIN:VCALENDAR\r\nMETHOD:REQUEST\r\nBEGIN:VEVENT\r\nUID:u\r\nSEQUENCE:3\r\n\
            RECURRENCE-ID;TZID=Europe/Paris:20261102T090000\r\nSUMMARY:Standup\r\n\
            DTSTART:20261102T080000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let inv = parse(ics).expect("an invitation");
        let reply = reply_ics(&inv, "", "me@example.com", "DECLINED");
        assert!(reply.contains("SEQUENCE:3"));
        assert!(reply.contains("RECURRENCE-ID:20261102T090000"));
        assert!(reply.contains("ATTENDEE;PARTSTAT=DECLINED:mailto:me@example.com"));
    }

    #[test]
    fn long_lines_are_folded_on_the_way_out() {
        let ics = format!(
            "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:{}\r\nDTSTART:20261102T080000Z\r\n\
             END:VEVENT\r\nEND:VCALENDAR\r\n",
            "x".repeat(200)
        );
        let inv = parse(&ics).expect("an event");
        let reply = reply_ics(&inv, "", "me@example.com", "TENTATIVE");
        assert!(reply.lines().all(|l| l.len() <= 75));
        // And folds back to the same value.
        assert_eq!(parse(&reply).unwrap().uid.len(), 200);
    }
}
