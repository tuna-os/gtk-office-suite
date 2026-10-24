//! Cross-handle and cross-binding comparison of what two stores show.

use dcp_core::integrity::sha256_hex;
use dcp_core::view::{read_view, View};
use dcp_core::{DocId, DocumentStore};

use crate::{ensure, Failure};

fn describe(view: &View) -> String {
    format!(
        "heads {:?}, rejected {:?}",
        view.head_ids(),
        view.rejected
            .iter()
            .map(|r| r.id.as_str())
            .collect::<Vec<_>>()
    )
}

/// Two handles on ONE binding show the same document: the same heads (same
/// identifiers, same order) with the same verified bytes, and the same
/// versions rejected.
pub async fn assert_same_view(
    a: &dyn DocumentStore,
    b: &dyn DocumentStore,
    doc: &DocId,
) -> Result<(), Failure> {
    let va = read_view(a, doc).await?;
    let vb = read_view(b, doc).await?;
    same_view(
        &va,
        &vb,
        &format!("{} and {}", a.principal(), b.principal()),
    )
}

pub fn same_view(va: &View, vb: &View, who: &str) -> Result<(), Failure> {
    ensure!(
        va.head_ids() == vb.head_ids(),
        "{who} disagree on the heads: {} vs {}",
        describe(va),
        describe(vb)
    );
    for (ha, hb) in va.heads.iter().zip(&vb.heads) {
        ensure!(
            ha.bytes == hb.bytes,
            "{who} see different bytes for head {}",
            ha.version.id
        );
    }
    let rejected = |v: &View| {
        let mut ids: Vec<String> = v.rejected.iter().map(|r| r.id.clone()).collect();
        ids.sort();
        ids
    };
    ensure!(
        rejected(va) == rejected(vb),
        "{who} reject different versions: {} vs {}",
        describe(va),
        describe(vb)
    );
    Ok(())
}

/// Two bindings (identifiers differ, so they are not compared) show the same
/// content: the same number of heads and the same set of file hashes.
pub async fn assert_same_content(
    a: &dyn DocumentStore,
    doc_a: &DocId,
    b: &dyn DocumentStore,
    doc_b: &DocId,
) -> Result<(), Failure> {
    let va = read_view(a, doc_a).await?;
    let vb = read_view(b, doc_b).await?;
    let hashes = |v: &View| {
        let mut h: Vec<String> = v.heads.iter().map(|h| sha256_hex(&h.bytes)).collect();
        h.sort();
        h
    };
    ensure!(
        hashes(&va) == hashes(&vb),
        "the two bindings show different content: {} heads vs {} heads, {:?} vs {:?}",
        va.heads.len(),
        vb.heads.len(),
        hashes(&va),
        hashes(&vb)
    );
    Ok(())
}
