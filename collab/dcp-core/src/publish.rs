//! The writer's side of spec 4.2 version rules 2 and 3 in one call.

use crate::graph;
use crate::integrity::{self, PackageError};
use crate::{Covers, DocId, DocumentStore, NewFile, Stamped, StoreError, Version};

/// Publish `file` only if the writer could open it (rule 3: for ODF and OOXML
/// a ZIP package that parses). The upload-verify-publish sequence of rule 2 is
/// the store's (see [`DocumentStore::publish_version`]); a reader still
/// verifies on its side.
pub async fn publish_checked(
    store: &dyn DocumentStore,
    doc: &DocId,
    parents: Vec<String>,
    file: NewFile,
    message: Option<String>,
) -> Result<Stamped<Version>, StoreError> {
    check_openable(&file)?;
    store.publish_version(doc, parents, file, message).await
}

fn check_openable(file: &NewFile) -> Result<(), StoreError> {
    integrity::check_package(&file.mime, &file.bytes).map_err(|e: PackageError| {
        StoreError::Invalid(format!(
            "refusing to publish a file that does not open: {e}"
        ))
    })
}

/// [`publish_checked`] for a version written from a live replica: the version
/// carries `covers` ([`DocumentStore::publish_version_through`]).
pub async fn publish_checked_through(
    store: &dyn DocumentStore,
    doc: &DocId,
    parents: Vec<String>,
    file: NewFile,
    message: Option<String>,
    covers: Option<Covers>,
) -> Result<Stamped<Version>, StoreError> {
    check_openable(&file)?;
    store
        .publish_version_through(doc, parents, file, message, covers)
        .await
}

/// Rule 4 for a program that has the version list at hand: the parents for a
/// save started from `opened`. A save is a child of the version the editor
/// started from, whatever the head is now (if the head moved, that makes a
/// second head; it is not hidden by rebasing).
pub fn parents_for_save(opened: &str) -> Vec<String> {
    vec![opened.to_owned()]
}

