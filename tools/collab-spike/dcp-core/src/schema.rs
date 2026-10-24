//! Validation of any DCP object against the draft-1 JSON schemas.
//!
//! The schemas are `include_str!`ed from `docs/collab-spec/schemas`, so that
//! directory stays the single source of truth: a change there changes what
//! this crate accepts, and `validate.py` and this module cannot drift apart.

use std::sync::LazyLock;

use jsonschema::{Draft, Resource, Validator};
use serde::Serialize;
use serde_json::Value;

const META: &str = include_str!("../../../../docs/collab-spec/schemas/meta.schema.json");
const VERSION: &str = include_str!("../../../../docs/collab-spec/schemas/version.schema.json");
const LEASE: &str = include_str!("../../../../docs/collab-spec/schemas/lease.schema.json");
const UPDATE: &str = include_str!("../../../../docs/collab-spec/schemas/update.schema.json");
const FILE_REF: &str = include_str!("../../../../docs/collab-spec/schemas/file-ref.schema.json");

/// The `$id` base the schemas use for their `$ref`s.
const BASE: &str = "https://tunaos.org/dcp/1/";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Meta,
    Version,
    Lease,
    Update,
    FileRef,
}

impl Kind {
    pub const ALL: [Kind; 5] = [
        Kind::Meta,
        Kind::Version,
        Kind::Lease,
        Kind::Update,
        Kind::FileRef,
    ];

    fn index(self) -> usize {
        self as usize
    }

    fn source(self) -> &'static str {
        match self {
            Kind::Meta => META,
            Kind::Version => VERSION,
            Kind::Lease => LEASE,
            Kind::Update => UPDATE,
            Kind::FileRef => FILE_REF,
        }
    }
}

/// Every error the schema found, as human-readable lines.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{kind:?} does not match its schema: {}", .errors.join("; "))]
pub struct SchemaError {
    pub kind: Kind,
    pub errors: Vec<String>,
}

static VALIDATORS: LazyLock<[Validator; 5]> = LazyLock::new(|| {
    let build = |kind: Kind| -> Validator {
        let mut options = jsonschema::options().with_draft(Draft::Draft202012);
        // `version` and `update` `$ref` the file reference by its relative
        // name, which resolves against `$id`, so it has to be registered
        // under that absolute URI (there is no network retriever).
        let file_ref: Value = serde_json::from_str(FILE_REF).expect("file-ref schema is JSON");
        options = options.with_resource(
            format!("{BASE}file-ref.schema.json"),
            Resource::from_contents(file_ref).expect("file-ref schema is a schema"),
        );
        let schema: Value = serde_json::from_str(kind.source()).expect("schema is JSON");
        options.build(&schema).expect("the draft-1 schemas compile")
    };
    Kind::ALL.map(build)
});

/// Validate `value` against the schema for `kind`.
pub fn validate(kind: Kind, value: &Value) -> Result<(), SchemaError> {
    let validator = &VALIDATORS[kind.index()];
    let errors: Vec<String> = validator
        .iter_errors(value)
        .map(|e| format!("{} (at {})", e, e.instance_path))
        .collect();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(SchemaError { kind, errors })
    }
}

/// Validate a typed DCP object by serializing it first. This catches what the
/// Rust types cannot express (lengths, patterns, conditional requirements).
pub fn validate_typed<T: Serialize>(kind: Kind, object: &T) -> Result<(), SchemaError> {
    let value = serde_json::to_value(object).map_err(|e| SchemaError {
        kind,
        errors: vec![format!("not serializable: {e}")],
    })?;
    validate(kind, &value)
}

