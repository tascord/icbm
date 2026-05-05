//! General-purpose utilities used across the workspace.

use std::collections::HashMap;

pub use uuid::Uuid;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum UtilsError {
    #[error("Parse error: {0}")]
    Parse(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Invalid argument: {0}")]
    InvalidArgument(String),
}

pub type Result<T, E = UtilsError> = std::result::Result<T, E>;

// ---------------------------------------------------------------------------
// String utilities
// ---------------------------------------------------------------------------

/// Converts a `snake_case` string to `CamelCase`.
///
/// ```
/// assert_eq!(utils::to_camel_case("hello_world"), "HelloWorld");
/// assert_eq!(utils::to_camel_case("foo"), "Foo");
/// ```
pub fn to_camel_case(s: &str) -> String {
    s.split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(c) => c.to_uppercase().to_string() + chars.as_str(),
            }
        })
        .collect()
}

/// Converts a `CamelCase` string to `snake_case`.
///
/// ```
/// assert_eq!(utils::to_snake_case("HelloWorld"), "hello_world");
/// ```
pub fn to_snake_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for (i, c) in s.char_indices() {
        if c.is_uppercase() && i != 0 {
            out.push('_');
        }
        out.extend(c.to_lowercase());
    }
    out
}

/// Truncates `s` to at most `max` characters, appending `…` if truncated.
pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let end = s.char_indices().nth(max.saturating_sub(1)).map(|(i, _)| i).unwrap_or(s.len());
    format!("{}…", &s[..end])
}

// ---------------------------------------------------------------------------
// Numeric utilities
// ---------------------------------------------------------------------------

/// Clamps `value` to `[min, max]`.
pub fn clamp<T: PartialOrd>(value: T, min: T, max: T) -> T {
    if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    }
}

/// Computes the arithmetic mean of a slice of `f64` values.
///
/// Returns `None` for an empty slice.
pub fn mean(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    Some(values.iter().sum::<f64>() / values.len() as f64)
}

/// Computes the population standard deviation.
pub fn std_dev(values: &[f64]) -> Option<f64> {
    let m = mean(values)?;
    let variance = values.iter().map(|v| (v - m).powi(2)).sum::<f64>() / values.len() as f64;
    Some(variance.sqrt())
}

// ---------------------------------------------------------------------------
// Generic registry
// ---------------------------------------------------------------------------

/// A simple string-keyed registry that can hold any `Clone + Send + Sync` value.
#[derive(Default, Clone)]
pub struct Registry<V: Clone + Send + Sync> {
    inner: HashMap<String, V>,
}

impl<V: Clone + Send + Sync> Registry<V> {
    pub fn new() -> Self { Self { inner: HashMap::new() } }

    pub fn insert(&mut self, key: impl Into<String>, value: V) { self.inner.insert(key.into(), value); }

    pub fn get(&self, key: &str) -> Option<&V> { self.inner.get(key) }

    pub fn remove(&mut self, key: &str) -> Option<V> { self.inner.remove(key) }

    pub fn len(&self) -> usize { self.inner.len() }

    pub fn is_empty(&self) -> bool { self.inner.is_empty() }

    pub fn keys(&self) -> impl Iterator<Item = &String> { self.inner.keys() }

    pub fn values(&self) -> impl Iterator<Item = &V> { self.inner.values() }
}

// ---------------------------------------------------------------------------
// Trait: Describable
// ---------------------------------------------------------------------------

/// Any type that can describe itself in a short human-readable string.
pub trait Describable {
    fn describe(&self) -> String;
}

// ---------------------------------------------------------------------------
// Unique ID newtype
// ---------------------------------------------------------------------------

/// A strongly-typed UUID wrapper.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Id(Uuid);

impl Id {
    pub fn new() -> Self { Id(Uuid::new_v4()) }
}

impl Default for Id {
    fn default() -> Self { Self::new() }
}

impl std::fmt::Display for Id {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "{}", self.0) }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_camel_snake_roundtrip() {
        let s = "hello_world_foo";
        let camel = to_camel_case(s);
        assert_eq!(camel, "HelloWorldFoo");
        let back = to_snake_case(&camel);
        assert_eq!(back, s);
    }

    #[test]
    fn test_truncate() {
        assert_eq!(truncate("hello world", 5), "hell…");
        assert_eq!(truncate("hi", 10), "hi");
    }

    #[test]
    fn test_mean_std_dev() {
        let data = vec![2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0];
        assert!((mean(&data).unwrap() - 5.0).abs() < 1e-10);
        assert!((std_dev(&data).unwrap() - 2.0).abs() < 1e-10);
    }

    #[test]
    fn test_registry() {
        let mut r: Registry<u32> = Registry::new();
        r.insert("a", 1);
        r.insert("b", 2);
        assert_eq!(r.get("a"), Some(&1));
        assert_eq!(r.len(), 2);
        r.remove("a");
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn test_clamp() {
        assert_eq!(clamp(5, 0, 10), 5);
        assert_eq!(clamp(-1, 0, 10), 0);
        assert_eq!(clamp(11, 0, 10), 10);
    }
}