/// The parents of a save when the document has more than one head and the
/// user chose to merge all of them (spec 6.3).
pub fn parents_for_merge(versions: &[Stamped<Version>]) -> Vec<String> {
    graph::merge_parents(&graph::heads(versions))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{ManualClock, MemoryBackend};
    use crate::{Level, Meta};
    use std::sync::Arc;

    const ODT: &str = "application/vnd.oasis.opendocument.text";

    fn odt(content: &[u8]) -> Vec<u8> {
        crate::integrity::tests::zip(&[("mimetype", ODT.as_bytes()), ("content.xml", content)])
    }

    async fn doc() -> (crate::memory::MemoryStore, DocId, String) {
        let backend = MemoryBackend::new(Arc::new(ManualClock::new(1)));
        let o = backend.store("@o:m", "D");
        let meta = Meta {
            dcp: 1,
            title: "t".into(),
            mime: ODT.into(),
            levels: vec![Level::L1],
            algorithms: None,
            editor_services: None,
        };
        let doc = o
            .create_document(
                &meta,
                Some(NewFile {
                    mime: ODT.into(),
                    bytes: odt(b"one"),
                }),
            )
            .await
            .unwrap();
        let first = o.versions(&doc).await.unwrap()[0].id.clone();
        (o, doc, first)
    }

    #[tokio::test]
    async fn a_file_that_does_not_open_is_never_handed_to_the_store() {
        let (o, doc, first) = doc().await;
        let mut cut = odt(b"two");
        cut.truncate(cut.len() - 5);
        let r = publish_checked(
            &o,
            &doc,
            parents_for_save(&first),
            NewFile {
                mime: ODT.into(),
                bytes: cut,
            },
            None,
        )
        .await;
        assert!(matches!(r, Err(StoreError::Invalid(_))), "{r:?}");
        assert_eq!(o.versions(&doc).await.unwrap().len(), 1);
        publish_checked(
            &o,
            &doc,
            parents_for_save(&first),
            NewFile {
                mime: ODT.into(),
                bytes: odt(b"two"),
            },
            None,
        )
        .await
        .unwrap();
        assert_eq!(o.versions(&doc).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn a_save_from_a_stale_version_makes_a_second_head_not_a_rebase() {
        // Failure table row 1: network lost, then reconnect: publish a version
        // based on the last known head. The head moved: two heads.
        let (o, doc, first) = doc().await;
        let file = |c: &[u8]| NewFile {
            mime: ODT.into(),
            bytes: odt(c),
        };
        publish_checked(
            &o,
            &doc,
            parents_for_save(&first),
            file(b"online edit"),
            None,
        )
        .await
        .unwrap();
        publish_checked(
            &o,
            &doc,
            parents_for_save(&first),
            file(b"offline edit"),
            None,
        )
        .await
        .unwrap();
        let versions = o.versions(&doc).await.unwrap();
        assert_eq!(graph::heads(&versions).len(), 2);
        // The user merges: every head is a parent, one head again.
        let merged = publish_checked(
            &o,
            &doc,
            parents_for_merge(&versions),
            file(b"merged"),
            None,
        )
        .await
        .unwrap();
        assert_eq!(merged.body.parents.len(), 2);
        assert_eq!(graph::heads(&o.versions(&doc).await.unwrap()).len(), 1);
    }

    #[tokio::test]
    async fn earlier_versions_are_never_changed_by_later_saves_rule_4() {
        let (o, doc, first) = doc().await;
        let before = o.versions(&doc).await.unwrap()[0].clone();
        let bytes_before = o.fetch_file(&doc, &before).await.unwrap();
        for i in 0..3 {
            let head = graph::heads(&o.versions(&doc).await.unwrap())[0].id.clone();
            let c = format!("edit {i}");
            publish_checked(
                &o,
                &doc,
                parents_for_save(&head),
                NewFile {
                    mime: ODT.into(),
                    bytes: odt(c.as_bytes()),
                },
                None,
            )
            .await
            .unwrap();
        }
        let all = o.versions(&doc).await.unwrap();
        assert_eq!(all.len(), 4);
        let still = all.iter().find(|v| v.id == first).unwrap();
        assert_eq!(still, &before, "the version record is immutable (rule 1)");
        assert_eq!(o.fetch_file(&doc, still).await.unwrap(), bytes_before);
    }

    #[tokio::test]
    async fn a_version_written_through_a_prefix_carries_that_claim_and_only_a_valid_one() {
        let (o, doc, first) = doc().await;
        let file = |c: &[u8]| NewFile {
            mime: ODT.into(),
            bytes: odt(c),
        };
        let claim = Covers {
            algorithm: "loro/1".into(),
            through: "$ev000007:mem".into(),
        };
        let v = publish_checked_through(
            &o,
            &doc,
            parents_for_save(&first),
            file(b"two"),
            None,
            Some(claim.clone()),
        )
        .await
        .unwrap();
        assert_eq!(v.body.covers, Some(claim.clone()));
        let listed = o.versions(&doc).await.unwrap();
        assert_eq!(listed.iter().find(|x| x.id == v.id).unwrap().body.covers, Some(claim));

        // No claim: an ordinary version.
        let plain = publish_checked_through(&o, &doc, vec![v.id.clone()], file(b"three"), None, None)
            .await
            .unwrap();
        assert_eq!(plain.body.covers, None);

        // A claim the schema refuses (spec 4.2: `algorithm` is `name/major`, `through`
        // is not empty) publishes nothing.
        for bad in [
            Covers { algorithm: "Loro".into(), through: "u".into() },
            Covers { algorithm: "loro/1".into(), through: String::new() },
        ] {
            let r = publish_checked_through(
                &o,
                &doc,
                vec![plain.id.clone()],
                file(b"four"),
                None,
                Some(bad),
            )
            .await;
            assert!(matches!(r, Err(StoreError::Invalid(_))), "{r:?}");
        }
        assert_eq!(o.versions(&doc).await.unwrap().len(), 3);

        // And the file must open, with or without a claim.
        let mut cut = odt(b"five");
        cut.truncate(cut.len() - 5);
        let r = publish_checked_through(
            &o,
            &doc,
            vec![plain.id.clone()],
            NewFile { mime: ODT.into(), bytes: cut },
            None,
            Some(Covers { algorithm: "loro/1".into(), through: "u".into() }),
        )
        .await;
        assert!(matches!(r, Err(StoreError::Invalid(_))), "{r:?}");
        assert_eq!(o.versions(&doc).await.unwrap().len(), 3);
    }
}
