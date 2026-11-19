//! Mail a helper decrypted (see `crate::cache::sealed`): it arrived
//! end-to-end encrypted, it is on disk as ordinary text, and the mark on
//! its row keeps it from assistants as OpenPGP mail is kept.

use super::*;

#[tokio::test]
async fn end_to_end_mail_a_helper_decrypted_is_out_of_reach_by_every_route() {
    let mut s = Session::start(ada_may(&AssistantCap::ALL), Ok(plain_raw("pineapple"))).await;
    let id = id_of(ADA, "Vault", 12);

    let list = data(&s.call("list_messages", json!({"folder":"Vault"})).await);
    let listed = list
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["id"] == id.as_str())
        .unwrap_or_else(|| panic!("{list}"));
    assert_eq!(listed["encrypted"], true, "{list}");
    assert_eq!(listed["preview"], "", "{list}");
    assert_eq!(listed["subject"], "Helper letter", "{list}");

    let read = s.call("read_message", json!({"id":id})).await;
    let out = data(&read);
    assert_eq!(out["encrypted"], true, "{read}");
    assert!(out.get("body").is_none(), "{read}");
    assert!(!read.to_string().contains("pineapple"), "{read}");

    for (tool, args) in [
        ("reply_to_message", json!({"id":id})),
        ("forward_message", json!({"id":id})),
        ("list_attachments", json!({"id":id})),
        ("read_attachment", json!({"id":id,"index":0})),
    ] {
        let result = s.call(tool, args).await;
        assert!(
            refused(&result) && result.to_string().contains("encrypted"),
            "{tool}: {result}"
        );
    }

    // Its preview and its text are on disk, and a search does not say so.
    for words in ["pineapple", "preview"] {
        let found = s
            .call("search", json!({"query":words,"folder":"Vault"}))
            .await;
        assert!(subjects(&found).is_empty(), "{words}: {found}");
    }
    // Its subject is an assistant's to see, and to find it by.
    let found = s.call("search", json!({"query":"helper"})).await;
    assert_eq!(subjects(&found), ["Helper letter"], "{found}");

    // Nothing was asked of the server, and nothing quoted into a composer.
    let saw = s.saw.lock().unwrap();
    assert_eq!((saw.fetched, saw.drafts), (0, 0));
}
