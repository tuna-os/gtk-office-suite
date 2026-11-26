use super::*;
use std::cell::RefCell;

struct Fake(RefCell<Option<DocumentContext>>);
impl ContextSource for Fake {
    fn current(&self) -> Option<DocumentContext> {
        self.0.borrow().clone()
    }
}

fn dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ctxmcp-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn setup(tag: &str) -> (Config, Grant, Fake, PathBuf) {
    let d = dir(tag);
    let f = d.join("a.odt");
    std::fs::write(&f, b"x").unwrap();
    let grant = Grant::new(&f).unwrap();
    let doc = DocumentContext { path: Some(f.clone()), title: "A".into(), mime: "application/vnd.oasis.opendocument.text".into(), revision: 3, text: "hello world".into(), ..Default::default() };
    (Config { enabled: true, ..Config::default() }, grant, Fake(RefCell::new(Some(doc))), d)
}

fn call(cfg: &Config, g: &Grant, s: &Fake, tool: &str, args: Value) -> Value {
    let line = json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":tool,"arguments":args}}).to_string();
    serde_json::from_str(&handle_line(cfg, g, s, &line).unwrap()).unwrap()
}

#[test]
fn disabled_by_default_reads_nothing() {
    assert!(!Config::default().enabled);
    let (_, g, s, _) = setup("dis");
    struct Boom;
    impl BufRead for Boom { fn fill_buf(&mut self) -> std::io::Result<&[u8]> { panic!("read while disabled") } fn consume(&mut self, _: usize) {} }
    impl std::io::Read for Boom { fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> { panic!("read while disabled") } }
    assert!(matches!(serve(&Config::default(), &g, &s, Boom, Vec::new()), Err(ServeError::Disabled)));
}

