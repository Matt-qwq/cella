//! JSON fields of a workbook or sheet that this version of Cella does not model.
//!
//! The JSON workbook is the format we write that other Cella builds (and scripts) read: a
//! newer build may write a field this one has never heard of, and a build that dropped a field
//! leaves the old one behind in files written before. Serde normally discards what it does not
//! recognise, which would make a save from an older build silently destroy the newer build's data.
//!
//! A `#[serde(flatten)]` field of this type collects exactly those unmatched keys, and serialising
//! it writes them back where they came from. The values are kept as they were read (no merging, no
//! normalising), so a load + save is lossless for anything above the model.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

/// Unmodelled JSON fields of one object (a workbook, a sheet or the envelope around them).
///
/// Transparent over a JSON object: it serialises as its keys and deserialises every key it is
/// given, which is what makes it usable as a `#[serde(flatten)]` catch-all.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct JsonKept(Map<String, Value>);

impl JsonKept {
    /// No unknown fields: the object held only fields the model knows.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The fields, by name.
    pub fn fields(&self) -> &Map<String, Value> {
        &self.0
    }

    /// The value of field `key`, if the file had one we do not model.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.get(key)
    }

    /// Keeps `key` with `value`. An empty key (a malformed file) is ignored, and so is a value
    /// that cannot be held: keeping is never a reason to fail an open.
    pub fn keep(&mut self, key: impl Into<String>, value: Value) {
        let key = key.into();
        if !key.is_empty() {
            self.0.insert(key, value);
        }
    }

    /// Keeps every field of `object`, except the ones this build owns (`reserved`), which are
    /// written from the model instead.
    pub fn keep_object(&mut self, object: Map<String, Value>, reserved: &[&str]) {
        for (key, value) in object {
            if !reserved.iter().any(|r| *r == key) {
                self.keep(key, value);
            }
        }
    }
}

impl Serialize for JsonKept {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        // A map, not a newtype: this is what a flattened field can be written as.
        self.0.serialize(s)
    }
}

impl<'de> Deserialize<'de> for JsonKept {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<JsonKept, D::Error> {
        // Whatever keys the model's own fields did not take.
        Map::deserialize(d).map(JsonKept)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Known {
        name: String,
        #[serde(flatten)]
        kept: JsonKept,
    }

    #[test]
    fn unknown_keys_are_kept_and_written_back() {
        let text = r#"{"name":"a","futureFlag":42,"nested":{"x":[1,2]}}"#;
        let known: Known = serde_json::from_str(text).unwrap();
        assert_eq!(known.kept.get("futureFlag"), Some(&json!(42)));
        assert_eq!(known.kept.get("nested"), Some(&json!({"x": [1, 2]})));
        assert_eq!(serde_json::to_string(&known).unwrap(), text);
    }

    #[test]
    fn known_keys_never_reach_the_catch_all() {
        let known: Known = serde_json::from_str(r#"{"name":"a"}"#).unwrap();
        assert!(known.kept.is_empty());
        assert_eq!(known.kept.get("name"), None);
        assert_eq!(serde_json::to_string(&known).unwrap(), r#"{"name":"a"}"#);
    }

    #[test]
    fn reserved_keys_are_not_kept() {
        let mut kept = JsonKept::default();
        kept.keep_object([("workbook".to_string(), json!(1)), ("extra".to_string(), json!(2))].into_iter().collect(), &["workbook"]);
        assert_eq!(kept.fields().len(), 1);
        assert_eq!(kept.get("extra"), Some(&json!(2)));
    }
}
