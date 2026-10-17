// Copyright 2026 Tracer Cloud. SPDX-License-Identifier: Apache-2.0
// Modified: recursive Rust validation; bounded work and value-free error messages.
use anyhow::{Result, bail, ensure};
use serde_json::Value;

/// Validate the supported JSON Schema subset without placing supplied values in errors.
/// Unknown annotation keywords are ignored; local $ref is resolved against the root.
pub fn validate_input(schema: &Value, input: &Value) -> Result<()> {
    ensure!(
        schema["type"] == "object",
        "tool exposes a non-object input schema"
    );
    ensure!(input.is_object(), "tool expected object input");
    let mut work = 0;
    check(schema, input, schema, 0, &mut work)
}
fn kind(value: &Value, expected: &str) -> bool {
    match expected {
        "null" => value.is_null(),
        "boolean" => value.is_boolean(),
        "integer" => value.is_i64() || value.is_u64(),
        "number" => value.is_number(),
        "string" => value.is_string(),
        "array" => value.is_array(),
        "object" => value.is_object(),
        _ => false,
    }
}
fn check(
    schema: &Value,
    input: &Value,
    root: &Value,
    depth: usize,
    work: &mut usize,
) -> Result<()> {
    *work += 1;
    ensure!(
        depth <= 32 && *work <= 8192,
        "tool schema validation budget exceeded"
    );
    if schema == &Value::Bool(true) {
        return Ok(());
    }
    ensure!(
        schema != &Value::Bool(false),
        "tool argument rejected by schema"
    );
    ensure!(schema.is_object(), "invalid tool schema");
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        ensure!(
            reference.starts_with("#/"),
            "only local schema references are supported"
        );
        let resolved = root
            .pointer(&reference[1..])
            .ok_or_else(|| anyhow::anyhow!("unresolved schema reference"))?;
        check(resolved, input, root, depth + 1, work)?;
    }
    if input.is_null() && schema["nullable"] == true {
        return Ok(());
    }
    if let Some(values) = schema.get("enum").and_then(Value::as_array) {
        ensure!(
            values.contains(input),
            "tool argument is outside its allowed enum"
        );
    }
    if let Some(value) = schema.get("const") {
        ensure!(input == value, "tool argument does not match its constant");
    }
    for (key, exact) in [("oneOf", true), ("anyOf", false)] {
        if let Some(options) = schema.get(key).and_then(Value::as_array) {
            ensure!(!options.is_empty(), "empty schema alternatives");
            let mut matches = 0;
            for option in options {
                if check(option, input, root, depth + 1, work).is_ok() {
                    matches += 1;
                }
            }
            ensure!(*work <= 8192, "tool schema validation budget exceeded");
            ensure!(
                if exact { matches == 1 } else { matches >= 1 },
                "tool argument does not match schema alternatives"
            );
        }
    }
    if let Some(options) = schema.get("allOf").and_then(Value::as_array) {
        for option in options {
            check(option, input, root, depth + 1, work)?;
        }
    }
    if let Some(expected) = schema.get("type") {
        let matches = match expected {
            Value::String(name) => kind(input, name),
            Value::Array(names) => names
                .iter()
                .any(|n| n.as_str().is_some_and(|name| kind(input, name))),
            _ => false,
        };
        ensure!(matches, "tool argument has invalid type");
    }
    if let Some(object) = input.as_object() {
        if let Some(required) = schema.get("required").and_then(Value::as_array) {
            ensure!(
                required
                    .iter()
                    .all(|key| key.as_str().is_some_and(|k| object.contains_key(k))),
                "tool input is missing required arguments"
            );
        }
        let properties = schema.get("properties").and_then(Value::as_object);
        for (key, value) in object {
            if let Some(property) = properties.and_then(|p| p.get(key)) {
                check(property, value, root, depth + 1, work)?;
            } else {
                match schema.get("additionalProperties") {
                    Some(Value::Bool(false)) => bail!("tool input contains unexpected arguments"),
                    Some(rule) if rule.is_object() => check(rule, value, root, depth + 1, work)?,
                    _ => {}
                }
            }
        }
    }
    if let Some(items) = input.as_array() {
        length_bounds(items.len(), schema, "minItems", "maxItems")?;
        if let Some(rule) = schema.get("items") {
            for item in items {
                check(rule, item, root, depth + 1, work)?;
            }
        }
        if schema["uniqueItems"] == true {
            ensure!(
                items.len() <= 1024,
                "unique-item validation budget exceeded"
            );
            for (i, item) in items.iter().enumerate() {
                ensure!(
                    !items[..i].contains(item),
                    "tool array contains duplicate items"
                );
            }
        }
    }
    if let Some(text) = input.as_str() {
        length_bounds(text.chars().count(), schema, "minLength", "maxLength")?;
    }
    if let Some(number) = input.as_f64() {
        ensure!(
            !schema["minimum"].as_f64().is_some_and(|min| number < min),
            "tool number is below its minimum"
        );
        ensure!(
            !schema["maximum"].as_f64().is_some_and(|max| number > max),
            "tool number exceeds its maximum"
        );
    }
    Ok(())
}
fn length_bounds(len: usize, schema: &Value, min: &str, max: &str) -> Result<()> {
    ensure!(
        schema[min].as_u64().is_none_or(|n| (len as u64) >= n),
        "tool value is shorter than its minimum"
    );
    ensure!(
        schema[max].as_u64().is_none_or(|n| (len as u64) <= n),
        "tool value exceeds its maximum length"
    );
    Ok(())
}