#[test]
fn lists_only_read_tools_and_rejects_others() {
    let (c, g, s, _) = setup("tools");
    let l: Value = serde_json::from_str(&handle_line(&c, &g, &s, r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#).unwrap()).unwrap();
    for t in l["result"]["tools"].as_array().unwrap() {
        assert_eq!(t["annotations"]["readOnlyHint"], true);
    }
    assert_eq!(l["result"]["tools"].as_array().unwrap().len(), 2);
    let r = call(&c, &g, &s, "office_write_text", json!({"text":"x"}));
    assert_eq!(r["error"]["code"], -32602);
}

#[test]
fn reads_snapshot_with_revision_and_selection_scope() {
    let (c, g, s, _) = setup("read");
    let r = call(&c, &g, &s, "office_get_context", json!({}));
    assert_eq!(r["result"]["structuredContent"]["revision"], 3);
    assert_eq!(r["result"]["structuredContent"]["scope"], "document");
    s.0.borrow_mut().as_mut().unwrap().selection = Some("world".into());
    let r = call(&c, &g, &s, "office_get_context", json!({}));
    assert_eq!(r["result"]["structuredContent"]["text"], "world");
    assert_eq!(r["result"]["structuredContent"]["scope"], "selection");
}

#[test]
fn stale_revision_is_refused() {
    let (c, g, s, _) = setup("stale");
    assert_eq!(call(&c, &g, &s, "office_get_context", json!({"expected_revision":3}))["result"]["isError"], Value::Null);
    s.0.borrow_mut().as_mut().unwrap().revision = 4;
    let r = call(&c, &g, &s, "office_get_context", json!({"expected_revision":3}));
    assert_eq!(r["result"]["structuredContent"]["error"], "stale_revision");
}

#[test]
fn revoked_selection_fails_closed() {
    let (c, g, s, _) = setup("rev");
    g.revoke();
    let r = call(&c, &g, &s, "office_get_context", json!({}));
    assert_eq!(r["result"]["structuredContent"]["error"], "selection_revoked");
}

#[test]
fn closed_or_other_document_is_refused() {
    let (c, g, s, d) = setup("other");
    let other = d.join("b.odt");
    std::fs::write(&other, b"y").unwrap();
    s.0.borrow_mut().as_mut().unwrap().path = Some(other);
    assert_eq!(call(&c, &g, &s, "office_get_context", json!({}))["result"]["structuredContent"]["error"], "document_not_open");
    *s.0.borrow_mut() = None;
    assert_eq!(call(&c, &g, &s, "office_get_metadata", json!({}))["result"]["structuredContent"]["error"], "document_not_open");
}

#[cfg(unix)]
#[test]
fn symlinks_and_non_files_cannot_be_granted() {
    use std::os::unix::fs::symlink;
    let d = dir("sym");
    let real = d.join("real.odt");
    std::fs::write(&real, b"x").unwrap();
    symlink(&real, d.join("link.odt")).unwrap();
    assert_eq!(Grant::new(&d.join("link.odt")).unwrap_err(), GrantError::Symlink);
    std::fs::create_dir(d.join("sub")).unwrap();
    std::fs::write(d.join("sub/f.odt"), b"x").unwrap();
    symlink(d.join("sub"), d.join("dirlink")).unwrap();
    assert_eq!(Grant::new(&d.join("dirlink/f.odt")).unwrap_err(), GrantError::Symlink);
    assert_eq!(Grant::new(&d.join("sub")).unwrap_err(), GrantError::NotRegularFile);
    assert_eq!(Grant::new(&d.join("nope")).unwrap_err(), GrantError::NotFound);
    // `..` escapes resolve to the same file, never to a different one.
    assert!(Grant::new(&d.join("sub/../real.odt")).is_ok());
}

#[cfg(unix)]
#[test]
fn file_swapped_for_symlink_after_grant_is_refused() {
    let (c, g, s, d) = setup("swap");
    let f = d.join("a.odt");
    std::fs::remove_file(&f).unwrap();
    std::os::unix::fs::symlink("/etc/hostname", &f).unwrap();
    assert_eq!(call(&c, &g, &s, "office_get_context", json!({}))["result"]["structuredContent"]["error"], "selection_invalid");
}

#[test]
fn size_limits_cut_visibly_on_char_boundary() {
    let (mut c, g, s, _) = setup("size");
    c.max_text_bytes = 4;
    c.max_table_cells = 3;
    {
        let mut b = s.0.borrow_mut();
        let d = b.as_mut().unwrap();
        d.text = "aé€€".into(); // 1 + 2 + 3 bytes: cut at 3 keeps "aé"
        d.tables = vec![TableSnapshot { name: "T".into(), rows: vec![vec!["1".into(), "2".into()], vec!["3".into(), "4".into()]] }];
    }
    let r = call(&c, &g, &s, "office_get_context", json!({}));
    let sc = &r["result"]["structuredContent"];
    assert_eq!(sc["text"], "aé");
    assert_eq!(sc["truncated"], true);
    assert_eq!(sc["tables"][0]["rows"].as_array().unwrap().len(), 1);
}

#[test]
fn oversized_request_line_is_refused_without_buffering_it() {
    let (mut c, g, s, _) = setup("big");
    c.max_request_bytes = 64;
    let input = format!("{}\n", "x".repeat(10_000));
    let mut out = Vec::new();
    serve(&c, &g, &s, input.as_bytes(), &mut out).unwrap();
    assert!(String::from_utf8(out).unwrap().contains("request too large"));
}

#[test]
fn serve_round_trip_over_pipes_and_notifications_get_no_reply() {
    let (c, g, s, _) = setup("pipe");
    let input = concat!(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#, "\n",
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#, "\n",
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"office_get_metadata"}}"#, "\n",
    );
    let mut out = Vec::new();
    serve(&c, &g, &s, input.as_bytes(), &mut out).unwrap();
    let lines: Vec<_> = String::from_utf8(out).unwrap().lines().map(String::from).collect();
    assert_eq!(lines.len(), 2);
    assert!(lines[1].contains("\\\"revision\\\":3"));
}
