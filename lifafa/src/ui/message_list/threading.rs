//! Conversations: grouping messages into threads (a union-find over
//! Message-ID and References, off the main thread for big folders), and
//! the pure rules for which row stands for which conversation.

use crate::models::Message;
use std::rc::Rc;

/// Which shown row stands for a reader key: the message's own row when the
/// list has one, and otherwise the row of the conversation it belongs to.
///
/// With expandable conversations off, a reply never gets a row of its own and
/// never will — the thread is only ever the one head row here. Without this
/// fallback the reader's selection would find nothing to select, and the
/// conversation would appear to deselect itself the moment one of its other
/// messages was clicked in the reading pane (#211). The head row stands for
/// the whole thread, so it is what stays lit however the user moves through
/// the cards.
///
/// A conversation reaches across folders, so some of its cards — the user's
/// own replies, pulled in from Sent — belong to no row in this folder at all
/// and are not in `msg_thread` either. `viewed` is the row the open
/// conversation was opened from, and `emitted` is that conversation as it was
/// handed to the reader: a card from it keeps that row lit.
pub(super) fn row_for_reader_key<M: std::borrow::Borrow<Message>>(
    key: &(u32, u32),
    shown: &[M],
    msg_thread: &std::collections::HashMap<(u32, u32), (u32, String)>,
    emitted: &[(u32, u32)],
    viewed: Option<(u32, u32)>,
) -> Option<usize> {
    let own_row = shown
        .iter()
        .map(|m| m.borrow())
        .position(|m| (m.account_id, m.id) == *key);
    let thread_row = || {
        let tkey = msg_thread.get(key)?;
        shown
            .iter()
            .map(|m| m.borrow())
            .position(|m| msg_thread.get(&(m.account_id, m.id)) == Some(tkey))
    };
    let viewed_row = || {
        if !emitted.contains(key) {
            return None;
        }
        let viewed = viewed?;
        shown
            .iter()
            .map(|m| m.borrow())
            .position(|m| (m.account_id, m.id) == viewed)
    };
    own_row.or_else(thread_row).or_else(viewed_row)
}

/// The conversation the reader is showing, for [`row_for_reader_key`]: what
/// this list `emitted` when the row was opened, plus whatever the app has
/// `merged` into it since from other folders — the user's own replies from
/// Sent, which reach the reader under ids this list never listed (#220).
pub(super) fn reader_conversation(
    emitted: &[(u32, u32)],
    merged: &[(u32, u32)],
) -> Vec<(u32, u32)> {
    let mut all = emitted.to_vec();
    all.extend(merged.iter().filter(|k| !emitted.contains(k)).copied());
    all
}

/// Whether the message at `key` is the row its conversation collapses to:
/// the oldest member among those the rows were grouped from, or a message
/// grouped with nothing there (#236).
///
/// Judged over `msg_thread` and `thread_members`, which the rebuild fills
/// from the rendered window, and not over everything the list holds. The
/// two differ exactly when a conversation's older messages sit past the
/// window: the row on screen is then the oldest member *shown*, while the
/// conversation's true head is further down, unrendered. Asked against the
/// whole list, that row failed the head test, was taken for a reply picked
/// out of an opened-up thread, and was shown alone in the reader, with no
/// look in the cache for the rest. The unified Inboxes hit this constantly:
/// several inboxes merged, newest first, put a conversation's start past
/// the rendered window within days, while one folder rarely does.
pub(super) fn heads_its_row(
    key: (u32, u32),
    msg_thread: &std::collections::HashMap<(u32, u32), (u32, String)>,
    thread_members: &std::collections::HashMap<(u32, String), Vec<(u32, u32)>>,
) -> bool {
    match msg_thread.get(&key) {
        None => true,
        Some(thread) => thread_members.get(thread).and_then(|m| m.first()) == Some(&key),
    }
}

/// Which of a conversation's `found` members, from anywhere in the account,
/// a row nests under the folder's `own` (#309): not the ones in a folder on
/// the list, which are rows of their own, nor a copy of mail already there
/// (Gmail files one message under every label it has), and only those the
/// list's filters let through. A message with no Message-ID cannot be told
/// apart from its copies, so it is left out.
pub(super) fn nested_members<M: std::borrow::Borrow<Message>>(
    found: &[Message],
    own: &[M],
    listed_folders: &std::collections::HashSet<(u32, u32)>,
    passes: &impl Fn(&Message) -> bool,
) -> Vec<Message> {
    let mut out: Vec<Message> = Vec::new();
    for m in found {
        if m.message_id.is_empty()
            || listed_folders.contains(&(m.account_id, m.folder_id))
            || !passes(m)
            || own
                .iter()
                .map(|o| o.borrow())
                .chain(&out)
                .any(|o| o.message_id == m.message_id)
        {
            continue;
        }
        out.push(m.clone());
    }
    out
}

