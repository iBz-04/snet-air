use snet_wire::JsonValue;

/// JSON text written into a buffer sized first, so text holding restored
/// values never leaves a copy behind when the buffer grows.
pub(crate) trait Sink {
    fn push_str(&mut self, text: &str);
}

pub(crate) struct Counter(pub(crate) usize);

impl Sink for Counter {
    fn push_str(&mut self, text: &str) {
        self.0 += text.len();
    }
}

impl Sink for String {
    fn push_str(&mut self, text: &str) {
        String::push_str(self, text);
    }
}

pub(crate) fn string(sink: &mut dyn Sink, text: &str) {
    sink.push_str("\"");
    let mut run_start = 0;
    for (index, character) in text.char_indices() {
        let escape = match character {
            '"' => Some("\\\""),
            '\\' => Some("\\\\"),
            '\n' => Some("\\n"),
            '\r' => Some("\\r"),
            '\t' => Some("\\t"),
            '\u{08}' => Some("\\b"),
            '\u{0C}' => Some("\\f"),
            _ => None,
        };
        let control = character < '\u{20}';
        if escape.is_none() && !control {
            continue;
        }
        sink.push_str(&text[run_start..index]);
        match escape {
            Some(sequence) => sink.push_str(sequence),
            None => sink.push_str(&format!("\\u{:04x}", u32::from(character))),
        }
        run_start = index + character.len_utf8();
    }
    sink.push_str(&text[run_start..]);
    sink.push_str("\"");
}

pub(crate) fn value(sink: &mut dyn Sink, json: &JsonValue) {
    match json {
        JsonValue::Null => sink.push_str("null"),
        JsonValue::Bool(true) => sink.push_str("true"),
        JsonValue::Bool(false) => sink.push_str("false"),
        JsonValue::Number(number) => sink.push_str(&number.to_string()),
        JsonValue::String(text) => string(sink, text),
        JsonValue::Array(items) => {
            sink.push_str("[");
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    sink.push_str(",");
                }
                value(sink, item);
            }
            sink.push_str("]");
        }
        JsonValue::Object(members) => {
            sink.push_str("{");
            for (index, (key, item)) in members.iter().enumerate() {
                if index > 0 {
                    sink.push_str(",");
                }
                string(sink, key);
                sink.push_str(":");
                value(sink, item);
            }
            sink.push_str("}");
        }
    }
}

/// Whether `json` nests deeper than `limit` levels, found without
/// recursion so a value built in code cannot exhaust the stack.
pub(crate) fn nests_deeper_than(json: &JsonValue, limit: usize) -> bool {
    let mut pending = vec![(json, 1usize)];
    while let Some((item, level)) = pending.pop() {
        if level > limit {
            return true;
        }
        match item {
            JsonValue::Array(items) => pending.extend(items.iter().map(|inner| (inner, level + 1))),
            JsonValue::Object(members) => {
                pending.extend(members.values().map(|inner| (inner, level + 1)));
            }
            _ => {}
        }
    }
    false
}
