use serde_json::Value;

/// Compact JSON with object keys sorted by byte order at every level, independent of
/// serde_json's `preserve_order` feature. Strings and numbers use serde_json's encoding.
pub fn canonical_json(v: &Value) -> String {
    let mut out = String::new();
    write(v, &mut out);
    out
}

fn write(v: &Value, out: &mut String) {
    match v {
        Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            out.push('{');
            for (i, k) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&Value::String((*k).clone()).to_string());
                out.push(':');
                write(&m[k.as_str()], out);
            }
            out.push('}');
        }
        Value::Array(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write(x, out);
            }
            out.push(']');
        }
        scalar => out.push_str(&scalar.to_string()),
    }
}

/// Integers become decimal strings so JavaScript readers stay exact above 2^53.
pub fn stringify_integers(v: &Value) -> Value {
    match v {
        Value::Number(n) if n.is_u64() || n.is_i64() => Value::String(n.to_string()),
        Value::Object(m) => Value::Object(
            m.iter()
                .map(|(k, x)| (k.clone(), stringify_integers(x)))
                .collect(),
        ),
        Value::Array(a) => Value::Array(a.iter().map(stringify_integers).collect()),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn keys_sorted_at_every_level_and_compact() {
        let v = json!({"b": 1, "a": {"z": [3, {"y": null, "x": "q\""}], "c": true}});
        assert_eq!(
            canonical_json(&v),
            r#"{"a":{"c":true,"z":[3,{"x":"q\"","y":null}]},"b":1}"#
        );
    }

    #[test]
    fn u64_max_is_stringified_exactly() {
        assert_eq!(
            stringify_integers(&json!({"a": u64::MAX, "b": [-1], "c": 1.5})),
            json!({"a": "18446744073709551615", "b": ["-1"], "c": 1.5})
        );
    }
}