/// The message a conversation's row should speak for when it is not one this
/// folder holds: the cache's newest member, if it is later than anything on
/// screen (#236).
///
/// A mail you answered is a single row in the Inbox, with the answer filed in
/// Sent. The row goes on showing the other side's last word and its time, so
/// nothing short of opening the conversation says you have replied. `None`
/// keeps the row exactly as the folder describes it: the cache knows of
/// nothing newer, or knows of nothing at all yet. Whether it is asked at all
/// is the "Show your own replies in the message list" setting, off by
/// default.
pub(super) fn latest_elsewhere(
    summary: Option<&crate::models::ThreadSummary>,
    newest_here: i64,
) -> Option<crate::models::ThreadLatest> {
    summary
        .and_then(|s| s.latest.as_ref())
        .filter(|l| l.timestamp > newest_here)
        .cloned()
}

/// Which of the page's conversations still need their real size looked up
/// (#222): the ones nobody has asked the cache about yet.
///
/// Every rebuild runs this, and rebuilds are cheap and frequent — a sync, a
/// scroll, each keystroke of a search. Asking is not cheap: it is a scan of the
/// account's message index. So a thread is asked about once and remembered.
/// That is also what stops the loop, since the answer arrives as a rebuild:
/// with every thread on the page already asked about, the next pass has nothing
/// to send and the list settles.
pub(super) fn unasked_threads(
    listed: &[(u32, String, Vec<String>)],
    asked: &std::collections::HashSet<(u32, String)>,
) -> Vec<(u32, String, Vec<String>)> {
    listed
        .iter()
        .filter(|(aid, root, _)| !asked.contains(&(*aid, root.clone())))
        .cloned()
        .collect()
}

/// What working out conversations reads of each message, to take to a
/// thread.
pub(super) fn thread_inputs(source: &[Rc<Message>]) -> Vec<ThreadInput> {
    source
        .iter()
        .map(|m| ThreadInput {
            account_id: m.account_id,
            folder_id: m.folder_id,
            id: m.id,
            uid: m.uid,
            message_id: m.message_id.clone(),
            references: m.references.clone(),
        })
        .collect()
}

/// What working out conversations reads of a message: from the message
/// itself, or from the copy of just these fields that a large list hands to
/// a background thread.
pub(super) trait ThreadFields {
    fn account_id(&self) -> u32;
    fn folder_id(&self) -> u32;
    fn id(&self) -> u32;
    fn uid(&self) -> u32;
    fn message_id(&self) -> &str;
    fn references(&self) -> &str;
}

impl ThreadFields for Message {
    fn account_id(&self) -> u32 {
        self.account_id
    }
    fn folder_id(&self) -> u32 {
        self.folder_id
    }
    fn id(&self) -> u32 {
        self.id
    }
    fn uid(&self) -> u32 {
        self.uid
    }
    fn message_id(&self) -> &str {
        &self.message_id
    }
    fn references(&self) -> &str {
        &self.references
    }
}

impl ThreadFields for Rc<Message> {
    fn account_id(&self) -> u32 {
        self.as_ref().account_id
    }
    fn folder_id(&self) -> u32 {
        self.as_ref().folder_id
    }
    fn id(&self) -> u32 {
        self.as_ref().id
    }
    fn uid(&self) -> u32 {
        self.as_ref().uid
    }
    fn message_id(&self) -> &str {
        &self.as_ref().message_id
    }
    fn references(&self) -> &str {
        &self.as_ref().references
    }
}

/// A message's threading fields, owned, to be worked on off the main loop.
pub(super) struct ThreadInput {
    account_id: u32,
    folder_id: u32,
    id: u32,
    uid: u32,
    message_id: String,
    references: String,
}

impl ThreadFields for ThreadInput {
    fn account_id(&self) -> u32 {
        self.account_id
    }
    fn folder_id(&self) -> u32 {
        self.folder_id
    }
    fn id(&self) -> u32 {
        self.id
    }
    fn uid(&self) -> u32 {
        self.uid
    }
    fn message_id(&self) -> &str {
        &self.message_id
    }
    fn references(&self) -> &str {
        &self.references
    }
}

