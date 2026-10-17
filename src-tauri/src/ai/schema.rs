//! JSON-schema conversion for each provider's structured-output dialect, and [`clamp`], which
//! re-applies the original limits to a cloud reply.
use serde_json::{Map, Value};

use super::Provider;

const STRING_LENGTHS: [&str; 2] = ["minLength", "maxLength"];
const ANTHROPIC_DROPS: [&str; 8] =
    ["minLength", "maxLength", "minimum", "maximum", "exclusiveMinimum", "exclusiveMaximum", "multipleOf", "maxItems"];

/// Whether `p` rejects `key: value` in a schema node.
fn dropped(p: Provider, key: &str, value: &Value) -> bool {
    match p {
        Provider::Local | Provider::Ollama | Provider::LmStudio => false,
        Provider::OpenAi | Provider::Gemini => STRING_LENGTHS.contains(&key),
        Provider::Anthropic => {
            ANTHROPIC_DROPS.contains(&key) || (key == "minItems" && value.as_u64().is_none_or(|n| n > 1))
        }
    }
}

/// `schema` as `p` accepts it. Every object gets `additionalProperties: false` and
/// `required` = all its property keys. Removed keywords are appended to that node's
/// `description` as compact JSON in schema order (e.g. `{"maxLength":120}`), the way the Anthropic SDK does it.
pub fn for_provider(schema: &Value, p: Provider) -> Value {
    let Value::Object(node) = schema else { return schema.clone() };
    let mut out = Map::new();
    let mut removed = Map::new();
    for (k, v) in node {
        if dropped(p, k, v) {
            removed.insert(k.clone(), v.clone());
            continue;
        }
        let v = match k.as_str() {
            "properties" | "$defs" | "definitions" => match v {
                Value::Object(m) => Value::Object(m.iter().map(|(k, s)| (k.clone(), for_provider(s, p))).collect()),
                other => other.clone(),
            },
            "items" => for_provider(v, p),
            "anyOf" | "oneOf" | "allOf" => match v {
                Value::Array(a) => Value::Array(a.iter().map(|s| for_provider(s, p)).collect()),
                other => other.clone(),
            },
            _ => v.clone(),
        };
        out.insert(k.clone(), v);
    }
    if let Some(Value::Object(props)) = out.get("properties") {
        let required: Vec<Value> = props.keys().map(|k| Value::String(k.clone())).collect();
        out.insert("required".into(), Value::Array(required));
        out.insert("additionalProperties".into(), Value::Bool(false));
    } else if out.get("type").and_then(Value::as_str) == Some("object") {
        out.insert("additionalProperties".into(), Value::Bool(false));
    }
    if !removed.is_empty() {
        let limits = Value::Object(removed).to_string();
        let description = match out.get("description").and_then(Value::as_str) {
            Some(d) if !d.is_empty() => format!("{d}\n\n{limits}"),
            _ => limits,
        };
        out.insert("description".into(), Value::String(description));
    }
    Value::Object(out)
}

