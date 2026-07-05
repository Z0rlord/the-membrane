//! Deterministic JSON serialization for MembraneEvent signing and Merkle leaves.

use serde::Serialize;
use serde_json::{Map, Value};

#[derive(Debug, thiserror::Error)]
pub enum CanonicalError {
    #[error("failed to serialize canonical JSON: {0}")]
    Serde(#[from] serde_json::Error),
}

/// Produce canonical JSON bytes (sorted object keys, no insignificant whitespace).
pub fn canonical_json_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>, CanonicalError> {
  let value = serde_json::to_value(value)?;
  let canonical = canonicalize_value(&value);
  Ok(serde_json::to_vec(&canonical)?)
}

fn canonicalize_value(value: &Value) -> Value {
  match value {
    Value::Object(map) => {
      let mut keys: Vec<&String> = map.keys().collect();
      keys.sort();
      let mut out = Map::new();
      for key in keys {
        out.insert(key.clone(), canonicalize_value(&map[key]));
      }
      Value::Object(out)
    }
    Value::Array(items) => Value::Array(items.iter().map(canonicalize_value).collect()),
    other => other.clone(),
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use serde_json::json;

  #[test]
  fn sorts_object_keys() {
    let input = json!({"b": 2, "a": 1});
    let bytes = canonical_json_bytes(&input).unwrap();
    assert_eq!(bytes, br#"{"a":1,"b":2}"#);
  }
}
