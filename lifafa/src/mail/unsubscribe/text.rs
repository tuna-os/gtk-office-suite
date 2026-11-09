//! The plain-text body: a URL on the line of (or after) the word,
//! "send an email to X", and "reply with UNSUBSCRIBE".

use crate::models::ReplyRoute;

use super::body::{has_medium, has_strong, squash, url_score, BodyFindings, MEDIUM, STRONG_TEXT};
use super::is_email;

/// Words in an instruction that mean "write to": "send an email to X".
const SEND_WORDS: &[&str] = &[
    "send",
    "email",
    "e-mail",
    "mail",
    "write",
    "message",
    "envía",
    "envia",
    "envoyer",
    "envoyez",
    "senden",
    "schreib",
    "invia",
    "stuur",
    "skicka",
    "wyślij",
    "отправ",
    "напиш",
];
/// Words in an instruction that mean "reply": "reply with STOP".
const REPLY_WORDS: &[&str] = &[
    "reply",
    "replying",
    "respond",
    "responding",
    "antworten",
    "antwort",
    "répondre",
    "répondez",
    "repondre",
    "responder",
    "responda",
    "rispondi",
    "rispondere",
    "antwoord",
    "svara",
    "odpowiedz",
    "ответ",
];
/// The words a reply is asked to carry, uppercased as a subject.
const REPLY_COMMANDS: &[&str] = &[
    "unsubscribe",
    "stop",
    "remove",
    "signoff",
    "leave",
    "quit",
    "end",
];

const TOKEN_BREAK: &[char] = &['<', '>', '"', '\'', '(', ')', '[', ']'];

/// A line that tells the reader to write back to leave, in words that never
/// say "unsubscribe": "Reply to this email with STOP in the subject line."
/// It needs a reply-or-send word and a command word the list named. The
/// ambiguous commands count only in capitals, as lists write them, so
/// "we stop sending at 5pm" and "reply to leave a comment" are not
/// instructions. Gives back the command, uppercased.
fn instruction_command(line: &str) -> Option<String> {
    let lower = line.to_lowercase();
    let asks = REPLY_WORDS.iter().any(|w| lower.contains(w))
        || SEND_WORDS.iter().any(|w| lower.contains(w));
    if !asks {
        return None;
    }
    line.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .find(|token| {
            let l = token.to_lowercase();
            REPLY_COMMANDS.contains(&l.as_str())
                && (matches!(l.as_str(), "unsubscribe" | "signoff")
                    || token.chars().all(|c| !c.is_lowercase()))
        })
        .map(|t| t.to_uppercase())
}

/// A plain-text body: a URL on the line of (or the two lines after) an
/// unsubscribe word, "send an email to X", and "reply with UNSUBSCRIBE".
pub(super) fn scan_text(text: &str, reply_addr: Option<&str>) -> BodyFindings {
    let mut out = BodyFindings::default();
    let lines: Vec<&str> = text.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let sq = squash(line);
        let asked = instruction_command(line);
        let strong = has_strong(&sq) || asked.is_some();
        let medium = has_medium(&sq);
        if !strong && !medium {
            continue;
        }
        let score = if strong { STRONG_TEXT } else { MEDIUM };
        let window = &lines[i.saturating_sub(1)..(i + 3).min(lines.len())];
        let joined = window.join(" ");
        let lower = joined.to_lowercase();
        // A web address near the word.
        for tok in joined.split(|c: char| c.is_whitespace() || TOKEN_BREAK.contains(&c)) {
            let tok = tok.trim_end_matches(['.', ',', ';', ':', '!', '?']);
            let l = tok.to_ascii_lowercase();
            if l.starts_with("http://") || l.starts_with("https://") {
                out.offer_web(tok.to_string(), score.max(url_score(tok)));
            } else if l.starts_with("www.") {
                out.offer_web(format!("https://{tok}"), score.max(url_score(tok)));
            }
        }
        if !strong {
            continue;
        }
        // "reply with UNSUBSCRIBE": the reply route, with the word asked for.
        if out.reply.is_none() && REPLY_WORDS.iter().any(|w| lower.contains(w)) {
            if let Some(to) = reply_addr {
                let subject = asked.clone().unwrap_or_else(|| command_in(&lower));
                out.reply = Some(ReplyRoute {
                    to: to.to_string(),
                    subject,
                });
            }
        }
        // "send an email to leave@list.example": the mailto route.
        if out.mailto.is_none() && SEND_WORDS.iter().any(|w| lower.contains(w)) {
            for tok in
                joined.split(|c: char| c.is_whitespace() || c == ',' || TOKEN_BREAK.contains(&c))
            {
                let tok = tok
                    .trim_end_matches(['.', ';', ':', '!', '?'])
                    .trim_start_matches("mailto:");
                if is_email(tok) {
                    let subject = asked.clone().unwrap_or_else(|| command_in(&lower));
                    out.mailto = Some(format!("mailto:{tok}?subject={subject}"));
                    break;
                }
            }
        }
    }
    out
}

/// The command word an instruction asks for, uppercased; UNSUBSCRIBE when
/// it names none.
fn command_in(lower: &str) -> String {
    REPLY_COMMANDS
        .iter()
        .find(|w| lower.contains(*w))
        .map(|w| w.to_uppercase())
        .unwrap_or_else(|| "UNSUBSCRIBE".to_string())
}
