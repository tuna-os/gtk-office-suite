//! The reader pipeline of spec 4.2 and section 6: take what a store lists,
//! drop what the rules reject, verify the files, and say what the document
//! looks like. Every binding's journeys go through this, so a store cannot
//! hide a corrupt file by vouching for it itself.

use std::future::Future;

use crate::graph::{self, Options, ParentError};
use crate::integrity::{self, IntegrityError};
use crate::schema::{self, Kind};
use crate::{DocId, DocumentStore, Stamped, StoreError, Version};

/// Why a version is not part of the view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    /// The version object does not match its schema (spec 4).
    Schema(String),
    /// Version rule 6.
    Parents(ParentError),
    /// Version rule 2: the fetched file does not match `size` or `sha256`.
    Integrity(IntegrityError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejected {
    pub id: String,
    pub reason: Reason,
}

/// A head together with its verified file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Head {
    pub version: Stamped<Version>,
    pub bytes: Vec<u8>,
}

/// What opening the document shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View {
    /// Newest first. Empty: the document has no content (rule 7). More than
    /// one: a conflict to show, never to resolve silently (rule 5).
    pub heads: Vec<Head>,
    /// Every version the reader refused, and why, to be reported.
    pub rejected: Vec<Rejected>,
}

impl View {
    pub fn is_conflict(&self) -> bool {
        self.heads.len() > 1
    }

    pub fn head_ids(&self) -> Vec<&str> {
        self.heads.iter().map(|h| h.version.id.as_str()).collect()
    }
}

/// Compute the view from a version list and a way to fetch files. A head
/// whose file fails verification is discarded (and reported), after which the
/// heads are recomputed over what remains, so a good parent surfaces.
pub async fn build<F, Fut>(
    versions: Vec<Stamped<Version>>,
    options: Options,
    mut fetch: F,
) -> Result<View, StoreError>
where
    F: FnMut(Stamped<Version>) -> Fut,
    Fut: Future<Output = Result<Vec<u8>, StoreError>>,
{
    let mut rejected = Vec::new();

    let (mut live, schema_bad) = {
        let mut live = Vec::new();
        let mut bad = Vec::new();
        for v in versions {
            match schema::validate_typed(Kind::Version, &v.body) {
                Ok(()) => live.push(v),
                Err(e) => bad.push(Rejected {
                    id: v.id,
                    reason: Reason::Schema(e.to_string()),
                }),
            }
        }
        (live, bad)
    };
    rejected.extend(schema_bad);

    let (accepted, parent_bad) = graph::split(&live, options);
    rejected.extend(parent_bad.into_iter().map(|r| Rejected {
        id: r.id,
        reason: Reason::Parents(r.error),
    }));
    live = accepted;

    let mut verified: Vec<Head> = Vec::new();
    loop {
        let heads: Vec<Stamped<Version>> = graph::heads(&live).into_iter().cloned().collect();
        let mut dropped_any = false;
        for head in heads {
            if verified.iter().any(|h| h.version.id == head.id) {
                continue;
            }
            let bytes = fetch(head.clone()).await?;
            match integrity::verify_version(&head, &bytes) {
                Ok(()) => verified.push(Head {
                    version: head,
                    bytes,
                }),
                Err(d) => {
                    live.retain(|v| v.id != d.id);
                    rejected.push(Rejected {
                        id: d.id,
                        reason: Reason::Integrity(d.cause),
                    });
                    dropped_any = true;
                }
            }
        }
        if !dropped_any {
            break;
        }
    }
    verified.sort_by(|a, b| graph::newest_first(&a.version, &b.version));
    Ok(View {
        heads: verified,
        rejected,
    })
}

