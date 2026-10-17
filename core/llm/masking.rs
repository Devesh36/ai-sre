use serde_json::{Value, json};
pub struct Masker(Vec<(String, String)>);
impl Masker {
    pub fn new(values: &[String]) -> Self {
        let mut values = values.to_vec();
        values.sort_by_key(|v| std::cmp::Reverse(v.len()));
        values.dedup();
        Self(
            values
                .into_iter()
                .enumerate()
                .map(|(i, v)| (v, format!("__SRE_IDENTIFIER_{i}__")))
                .collect(),
        )
    }
    pub fn mask(&self, text: &str) -> String {
        self.0.iter().fold(text.into(), |s, (a, b)| s.replace(a, b))
    }
    pub fn restore(&self, text: &str) -> String {
        self.0.iter().fold(text.into(), |s, (a, b)| s.replace(b, a))
    }
    pub fn restore_value(&self, value: &mut Value) {
        map_strings(value, &|s| self.restore(s));
    }
    pub fn messages(&self, messages: &[Value]) -> Vec<Value> {
        let mut result = messages.to_vec();
        for message in &mut result {
            if let Some(text) = message["content"].as_str() {
                message["content"] = json!(self.payload(text));
            }
            if let Some(calls) = message["tool_calls"].as_array_mut() {
                for call in calls {
                    if let Some(args) = call["function"]["arguments"].as_str() {
                        call["function"]["arguments"] = json!(self.payload(args));
                    }
                }
            }
        }
        result
    }
    fn payload(&self, text: &str) -> String {
        if let Ok(mut value) = serde_json::from_str::<Value>(text) {
            map_strings(&mut value, &|s| self.mask(s));
            value.to_string()
        } else {
            self.mask(text)
        }
    }
}

fn map_strings(value: &mut Value, f: &impl Fn(&str) -> String) {
    match value {
        Value::String(s) => *s = f(s),
        Value::Array(a) => a.iter_mut().for_each(|v| map_strings(v, f)),
        Value::Object(o) => o.values_mut().for_each(|v| map_strings(v, f)),
        _ => {}
    }
}