/// Parse and validate raw event content (what a binding received) into `T`.
/// Content that is not valid JSON, fails the schema, or does not fit `T` is an
/// error, and a client MUST ignore such an event (spec 8.1).
pub fn parse<T: serde::de::DeserializeOwned>(kind: Kind, raw: &Value) -> Result<T, SchemaError> {
    validate(kind, raw)?;
    serde_json::from_value(raw.clone()).map_err(|e| SchemaError {
        kind,
        errors: vec![format!("does not fit the Rust type: {e}")],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Lease, Meta, Update, Version};
    use serde_json::json;

    const DIR: &str = "../../../../docs/collab-spec/examples";

    fn example(name: &str) -> Value {
        let text = match name {
            "meta.json" => include_str!("../../../../docs/collab-spec/examples/meta.json"),
            "version.json" => include_str!("../../../../docs/collab-spec/examples/version.json"),
            "lease-held.json" => {
                include_str!("../../../../docs/collab-spec/examples/lease-held.json")
            }
            "lease-released.json" => {
                include_str!("../../../../docs/collab-spec/examples/lease-released.json")
            }
            "lease-held-empty-document.json" => include_str!(
                "../../../../docs/collab-spec/examples/lease-held-empty-document.json"
            ),
            "lease-held-wopi.json" => {
                include_str!("../../../../docs/collab-spec/examples/lease-held-wopi.json")
            }
            "update.json" => include_str!("../../../../docs/collab-spec/examples/update.json"),
            other => panic!("unknown example {other} (examples live in {DIR})"),
        };
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn positive_examples_validate_and_roundtrip_through_the_rust_types() {
        validate(Kind::Meta, &example("meta.json")).unwrap();
        validate(Kind::Version, &example("version.json")).unwrap();
        validate(Kind::Lease, &example("lease-held.json")).unwrap();
        validate(Kind::Lease, &example("lease-released.json")).unwrap();
        validate(Kind::Lease, &example("lease-held-empty-document.json")).unwrap();
        validate(Kind::Lease, &example("lease-held-wopi.json")).unwrap();
        validate(Kind::Update, &example("update.json")).unwrap();

        // The typed model serializes back to something the schema accepts and
        // that is the same JSON (no field is silently dropped or renamed).
        let m: Meta = parse(Kind::Meta, &example("meta.json")).unwrap();
        assert_eq!(serde_json::to_value(&m).unwrap(), example("meta.json"));
        let v: Version = parse(Kind::Version, &example("version.json")).unwrap();
        assert_eq!(serde_json::to_value(&v).unwrap(), example("version.json"));
        let l: Lease = parse(Kind::Lease, &example("lease-held.json")).unwrap();
        assert_eq!(
            serde_json::to_value(&l).unwrap(),
            example("lease-held.json")
        );
        let r: Lease = parse(Kind::Lease, &example("lease-released.json")).unwrap();
        assert_eq!(
            serde_json::to_value(&r).unwrap(),
            example("lease-released.json")
        );
        for name in ["lease-held-empty-document.json", "lease-held-wopi.json"] {
            let l: Lease = parse(Kind::Lease, &example(name)).unwrap();
            assert_eq!(serde_json::to_value(&l).unwrap(), example(name), "{name}");
        }
        let u: Update = parse(Kind::Update, &example("update.json")).unwrap();
        assert_eq!(serde_json::to_value(&u).unwrap(), example("update.json"));
    }

    /// The sixteen malformed variants of `docs/collab-spec/validate.py`, in the
    /// same order and with the same labels (the draft-0 variants it generates
    /// from the examples are covered by `draft_0_objects_are_rejected`). Each
    /// must be rejected.
    fn negatives() -> Vec<(&'static str, Kind, Value)> {
        let mut out = Vec::new();
        let mut add = |label, kind, example_name: &str, change: &dyn Fn(&mut Value)| {
            let mut doc = example(example_name);
            change(&mut doc);
            out.push((label, kind, doc));
        };
        add("meta without L1", Kind::Meta, "meta.json", &|d| {
            d["levels"] = json!(["L0"])
        });
        add(
            "meta L2 without algorithms",
            Kind::Meta,
            "meta.json",
            &|d| {
                d.as_object_mut().unwrap().remove("algorithms");
            },
        );
        add(
            "version with bad hash",
            Kind::Version,
            "version.json",
            &|d| d["file"]["sha256"] = json!("xyz"),
        );
        add(
            "version without parents",
            Kind::Version,
            "version.json",
            &|d| {
                d.as_object_mut().unwrap().remove("parents");
            },
        );
        add(
            "version, plain-http webdav",
            Kind::Version,
            "version.json",
            &|d| {
                d["file"]["locator"] = json!({"type": "webdav", "href": "http://x/y"});
            },
        );
        add(
            "held lease without token",
            Kind::Lease,
            "lease-held.json",
            &|d| {
                d.as_object_mut().unwrap().remove("token");
            },
        );
        add(
            "held lease, 2 hour duration",
            Kind::Lease,
            "lease-held.json",
            &|d| d["duration_ms"] = json!(7_200_000),
        );
        add(
            "held lease without renewal",
            Kind::Lease,
            "lease-held.json",
            &|d| {
                d.as_object_mut().unwrap().remove("renewal");
            },
        );
        add(
            "held lease, negative renewal",
            Kind::Lease,
            "lease-held.json",
            &|d| d["renewal"] = json!(-1),
        );
        add(
            "held lease, fractional renewal",
            Kind::Lease,
            "lease-held.json",
            &|d| d["renewal"] = json!(1.5),
        );
        add(
            "held lease, string renewal",
            Kind::Lease,
            "lease-held.json",
            &|d| d["renewal"] = json!("2"),
        );
        add(
            "held lease, empty base_version (use omission, not a sentinel)",
            Kind::Lease,
            "lease-held.json",
            &|d| d["base_version"] = json!(""),
        );
        add(
            "held lease, unknown field",
            Kind::Lease,
            "lease-held.json",
            &|d| d["epoch"] = json!(1),
        );
        add(
            "held lease, 257-character token",
            Kind::Lease,
            "lease-held.json",
            &|d| d["token"] = json!("t".repeat(257)),
        );
        add(
            "update over 34000 bytes",
            Kind::Update,
            "update.json",
            &|d| d["payload"]["size"] = json!(34001),
        );
        add(
            "update with bad algorithm id",
            Kind::Update,
            "update.json",
            &|d| d["algorithm"] = json!("Loro"),
        );
        out
    }

    #[test]
    fn the_sixteen_validate_py_negatives_are_rejected() {
        let cases = negatives();
        assert_eq!(cases.len(), 16);
        for (label, kind, doc) in &cases {
            assert!(validate(*kind, doc).is_err(), "accepted: {label}");
        }
    }

    #[test]
    fn negatives_stay_in_step_with_validate_py() {
        // If someone adds a negative case to validate.py, this fails until the
        // Rust list above gets the same case.
        let py = include_str!("../../../../docs/collab-spec/validate.py");
        let in_py = py.matches("\n    (\"").count();
        assert_eq!(
            in_py,
            negatives().len(),
            "validate.py negatives changed; update negatives() here"
        );
        for (label, _, _) in negatives() {
            assert!(
                py.contains(&format!("\"{label}\"")),
                "validate.py lacks the case {label:?}"
            );
        }
    }

    #[test]
    fn unknown_fields_are_rejected_on_every_object() {
        for (name, kind) in [
            ("meta.json", Kind::Meta),
            ("version.json", Kind::Version),
            ("lease-held.json", Kind::Lease),
            ("update.json", Kind::Update),
        ] {
            let mut doc = example(name);
            doc["surprise"] = json!(1);
            assert!(
                validate(kind, &doc).is_err(),
                "{name} accepted an unknown field"
            );
        }
        // And nested ones.
        let mut v = example("version.json");
        v["created_by"]["extra"] = json!(true);
        assert!(validate(Kind::Version, &v).is_err());
        let mut v = example("version.json");
        v["file"]["extra"] = json!(true);
        assert!(
            validate(Kind::Version, &v).is_err(),
            "the file-ref $ref must be enforced"
        );
    }

    #[test]
    fn dcp_version_must_be_one() {
        for v in [0, 2] {
            let mut d = example("version.json");
            d["dcp"] = json!(v);
            assert!(validate(Kind::Version, &d).is_err(), "dcp {v} accepted");
        }
    }

    /// `validate.py` appends a `"dcp": 0` variant of every positive example.
    #[test]
    fn draft_0_objects_are_rejected() {
        for (name, kind) in [
            ("meta.json", Kind::Meta),
            ("version.json", Kind::Version),
            ("lease-held.json", Kind::Lease),
            ("lease-released.json", Kind::Lease),
            ("lease-held-empty-document.json", Kind::Lease),
            ("lease-held-wopi.json", Kind::Lease),
            ("update.json", Kind::Update),
        ] {
            let mut d = example(name);
            d["dcp"] = json!(0);
            assert!(validate(kind, &d).is_err(), "{name} with dcp 0 accepted");
        }
    }

    #[test]
    fn more_than_eight_parents_and_duplicates_are_rejected() {
        let mut v = example("version.json");
        v["parents"] = json!(["1", "2", "3", "4", "5", "6", "7", "8", "9"]);
        assert!(validate(Kind::Version, &v).is_err());
        v["parents"] = json!(["a", "a"]);
        assert!(validate(Kind::Version, &v).is_err());
    }

    #[test]
    fn file_payload_validates_the_nested_file_ref() {
        let mut u = example("update.json");
        u["payload"] = json!({"encoding": "file", "file": example("version.json")["file"].clone()});
        validate(Kind::Update, &u).unwrap();
        u["payload"]["file"]["sha256"] = json!("nope");
        assert!(validate(Kind::Update, &u).is_err());
    }

    #[test]
    fn typed_validation_catches_what_serde_alone_accepts() {
        let mut v: Version = parse(Kind::Version, &example("version.json")).unwrap();
        validate_typed(Kind::Version, &v).unwrap();
        v.file.sha256 = "xyz".into();
        assert!(validate_typed(Kind::Version, &v).is_err());
    }

    #[test]
    fn parse_rejects_content_that_is_not_an_object_of_the_kind() {
        assert!(parse::<Lease>(Kind::Lease, &json!("held")).is_err());
        assert!(parse::<Lease>(Kind::Lease, &json!({})).is_err());
    }
}
