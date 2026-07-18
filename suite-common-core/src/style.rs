// style.rs — Named styles with parent-chain inheritance.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Pattern: LibreOffice svl/style.hxx, svl/stylepool.hxx.
// Styles provide named formatting presets with hierarchical
// inheritance. Used by Tables (cell styles), Letters (paragraph
// styles), and Decks (text styles).

use crate::props::{PropertyId, PropertySet, PropertyValue};
use std::collections::HashMap;

/// A named style with optional parent inheritance.
#[derive(Clone, Debug)]
pub struct Style {
    /// Display name (e.g., "Heading 1", "Currency", "Normal").
    pub name: String,
    /// Parent style name — properties not set locally fall back to parent.
    pub parent: Option<String>,
    /// Local properties (override parent, fill in gaps).
    pub props: PropertySet,
}

impl Style {
    pub fn new(name: &str) -> Self {
        Style { name: name.into(), parent: None, props: PropertySet::new() }
    }

    pub fn with_parent(name: &str, parent: &str) -> Self {
        Style { name: name.into(), parent: Some(parent.into()), props: PropertySet::new() }
    }
}

/// A pool of named styles with lookup and resolution.
#[derive(Clone, Debug, Default)]
pub struct StylePool {
    styles: HashMap<String, Style>,
}

impl StylePool {
    pub fn new() -> Self { StylePool { styles: HashMap::new() } }

    /// Register a style in the pool.
    pub fn add(&mut self, style: Style) {
        self.styles.insert(style.name.clone(), style);
    }

    /// Get a style by name.
    pub fn get(&self, name: &str) -> Option<&Style> {
        self.styles.get(name)
    }

    /// Resolve a property value by walking the parent chain.
    /// Returns the first non-None value found, starting from the named style
    /// and walking up through parents.
    pub fn resolve(&self, style_name: &str, prop: PropertyId) -> Option<&PropertyValue> {
        let mut current = self.styles.get(style_name)?;
        loop {
            if let Some(val) = current.props.get(prop) {
                return Some(val);
            }
            let parent_name = current.parent.as_ref()?;
            current = self.styles.get(parent_name)?;
        }
    }

    /// Check if a style has a given property set locally (not inherited).
    pub fn has_local(&self, style_name: &str, prop: PropertyId) -> bool {
        self.styles.get(style_name)
            .map(|s| s.props.get(prop).is_some())
            .unwrap_or(false)
    }

    /// Number of styles in the pool.
    pub fn len(&self) -> usize { self.styles.len() }
    pub fn is_empty(&self) -> bool { self.styles.is_empty() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_style_local_property() {
        let mut s = Style::new("Test");
        s.props.set(PropertyId::Bold, PropertyValue::Bool(true));
        assert_eq!(s.props.get(PropertyId::Bold).unwrap().as_bool(), Some(true));
    }

    #[test]
    fn test_pool_lookup() {
        let mut pool = StylePool::new();
        pool.add(Style::new("Normal"));
        assert!(pool.get("Normal").is_some());
        assert!(pool.get("Missing").is_none());
    }

    #[test]
    fn test_parent_inheritance() {
        let mut pool = StylePool::new();

        let mut parent = Style::new("Normal");
        parent.props.set(PropertyId::FontSize, PropertyValue::Float(12.0));
        parent.props.set(PropertyId::Bold, PropertyValue::Bool(false));
        pool.add(parent);

        let mut child = Style::with_parent("Heading1", "Normal");
        child.props.set(PropertyId::Bold, PropertyValue::Bool(true));
        child.props.set(PropertyId::FontFamily, PropertyValue::String("Sans".into()));
        pool.add(child);

        // Local override
        assert_eq!(pool.resolve("Heading1", PropertyId::Bold).unwrap().as_bool(), Some(true));
        // Inherited from parent
        assert_eq!(pool.resolve("Heading1", PropertyId::FontSize).unwrap().as_float(), Some(12.0));
        // Local only
        assert_eq!(pool.resolve("Heading1", PropertyId::FontFamily).unwrap().as_string(), Some("Sans"));
        // Missing entirely
        assert!(pool.resolve("Heading1", PropertyId::Italic).is_none());
    }

    #[test]
    fn test_has_local() {
        let mut pool = StylePool::new();
        let mut parent = Style::new("Normal");
        parent.props.set(PropertyId::FontSize, PropertyValue::Float(12.0));
        pool.add(parent);

        let mut child = Style::with_parent("Heading1", "Normal");
        child.props.set(PropertyId::Bold, PropertyValue::Bool(true));
        pool.add(child);

        assert!(pool.has_local("Heading1", PropertyId::Bold));
        assert!(!pool.has_local("Heading1", PropertyId::FontSize)); // inherited
    }

    #[test]
    fn test_deep_chain() {
        let mut pool = StylePool::new();
        let mut a = Style::new("A");
        a.props.set(PropertyId::FontSize, PropertyValue::Float(10.0));
        pool.add(a);

        let mut b = Style::with_parent("B", "A");
        b.props.set(PropertyId::Italic, PropertyValue::Bool(true));
        pool.add(b);

        let mut c = Style::with_parent("C", "B");
        c.props.set(PropertyId::Bold, PropertyValue::Bool(true));
        pool.add(c);

        assert_eq!(pool.resolve("C", PropertyId::Bold).unwrap().as_bool(), Some(true));
        assert_eq!(pool.resolve("C", PropertyId::Italic).unwrap().as_bool(), Some(true));
        assert_eq!(pool.resolve("C", PropertyId::FontSize).unwrap().as_float(), Some(10.0));
    }
}