/// The view of `doc` as `store` shows it.
pub async fn read_view(store: &dyn DocumentStore, doc: &DocId) -> Result<View, StoreError> {
    let versions = store.versions(doc).await?;
    build(versions, Options::default(), |v| async move {
        store.fetch_file(doc, &v).await
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{ManualClock, MemoryBackend};
    use crate::{Level, Meta, NewFile, Role};
    use std::sync::Arc;

    fn meta() -> Meta {
        Meta {
            dcp: 1,
            title: "d".into(),
            mime: "text/plain".into(),
            levels: vec![Level::L1],
            algorithms: None,
            editor_services: None,
        }
    }

    fn file(s: &str) -> NewFile {
        NewFile {
            mime: "text/plain".into(),
            bytes: s.as_bytes().to_vec(),
        }
    }

    #[tokio::test]
    async fn one_head_one_file_verified() {
        let backend = MemoryBackend::new(Arc::new(ManualClock::new(1)));
        let o = backend.store("@o:m", "D");
        let doc = o.create_document(&meta(), Some(file("v1"))).await.unwrap();
        let view = read_view(&o, &doc).await.unwrap();
        assert_eq!(view.heads.len(), 1);
        assert_eq!(view.heads[0].bytes, b"v1");
        assert!(view.rejected.is_empty());
        assert!(!view.is_conflict());
    }

    #[tokio::test]
    async fn no_versions_means_no_content() {
        let backend = MemoryBackend::new(Arc::new(ManualClock::new(1)));
        let o = backend.store("@o:m", "D");
        let doc = o.create_document(&meta(), None).await.unwrap();
        let view = read_view(&o, &doc).await.unwrap();
        assert!(view.heads.is_empty() && view.rejected.is_empty());
    }

    #[tokio::test]
    async fn two_children_of_one_parent_are_a_conflict_showing_both() {
        let backend = MemoryBackend::new(Arc::new(ManualClock::new(1)));
        let o = backend.store("@o:m", "D");
        let e = backend.store("@e:m", "E");
        let doc = o.create_document(&meta(), Some(file("v1"))).await.unwrap();
        o.grant(&doc, "@e:m", Role::Editor).await.unwrap();
        let root = o.versions(&doc).await.unwrap()[0].id.clone();
        o.publish_version(&doc, vec![root.clone()], file("by-o"), None)
            .await
            .unwrap();
        e.publish_version(&doc, vec![root], file("by-e"), None)
            .await
            .unwrap();
        let view = read_view(&o, &doc).await.unwrap();
        assert!(view.is_conflict());
        let mut contents: Vec<_> = view.heads.iter().map(|h| h.bytes.clone()).collect();
        contents.sort();
        assert_eq!(contents, [b"by-e".to_vec(), b"by-o".to_vec()]);
    }

    #[tokio::test]
    async fn a_corrupt_head_is_discarded_reported_and_its_parent_shows() {
        let backend = MemoryBackend::new(Arc::new(ManualClock::new(1)));
        let o = backend.store("@o:m", "D");
        let doc = o.create_document(&meta(), Some(file("v1"))).await.unwrap();
        let root = o.versions(&doc).await.unwrap()[0].id.clone();
        let v2 = o
            .publish_version(&doc, vec![root.clone()], file("v2 content"), None)
            .await
            .unwrap();
        let versions = o.versions(&doc).await.unwrap();
        let view = build(versions, Options::default(), |v| {
            let o = o.clone();
            let doc = doc.clone();
            async move {
                let mut bytes = o.fetch_file(&doc, &v).await?;
                if v.id == "$ev000002:mem" {
                    bytes.truncate(3);
                }
                Ok(bytes)
            }
        })
        .await
        .unwrap();
        assert_eq!(view.head_ids(), [root.as_str()]);
        assert_eq!(view.rejected.len(), 1);
        assert_eq!(view.rejected[0].id, v2.id);
        assert!(matches!(
            view.rejected[0].reason,
            Reason::Integrity(IntegrityError::SizeMismatch { .. })
        ));
    }

    #[tokio::test]
    async fn a_same_size_bit_flip_is_a_hash_mismatch() {
        let backend = MemoryBackend::new(Arc::new(ManualClock::new(1)));
        let o = backend.store("@o:m", "D");
        let doc = o.create_document(&meta(), Some(file("v1"))).await.unwrap();
        let versions = o.versions(&doc).await.unwrap();
        let view = build(versions, Options::default(), |_| async {
            Ok(b"v2".to_vec())
        })
        .await
        .unwrap();
        assert!(
            view.heads.is_empty(),
            "the only version was corrupt: no content, and it is reported"
        );
        assert!(matches!(
            view.rejected[0].reason,
            Reason::Integrity(IntegrityError::HashMismatch { .. })
        ));
    }

    #[tokio::test]
    async fn rejected_by_schema_or_parents_never_reaches_fetch() {
        let backend = MemoryBackend::new(Arc::new(ManualClock::new(1)));
        let o = backend.store("@o:m", "D");
        let doc = o.create_document(&meta(), Some(file("v1"))).await.unwrap();
        let good = o.versions(&doc).await.unwrap().remove(0);
        let mut bad_schema = good.clone();
        bad_schema.id = "$bad1".into();
        bad_schema.body.file.sha256 = "xyz".into();
        let mut bad_parent = good.clone();
        bad_parent.id = "$bad2".into();
        bad_parent.body.parents = vec!["$bad2".into()];
        let mut unknown = good.clone();
        unknown.id = "$bad3".into();
        unknown.body.parents = vec!["$ghost".into()];
        let fetched = parking_lot::Mutex::new(Vec::new());
        let view = build(
            vec![good.clone(), bad_schema, bad_parent, unknown],
            Options::default(),
            |v| {
                fetched.lock().push(v.id.clone());
                let o = o.clone();
                let doc = doc.clone();
                async move { o.fetch_file(&doc, &v).await }
            },
        )
        .await
        .unwrap();
        assert_eq!(fetched.into_inner(), [good.id.clone()]);
        assert_eq!(view.head_ids(), [good.id.as_str()]);
        let reasons: Vec<_> = view
            .rejected
            .iter()
            .map(|r| (r.id.as_str(), &r.reason))
            .collect();
        assert!(reasons
            .iter()
            .any(|(id, r)| *id == "$bad1" && matches!(r, Reason::Schema(_))));
        assert!(
            reasons
                .iter()
                .any(|(id, r)| *id == "$bad2"
                    && matches!(r, Reason::Parents(ParentError::SelfParent)))
        );
        assert!(reasons
            .iter()
            .any(|(id, r)| *id == "$bad3"
                && matches!(r, Reason::Parents(ParentError::UnknownParent(_)))));
    }
}
