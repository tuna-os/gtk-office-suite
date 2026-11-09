//! Tests of the LDAP directories: suggestions, server addresses, the
//! failure reasons and the EDS key files; `live*` need a running EDS.

use super::*;

#[test]
fn a_directory_entry_gives_a_suggestion_per_address() {
    let card =
        "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Alan Turing\r\nEMAIL;TYPE=OTHER:alan@example.org\r\n\
                EMAIL;TYPE=OTHER:turing@example.org\r\nEND:VCARD\r\n";
    let s = suggestions_from_vcard(card);
    assert_eq!(s.len(), 2);
    assert_eq!(
        (s[0].name.as_str(), s[0].email.as_str()),
        ("Alan Turing", "alan@example.org")
    );
    assert_eq!(s[1].email, "turing@example.org");
    assert!(s.iter().all(|s| s.from_contacts && s.from_directory));
}

/// Save a directory through a real EDS, check it as Settings does, and
/// remove it again: `LDAP_LIVE=server|base|bind dn|password`, the server
/// as typed in Settings (`ldaps://host:636` and all). Run it on a sealed
/// session bus with a keyring of its own (see the EDS rig), never the
/// desktop's.
#[test]
#[ignore = "needs LDAP_LIVE (a live account or service); run by hand"]
fn live() {
    let Ok(spec) = std::env::var("LDAP_LIVE") else {
        return;
    };
    let mut f = spec.splitn(4, '|').map(str::to_string);
    let (server, base, bind, pw) = (
        f.next().unwrap(),
        f.next().unwrap(),
        f.next().unwrap(),
        f.next().unwrap(),
    );
    let (host, security, port) = parse_server(&server);
    let mut d = Directory::empty();
    d.name = "live".into();
    d.host = host;
    d.security = security.unwrap_or(Security::StartTls);
    d.port = port.unwrap_or(d.security.default_port());
    d.base_dn = base;
    d.bind_dn = bind;
    // LDAP_LIVE_OLD=1: the source as Hylki wrote it before #307's fix,
    // encryption under the key EDS never read.
    let uid = if std::env::var_os("LDAP_LIVE_OLD").is_some() {
        d.uid = "hylkiliveold".into();
        let old = d
            .to_source()
            .replace("[Security]\nMethod=", "[Old]\nX=")
            .replace(
                "[LDAP Backend]\n",
                &format!("[LDAP Backend]\nSecurityMethod={}\n", d.security.key()),
            );
        let sources: HashMap<&str, String> = HashMap::from([("hylkiliveold", old)]);
        bus()
            .unwrap()
            .call_method(
                Some(registry().unwrap().as_str()),
                MANAGER_PATH,
                Some(MANAGER_IFACE),
                "CreateSources",
                &(sources,),
            )
            .expect("old source");
        crate::config::store_cloud_password("ldap:hylkiliveold", &pw).unwrap();
        "hylkiliveold".to_string()
    } else {
        save(&d, Some(&pw)).expect("saved")
    };
    let mut found = None;
    for _ in 0..20 {
        found = list().unwrap().into_iter().find(|x| x.uid == uid);
        if found.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let found = found.expect("listed");
    println!(
        "saved host={:?} port={} security={:?}",
        found.host, found.port, found.security
    );
    let t = Instant::now();
    println!("check: {:?} after {:?}", check(&found), t.elapsed());
    remove(&found).expect("removed");
}

/// Search the directories already in EDS, as the composer and Contacts
/// do: `LDAP_LIVE_SEARCH=<typed>`, twice, so the second shows a failed
/// directory is not asked again but still reported. On the EDS rig
/// only, as for `live`.
#[test]
#[ignore = "needs LDAP_LIVE_SEARCH (a live account or service); run by hand"]
fn live_search() {
    let Ok(typed) = std::env::var("LDAP_LIVE_SEARCH") else {
        return;
    };
    for round in 1..=2 {
        let t = Instant::now();
        let (found, errors) = search(&typed);
        println!("round {round} after {:?}", t.elapsed());
        for s in &found {
            println!("  suggestion: {} <{}>", s.name, s.email);
        }
        for e in &errors {
            println!("  error: {}", e.text());
        }
    }
    // A failed directory rests, so this one asks only those that
    // answered, and names the one resting.
    let (entries, errors) = search_contacts(&typed, 50);
    for c in &entries {
        println!(
            "  entry: {} {:?} in {}",
            c.name,
            c.primary_email(),
            c.book_name
        );
    }
    println!(
        "  contacts errors: {:?}",
        errors
            .iter()
            .map(DirectoryFailure::text)
            .collect::<Vec<_>>()
    );
    for d in list().unwrap() {
        println!("  {} last error: {:?}", d.title(), last_error(&d.uid));
    }
}

#[test]
fn a_server_typed_as_a_url_is_taken_apart() {
    assert_eq!(
        parse_server("ldaps://ldap.example.com"),
        ("ldap.example.com".into(), Some(Security::Ldaps), None)
    );
    assert_eq!(
        parse_server(" ldaps://ldap.example.com:3269/ou=x,dc=example,dc=com "),
        ("ldap.example.com".into(), Some(Security::Ldaps), Some(3269))
    );
    assert_eq!(
        parse_server("LDAP://ldap.example.com:389"),
        ("ldap.example.com".into(), None, Some(389))
    );
    assert_eq!(
        parse_server("ldap.example.com"),
        ("ldap.example.com".into(), None, None)
    );
    assert_eq!(
        parse_server("ldap.example.com:636"),
        ("ldap.example.com".into(), None, Some(636))
    );
    assert_eq!(parse_server("[::1]:636"), ("::1".into(), None, Some(636)));
    assert_eq!(parse_server("::1"), ("::1".into(), None, None));
}

#[test]
fn the_recorded_reason_beats_not_connected() {
    let not_connected = || "Cannot get contact list: Not connected".to_string();
    let last = |reason: &str, message: &str| LastCredentials {
        reason: reason.into(),
        message: message.into(),
    };
    let mut d = Directory::empty();
    d.host = "ldap.example.com".into();
    d.port = 636;
    d.security = Security::Ldaps;
    assert_eq!(
        connect_error(
            &d,
            &last("error", "Can't contact LDAP server"),
            not_connected()
        ),
        "Can't contact LDAP server"
    );
    assert!(
        connect_error(&d, &last("error", "Repository offline"), not_connected())
            .contains("ldap.example.com:636 over TLS")
    );
    assert!(
        connect_error(&d, &last("error", "TLS not available"), not_connected())
            .contains("StartTLS")
    );
    assert!(connect_error(&d, &last("ssl_failed", ""), not_connected()).contains("certificate"));
    assert!(connect_error(&d, &last("rejected", ""), not_connected()).contains("password"));
    assert_eq!(
        connect_error(&d, &last("", ""), not_connected()),
        not_connected()
    );
}

#[test]
fn query_text_is_quoted_for_eds() {
    assert_eq!(sexp_str(r#"a"b\c"#), r#""a\"b\\c""#);
}

#[test]
fn a_source_round_trips_and_keeps_what_hylki_does_not_show() {
    let data = "[Data Source]\nDisplayName=Work\nEnabled=true\nParent=ldap-stub\n\n\
                [Address Book]\nBackendName=ldap\n\n\
                [Authentication]\nHost=ldap.example.org\nPort=636\nUser=cn=me,dc=example,dc=org\nMethod=ldap/simple-binddn\n\n\
                [LDAP Backend]\nRootDn=dc=example,dc=org\nScope=onelevel\nFilter=(ou=staff)\n\n\
                [Security]\nMethod=ldaps\n";
    let d = Directory::from_source("abc", "/p", data).unwrap();
    assert_eq!(d.host, "ldap.example.org");
    assert_eq!(d.port, 636);
    assert_eq!(d.security, Security::Ldaps);
    assert!(!d.subtree);
    assert_eq!(d.bind_dn, "cn=me,dc=example,dc=org");
    let mut anon = d.clone();
    anon.bind_dn.clear();
    let out = anon.to_source();
    assert!(out.contains("Filter=(ou=staff)"));
    assert!(out.contains("Method=none"));
    assert!(Directory::from_source("x", "/p", "[Address Book]\nBackendName=carddav\n").is_none());
}

/// #307: Hylki once wrote the encryption where EDS never looks, so EDS
/// ran every such directory unencrypted; reading takes the old key, and
/// saving moves it to the one EDS reads.
#[test]
fn the_old_security_key_is_read_and_moved() {
    let data =
        "[Address Book]\nBackendName=ldap\n\n[Authentication]\nHost=ldap.example.org\nPort=636\n\n\
                [LDAP Backend]\nRootDn=dc=example,dc=org\nSecurityMethod=ldaps\n";
    let d = Directory::from_source("abc", "/p", data).unwrap();
    assert_eq!(d.security, Security::Ldaps);
    let out = d.to_source();
    assert!(out.contains("[Security]\nMethod=ldaps"), "{out}");
    assert!(!out.contains("SecurityMethod"), "{out}");
}

#[test]
fn a_new_source_is_filed_where_evolution_files_them() {
    let mut d = Directory::empty();
    d.host = "ldap.example.org".into();
    d.base_dn = "dc=example,dc=org".into();
    let out = d.to_source();
    assert!(out.contains("Parent=ldap-stub"));
    assert!(out.contains("[Security]\nMethod=starttls"), "{out}");
    assert!(!out.contains("SecurityMethod"), "{out}");
    assert!(out.contains("DisplayName=ldap.example.org"));
}

/// The key files written for EDS, pinned: a new directory, and one
/// rewritten over what EDS had.
#[test]
fn the_source_key_files_as_written() {
    use crate::config::golden::assert_golden;
    let mut d = Directory::empty();
    d.host = "ldap.example.org".into();
    d.base_dn = "dc=example,dc=org".into();
    assert_golden("contacts/directory/testdata/source-new.ini", &d.to_source());
    let data = "[Data Source]\nDisplayName=Work\nEnabled=true\nParent=ldap-stub\n\n\
                [Address Book]\nBackendName=ldap\n\n\
                [Authentication]\nHost=ldap.example.org\nPort=636\nUser=cn=me,dc=example,dc=org\nMethod=ldap/simple-binddn\n\n\
                [LDAP Backend]\nRootDn=dc=example,dc=org\nScope=onelevel\nFilter=(ou=staff)\n\n\
                [Security]\nMethod=ldaps\n";
    let mut d = Directory::from_source("abc", "/p", data).unwrap();
    d.subtree = true;
    d.enabled = false;
    assert_golden(
        "contacts/directory/testdata/source-rewritten.ini",
        &d.to_source(),
    );
}