/// Truncates strings to `maxLength` chars and arrays to `maxItems`, clamps numbers into
/// [minimum, maximum], walking `v` against the original `schema`. Applied to cloud replies only.
pub fn clamp(v: &mut Value, schema: &Value) {
    match v {
        Value::String(s) => {
            if let Some(max) = schema.get("maxLength").and_then(Value::as_u64) {
                if let Some((cut, _)) = s.char_indices().nth(max as usize) {
                    s.truncate(cut);
                }
            }
        }
        Value::Number(n) => {
            let Some(x) = n.as_f64() else { return };
            let below = schema.get("minimum").filter(|m| m.as_f64().is_some_and(|m| x < m));
            let above = schema.get("maximum").filter(|m| m.as_f64().is_some_and(|m| x > m));
            if let Some(bound) = below.or(above) {
                *v = bound.clone();
            }
        }
        Value::Array(a) => {
            if let Some(max) = schema.get("maxItems").and_then(Value::as_u64) {
                a.truncate(max as usize);
            }
            if let Some(items) = schema.get("items") {
                a.iter_mut().for_each(|x| clamp(x, items));
            }
        }
        Value::Object(o) => {
            if let Some(Value::Object(props)) = schema.get("properties") {
                for (k, x) in o.iter_mut() {
                    if let Some(s) = props.get(k) {
                        clamp(x, s);
                    }
                }
            }
        }
        Value::Null | Value::Bool(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::db::{categories, Db};
    use crate::transcribe::extract;
    use crate::triage::ai;

    fn sample() -> Value {
        json!({
            "type": "object",
            "properties": {
                "title": { "type": "string", "maxLength": 120 },
                "kind": { "type": "string", "enum": ["a", "b"] },
                "tags": { "type": "array", "items": { "type": "string", "maxLength": 32 }, "minItems": 1, "maxItems": 5 },
                "steps": { "type": "array", "items": { "type": "string" }, "minItems": 2 },
                "estimateMin": { "type": "integer", "minimum": 5, "maximum": 240 },
                "note": { "type": "string", "description": "Short.", "minLength": 1 },
            },
        })
    }

    /// Every keyword `p` rejects, anywhere in `s`, plus objects missing `additionalProperties:false`.
    fn violations(s: &Value, p: Provider, at: &str, out: &mut Vec<String>) {
        match s {
            Value::Object(m) => {
                for (k, v) in m {
                    if dropped(p, k, v) {
                        out.push(format!("{at}.{k}"));
                    }
                }
                if m.get("type").and_then(Value::as_str) == Some("object") {
                    if m.get("additionalProperties") != Some(&json!(false)) {
                        out.push(format!("{at}: additionalProperties"));
                    }
                    let keys: Vec<Value> =
                        m["properties"].as_object().unwrap().keys().map(|k| json!(k)).collect();
                    if m.get("required") != Some(&Value::Array(keys)) {
                        out.push(format!("{at}: required"));
                    }
                }
                for (k, v) in m {
                    if !matches!(k.as_str(), "enum" | "required") {
                        violations(v, p, &format!("{at}.{k}"), out);
                    }
                }
            }
            Value::Array(a) => a.iter().for_each(|v| violations(v, p, at, out)),
            _ => {}
        }
    }

    #[test]
    fn openai_schema_drops_string_lengths() {
        let s = for_provider(&sample(), Provider::OpenAi);
        assert_eq!(s["properties"]["title"], json!({ "type": "string", "description": "{\"maxLength\":120}" }));
        assert_eq!(
            s["properties"]["tags"],
            json!({ "type": "array", "items": { "type": "string", "description": "{\"maxLength\":32}" }, "minItems": 1, "maxItems": 5 })
        );
        assert_eq!(s["properties"]["estimateMin"], json!({ "type": "integer", "minimum": 5, "maximum": 240 }));
        assert_eq!(s["properties"]["note"]["description"], "Short.\n\n{\"minLength\":1}");
        assert_eq!(s["additionalProperties"], json!(false));
        assert_eq!(s["required"], json!(["title", "kind", "tags", "steps", "estimateMin", "note"]));
    }

    #[test]
    fn anthropic_schema_drops_numeric_and_array_limits() {
        let s = for_provider(&sample(), Provider::Anthropic);
        let p = &s["properties"];
        assert_eq!(p["kind"], json!({ "type": "string", "enum": ["a", "b"] }));
        assert_eq!(p["estimateMin"], json!({ "type": "integer", "description": "{\"minimum\":5,\"maximum\":240}" }));
        assert_eq!(p["tags"]["minItems"], json!(1));
        assert_eq!(p["tags"]["description"], "{\"maxItems\":5}");
        assert_eq!(p["steps"], json!({ "type": "array", "items": { "type": "string" }, "description": "{\"minItems\":2}" }));
        assert_eq!(p["title"]["description"], "{\"maxLength\":120}");
    }

    #[test]
    fn gemini_schema_keeps_numeric_limits() {
        let s = for_provider(&sample(), Provider::Gemini);
        let p = &s["properties"];
        assert_eq!(p["estimateMin"], json!({ "type": "integer", "minimum": 5, "maximum": 240 }));
        assert_eq!(p["tags"]["maxItems"], json!(5));
        assert!(p["title"].get("maxLength").is_none());
        // Servers and the local model keep everything.
        for local in [Provider::Local, Provider::Ollama, Provider::LmStudio] {
            let s = for_provider(&sample(), local);
            assert_eq!(s["properties"], sample()["properties"]);
        }
    }

    #[test]
    fn real_schemas_convert_for_every_provider() {
        let live = categories::list(&Db::open_in_memory().unwrap().conn()).unwrap();
        let schemas = [
            ai::schema(&live),
            extract::notes_schema(&["t1".into()], true),
            extract::notes_schema(&[], false),
            extract::merge_schema(),
        ];
        for p in std::iter::once(Provider::Local).chain(Provider::CLOUD) {
            for s in &schemas {
                let mut bad = Vec::new();
                violations(&for_provider(s, p), p, "$", &mut bad);
                assert!(bad.is_empty(), "{p:?}: {bad:?}");
            }
        }
    }

    #[test]
    fn clamp_enforces_original_limits() {
        let mut v = json!({
            "title": "é".repeat(200),
            "kind": "a",
            "tags": ["1", "2", "3", "4", "5", "6", "x".repeat(40)],
            "estimateMin": 999,
            "note": "",
        });
        clamp(&mut v, &sample());
        assert_eq!(v["title"].as_str().unwrap().chars().count(), 120);
        assert_eq!(v["tags"], json!(["1", "2", "3", "4", "5"]));
        assert_eq!(v["estimateMin"], json!(240));
        let mut low = json!({ "estimateMin": 1, "tags": ["y".repeat(40)] });
        clamp(&mut low, &sample());
        assert_eq!(low["estimateMin"], json!(5));
        assert_eq!(low["tags"][0].as_str().unwrap().len(), 32);
    }
}
