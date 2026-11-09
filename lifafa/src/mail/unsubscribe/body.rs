//! Reading the body for an unsubscribe offer: what was found, and how sure
//! a link is, by its text, its URL and the sentence around it.

use crate::models::ReplyRoute;

/// What the body offered: the best web link with its score, a `mailto:`,
/// and a reply instruction.
#[derive(Debug, Default)]
pub(super) struct BodyFindings {
    pub(super) web: Option<(String, u8)>,
    pub(super) mailto: Option<String>,
    pub(super) reply: Option<ReplyRoute>,
}

impl BodyFindings {
    pub(super) fn is_empty(&self) -> bool {
        self.web.is_none() && self.mailto.is_none() && self.reply.is_none()
    }

    pub(super) fn merge(&mut self, other: BodyFindings) {
        if let Some((url, score)) = other.web {
            self.offer_web(url, score);
        }
        if self.mailto.is_none() {
            self.mailto = other.mailto;
        }
        if self.reply.is_none() {
            self.reply = other.reply;
        }
    }

    pub(super) fn offer_web(&mut self, url: String, score: u8) {
        if score > 0 && self.web.as_ref().is_none_or(|(_, s)| score > *s) {
            self.web = Some((url, score));
        }
    }
}

/// How sure a link is: the text says it outright, the URL says it, the
/// sentence around a "click here" says it, or only a preferences page.
pub(super) const STRONG_TEXT: u8 = 4;
const STRONG_URL: u8 = 3;
const CONTEXT: u8 = 3;
pub(super) const MEDIUM: u8 = 2;

/// Words that mean "unsubscribe", squashed (see [`squash`]) so they match
/// whatever the spacing, hyphens or case. English first, then the languages
/// bulk mail is commonly sent in.
const STRONG_WORDS: &[&str] = &[
    "unsubscribe",
    "unsubcribe",
    "unsuscribe",
    "unsubscribing",
    "optout",
    "signoff",
    "stopreceiving",
    "nolongerwishtoreceive",
    "nolongerwanttoreceive",
    "nolongerreceive",
    "removeme",
    "removeyourself",
    "removefromthislist",
    "removefromlist",
    "removefromourlist",
    "leavethislist",
    "leavelist",
    "leavethegroup",
    "cancelsubscription",
    "cancelyoursubscription",
    "cancelmysubscription",
    "endsubscription",
    "stopemails",
    "stoptheseemails",
    "stopthesemessages",
    "turnoffthesenotifications",
    "turnoffnotifications",
    // German
    "abmelden",
    "abbestellen",
    "austragen",
    "newsletterabmelden",
    "keineweiteren",
    // French
    "désabonner",
    "desabonner",
    "désinscrire",
    "desinscrire",
    "désinscription",
    "desinscription",
    "désabonnement",
    "desabonnement",
    "neplusrecevoir",
    // Spanish
    "darsedebaja",
    "darmedebaja",
    "dardebaja",
    "bajadelalista",
    "cancelarsuscripción",
    "cancelarsuscripcion",
    "anularsuscripción",
    "anularsuscripcion",
    "desuscribir",
    "dejarderecibir",
    // Portuguese
    "cancelarsubscrição",
    "cancelarsubscricao",
    "cancelarinscrição",
    "cancelarinscricao",
    "descadastrar",
    "desinscrever",
    "sairdalista",
    "deixarderecebir",
    // Italian
    "disiscriviti",
    "disiscrizione",
    "annullaiscrizione",
    "cancellaiscrizione",
    "cancellati",
    "rimuovimi",
    "nonriceverepiù",
    // Dutch
    "afmelden",
    "uitschrijven",
    "uitschrijving",
    "afmelding",
    // Swedish, Danish, Norwegian, Finnish
    "avregistrera",
    "avslutaprenumeration",
    "avprenumerera",
    "afmeld",
    "frameld",
    "opsigabonnement",
    "avmeld",
    "avsluttabonnement",
    "meldavabonnement",
    "peruutatilaus",
    "poistatilaus",
    "perutilaus",
    "lopetatilaus",
    // Polish, Czech, Slovak, Hungarian, Romanian
    "wypiszsię",
    "wypiszsie",
    "rezygnacja",
    "anulujsubskrypcję",
    "anulujsubskrypcje",
    "zrezygnuj",
    "odhlásit",
    "odhlasit",
    "odhlášení",
    "odhlaseni",
    "zrušitodběr",
    "zrusitodber",
    "odhlásiť",
    "leiratkozás",
    "leiratkozas",
    "leiratkozom",
    "dezabonare",
    "dezabonează",
    "dezaboneaza",
    // Russian, Ukrainian, Greek, Turkish
    "отписаться",
    "отписка",
    "отменитьподписку",
    "отказатьсяотрассылки",
    "відписатися",
    "відписка",
    "απεγγραφή",
    "κατάργησηεγγραφής",
    "διαγραφήαπότηλίστα",
    "aboneliktençık",
    "aboneliktencik",
    "abonelikiptal",
    "aboneliğiiptal",
    "listedençık",
    // Japanese, Chinese, Korean
    "配信停止",
    "購読解除",
    "登録解除",
    "配信解除",
    "購読をやめる",
    "受信拒否",
    "配信を停止",
    "取消订阅",
    "退订",
    "取消訂閱",
    "退訂",
    "구독취소",
    "수신거부",
    "구독해지",
    // Arabic, Hebrew, Indonesian/Malay, Vietnamese, Thai, Hindi
    "إلغاءالاشتراك",
    "الغاءالاشتراك",
    "הסרהמרשימתהתפוצה",
    "להסרהמרשימת",
    "הסרמרשימת",
    "berhentiberlangganan",
    "berhentilangganan",
    "hủyđăngký",
    "huydangky",
    "ยกเลิกการสมัคร",
    "ยกเลิกรับข่าวสาร",
    "सदस्यतारद्द",
    "सदस्यतासमाप्त",
];