/// Group messages into conversations by their reply headers (Message-ID linked
/// via In-Reply-To / References), scoped per account. Returns each message's
/// thread key `(account_id, root)`, looked up by [`thread_slot`]. Messages with
/// no reply relationship get a unique key (a thread of one) — so unrelated
/// messages that merely share a subject are never threaded together.
///
/// Age plays no part: a message threads because its headers say what it answers,
/// and those are indexed with every message. Grouping runs over the rendered
/// window, so covering the whole mailbox costs no more than covering a day of
/// it; what a conversation costs to *open* is bounded separately, by
/// `THREAD_MEMBER_LIMIT`.
pub(super) fn compute_thread_keys<M: ThreadFields>(
    msgs: &[M],
    links: &[(u32, String, String)],
) -> std::collections::HashMap<(u32, u32, u32), (u32, String)> {
    use std::collections::HashMap;

    // Union-find over message-id nodes (namespaced by account). The nodes
    // borrow their ids from the messages: a large folder is grouped whenever
    // it changes, and a String per id made most of the cost (#323).
    #[derive(Clone, Copy, PartialEq, Eq, Hash)]
    enum Node<'a> {
        Id(u32, &'a str),
        Uid(u32, u32, u32),
    }
    #[derive(Default)]
    struct Forest<'a> {
        index: HashMap<Node<'a>, usize>,
        nodes: Vec<Node<'a>>,
        parent: Vec<usize>,
    }
    impl<'a> Forest<'a> {
        fn node(&mut self, n: Node<'a>) -> usize {
            if let Some(&i) = self.index.get(&n) {
                return i;
            }
            let i = self.nodes.len();
            self.nodes.push(n);
            self.parent.push(i);
            self.index.insert(n, i);
            i
        }
        fn find(&mut self, mut x: usize) -> usize {
            while self.parent[x] != x {
                self.parent[x] = self.parent[self.parent[x]];
                x = self.parent[x];
            }
            x
        }
        fn union(&mut self, a: usize, b: usize) {
            let (ra, rb) = (self.find(a), self.find(b));
            if ra != rb {
                self.parent[ra] = rb;
            }
        }
    }
    // A message with its own Message-ID is a real node; one without gets a unique
    // node keyed by folder and uid so it only links through its references (if
    // any). A UID names a message in one folder only (#317).
    fn self_node<M: ThreadFields>(m: &M) -> Node<'_> {
        if m.message_id().is_empty() {
            Node::Uid(m.account_id(), m.folder_id(), m.uid())
        } else {
            Node::Id(m.account_id(), m.message_id())
        }
    }

    let mut forest = Forest::default();
    let mut own = Vec::with_capacity(msgs.len());
    for m in msgs {
        let sn = forest.node(self_node(m));
        own.push(sn);
        for r in m.references().split_whitespace() {
            let rn = forest.node(Node::Id(m.account_id(), r));
            forest.union(sn, rn);
        }
    }

    // Messages from elsewhere in the account contribute their links but never
    // appear: a reply in the Inbox and the one before it are two answers to the
    // same message in Sent, and without that message nothing says so.
    for (aid, id, refs) in links {
        let sn = forest.node(Node::Id(*aid, id));
        for r in refs.split_whitespace() {
            let rn = forest.node(Node::Id(*aid, r));
            forest.union(sn, rn);
        }
    }

    let mut names: HashMap<usize, String> = HashMap::new();
    let mut out = HashMap::with_capacity(msgs.len());
    for (m, sn) in msgs.iter().zip(own) {
        let root = forest.find(sn);
        let name = names
            .entry(root)
            .or_insert_with(|| match forest.nodes[root] {
                Node::Id(aid, id) => format!("{aid}\u{0}{id}"),
                Node::Uid(aid, folder, uid) => format!("{aid}\u{0}uid{folder}/{uid}"),
            });
        out.insert(
            (m.account_id(), m.folder_id(), m.id()),
            (m.account_id(), name.clone()),
        );
    }
    out
}

/// [`SourceThreads`] worked out from `msgs` (in the list's order).
pub(super) fn source_threads_of<M: ThreadFields>(
    msgs: &[M],
    links: &[(u32, String, String)],
    pool: bool,
) -> SourceThreads {
    let keys = compute_thread_keys(msgs, links);
    let mut members: std::collections::HashMap<(u32, String), Vec<usize>> =
        std::collections::HashMap::new();
    for (i, m) in msgs.iter().enumerate() {
        if let Some(key) = keys.get(&(m.account_id(), m.folder_id(), m.id())) {
            members.entry(key.clone()).or_default().push(i);
        }
    }
    SourceThreads {
        pool,
        keys,
        members,
    }
}

// Short: the map runs to the size of the folder.
impl std::fmt::Debug for SourceThreads {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SourceThreads({} messages)", self.keys.len())
    }
}

/// A list at least this long works out its conversations on a background
/// thread, and keeps its rows until the answer is in; below it, the work is
/// quick enough to do in place.
pub(super) const THREADS_OFF_MAIN: usize = 4000;

/// [`compute_thread_keys`] over everything a list holds, with each
/// conversation's members as positions in that same slice.
pub(crate) struct SourceThreads {
    /// Worked out from the search pool rather than the folder.
    pub(super) pool: bool,
    pub(super) keys: std::collections::HashMap<(u32, u32, u32), (u32, String)>,
    pub(super) members: std::collections::HashMap<(u32, String), Vec<usize>>,
}

/// Where [`compute_thread_keys`] files a message: its account, folder and id.
/// The id alone is a UID, which only means something inside one folder, and a
/// search over every folder holds the same UID many times; keyed without the
/// folder, one of them joins the other's conversation (#317).
pub(super) fn thread_slot(m: &Message) -> (u32, u32, u32) {
    (m.account_id, m.folder_id, m.id)
}
