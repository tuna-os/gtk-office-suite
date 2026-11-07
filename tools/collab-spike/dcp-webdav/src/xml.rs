//! A namespace-aware reader for WebDAV `multistatus` bodies. Nextcloud uses the
//! prefixes `d:`, `oc:`, `nc:` and `s:`, but a client must not depend on that,
//! so properties are keyed by `(namespace URI, local name)`.

use std::collections::HashMap;

use quick_xml::escape::unescape;
use quick_xml::events::Event;
use quick_xml::name::ResolveResult;
use quick_xml::NsReader;

pub const DAV: &str = "DAV:";
pub const OC: &str = "http://owncloud.org/ns";
pub const NC: &str = "http://nextcloud.org/ns";

/// One `response` element: a resource and the properties the server returned
/// for it with status 200.
#[derive(Debug, Default, Clone)]
pub struct Entry {
    /// The `href` exactly as sent (percent-encoded, absolute path).
    pub href: String,
    /// `(namespace, local name) -> text`. For a property with child elements the
    /// text is the raw inner XML.
    pub props: HashMap<(String, String), String>,
    /// Properties the server answered with another status, such as 404.
    pub missing: Vec<(String, String)>,
}

impl Entry {
    pub fn get(&self, ns: &str, local: &str) -> Option<&str> {
        self.props
            .get(&(ns.to_owned(), local.to_owned()))
            .map(String::as_str)
    }

    /// The last path segment of `href`, percent-decoded, without a trailing slash.
    pub fn name(&self) -> String {
        let seg = self
            .href
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or("");
        percent_decode(seg)
    }

    /// A collection: its `resourcetype` says so, or (when that was not asked
    /// for) its `href` ends with a slash, which Nextcloud does for collections.
    pub fn is_collection(&self) -> bool {
        self.href.ends_with('/')
            || self
                .get(DAV, "resourcetype")
                .is_some_and(|t| t.contains("collection"))
    }
}

pub fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("zz");
            if let Ok(v) = u8::from_str_radix(hex, 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn text_of(raw: &str) -> String {
    match unescape(raw) {
        Ok(t) => t.trim().to_owned(),
        Err(_) => raw.trim().to_owned(),
    }
}

fn status_code(line: &str) -> u16 {
    line.split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0)
}

/// Parse a `multistatus` document.
pub fn parse_multistatus(body: &[u8]) -> Result<Vec<Entry>, String> {
    let mut reader = NsReader::from_reader(body);
    let mut out = Vec::new();
    let mut cur: Option<Entry> = None;
    let mut in_propstat = false;
    let mut in_prop = false;
    let mut pending: Vec<((String, String), String)> = Vec::new();
    let mut pstatus: u16 = 0;
    loop {
        let (ns, ev) = reader
            .read_resolved_event()
            .map_err(|e| format!("xml: {e}"))?;
        let ns_str = match &ns {
            ResolveResult::Bound(n) => String::from_utf8_lossy(n.as_ref()).into_owned(),
            _ => String::new(),
        };
        match ev {
            Event::Start(e) => {
                let local = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                if in_prop {
                    let raw = reader
                        .read_text(e.name())
                        .map_err(|er| format!("xml: {er}"))?;
                    pending.push(((ns_str, local), text_of(&raw)));
                } else if ns_str == DAV {
                    match local.as_str() {
                        "response" => cur = Some(Entry::default()),
                        "href" if cur.is_some() && !in_propstat => {
                            let raw = reader
                                .read_text(e.name())
                                .map_err(|er| format!("xml: {er}"))?;
                            if let Some(c) = cur.as_mut() {
                                c.href = text_of(&raw);
                            }
                        }
                        "propstat" => {
                            in_propstat = true;
                            pending.clear();
                            pstatus = 0;
                        }
                        "prop" if in_propstat => in_prop = true,
                        "status" if in_propstat => {
                            let raw = reader
                                .read_text(e.name())
                                .map_err(|er| format!("xml: {er}"))?;
                            pstatus = status_code(&text_of(&raw));
                        }
                        _ => {}
                    }
                }
            }
            Event::Empty(e) => {
                if in_prop {
                    let local = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                    pending.push(((ns_str, local), String::new()));
                }
            }
            Event::End(e) => {
                let local = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                if ns_str == DAV {
                    match local.as_str() {
                        "prop" => in_prop = false,
                        "propstat" => {
                            in_propstat = false;
                            if let Some(c) = cur.as_mut() {
                                for (k, v) in pending.drain(..) {
                                    if (200..300).contains(&pstatus) {
                                        c.props.insert(k, v);
                                    } else {
                                        c.missing.push(k);
                                    }
                                }
                            }
                        }
                        "response" => {
                            if let Some(c) = cur.take() {
                                out.push(c);
                            }
                        }
                        _ => {}
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:" xmlns:s="http://sabredav.org/ns" xmlns:oc="http://owncloud.org/ns" xmlns:nc="http://nextcloud.org/ns">
 <d:response>
  <d:href>/remote.php/dav/files/alice/DCP-1/versions/a%20b.json</d:href>
  <d:propstat>
   <d:prop>
    <d:getetag>&quot;abc123&quot;</d:getetag>
    <d:getcontentlength>12</d:getcontentlength>
    <d:resourcetype/>
    <oc:fileid>77</oc:fileid>
    <nc:lock-owner-type/>
   </d:prop>
   <d:status>HTTP/1.1 200 OK</d:status>
  </d:propstat>
  <d:propstat>
   <d:prop><nc:share-permissions/></d:prop>
   <d:status>HTTP/1.1 404 Not Found</d:status>
  </d:propstat>
 </d:response>
 <d:response>
  <d:href>/remote.php/dav/files/alice/DCP-1/versions/</d:href>
  <d:propstat>
   <d:prop><d:resourcetype><d:collection/></d:resourcetype>
    <d:lockdiscovery><d:activelock><d:locktoken><d:href>opaquelocktoken:1</d:href></d:locktoken></d:activelock></d:lockdiscovery></d:prop>
   <d:status>HTTP/1.1 200 OK</d:status>
  </d:propstat>
 </d:response>
</d:multistatus>"#;

    #[test]
    fn reads_properties_by_namespace_not_prefix() {
        let r = parse_multistatus(SAMPLE.as_bytes()).unwrap();
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].name(), "a b.json");
        assert_eq!(r[0].get(DAV, "getetag"), Some("\"abc123\""));
        assert_eq!(r[0].get(OC, "fileid"), Some("77"));
        assert_eq!(r[0].get(DAV, "getcontentlength"), Some("12"));
        assert!(r[0].missing.contains(&(NC.to_owned(), "share-permissions".to_owned())));
        assert!(!r[0].is_collection());
        assert!(r[1].is_collection());
        assert!(r[1].get(DAV, "lockdiscovery").unwrap().contains("opaquelocktoken:1"));
    }
}