/// Words that mean a preferences page, where unsubscribing is one choice.
const MEDIUM_WORDS: &[&str] = &[
    "emailpreference",
    "communicationpreference",
    "notificationpreference",
    "subscriptionpreference",
    "managepreference",
    "manageyourpreference",
    "updatepreference",
    "updateyourpreference",
    "changeyourpreference",
    "preferencecenter",
    "preferencecentre",
    "managesubscription",
    "manageyoursubscription",
    "managenotification",
    "notificationsetting",
    "emailsetting",
    "subscriptionsetting",
    "manageemail",
    "managealert",
    "manageyouremail",
    "mailingpreference",
];

/// Link texts that say nothing by themselves; the sentence around them does.
const GENERIC_TEXTS: &[&str] = &[
    "here",
    "clickhere",
    "click",
    "thislink",
    "link",
    "thispage",
    "page",
    "this",
    "tap",
    "taphere",
    "clic",
    "cliquezici",
    "ici",
    "hier",
    "klickhier",
    "klickensiehier",
    "aquí",
    "aqui",
    "hagaclicaquí",
    "cliqueaqui",
    "qui",
    "clicca",
    "cliccaqui",
    "klikhier",
    "kliktutaj",
    "tutaj",
    "здесь",
    "тут",
    "こちら",
    "这里",
    "這裡",
    "여기",
];

/// What a URL says, lowercased and looked for as a substring.
const STRONG_URL_WORDS: &[&str] = &[
    "unsubscribe",
    "unsub",
    "optout",
    "opt-out",
    "opt_out",
    "/oo/",
    "signoff",
    "sign-off",
    "desabonn",
    "desinscri",
    "abmeld",
    "abbestell",
    "afmeld",
    "uitschrijv",
    "darsedebaja",
    "darse-de-baja",
    "descadastr",
    "disiscri",
    "wypisz",
    "otpis",
    "remove-me",
    "removeme",
    "remove_me",
    "/remove",
    "leave-list",
    "/leave",
];
const MEDIUM_URL_WORDS: &[&str] = &[
    "preference",
    "subscription",
    "/manage",
    "email-settings",
    "emailsettings",
    "notification",
];

/// Text as the word lists are matched: lowercased, letters and digits only.
pub(super) fn squash(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

pub(super) fn has_strong(squashed: &str) -> bool {
    STRONG_WORDS.iter().any(|w| squashed.contains(w))
}

pub(super) fn has_medium(squashed: &str) -> bool {
    MEDIUM_WORDS.iter().any(|w| squashed.contains(w))
}

fn is_generic(squashed: &str) -> bool {
    GENERIC_TEXTS.contains(&squashed)
}

pub(super) fn url_score(url: &str) -> u8 {
    let l = url.to_ascii_lowercase();
    if STRONG_URL_WORDS.iter().any(|w| l.contains(w)) {
        STRONG_URL
    } else if MEDIUM_URL_WORDS.iter().any(|w| l.contains(w)) {
        MEDIUM
    } else {
        0
    }
}

/// Score a link by its text, its URL and the sentence it sits in.
pub(super) fn link_score(text: &str, url: &str, context: &str) -> u8 {
    let text = squash(text);
    let ctx = squash(context);
    let mut score = 0u8;
    if has_strong(&text) {
        score = STRONG_TEXT;
    } else if is_generic(&text) && has_strong(&ctx) {
        score = CONTEXT;
    } else if has_medium(&text) {
        score = MEDIUM;
    } else if is_generic(&text) && has_medium(&ctx) {
        score = 1;
    }
    score.max(url_score(url))
}
