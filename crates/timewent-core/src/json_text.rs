//! JSON text that reads like a document, one item per line:
//! - a value stays on one line when it fits in `WIDTH` columns;
//! - an item of a list stays on one line when it is shallow (plain values, or plain lists and
//!   objects inside it: a `where` row, a block with its `why`), however long;
//! - anything else puts its members one per line.
//!
//! Works on compact `serde_json` output, so key order is exactly the serializer's. Pure, no
//! dependency.

/// Line width a value may take (indentation included) before it is expanded.
pub const WIDTH: usize = 100;

pub fn to_text<T: serde::Serialize>(value: &T) -> Result<String, serde_json::Error> {
    let compact = serde_json::to_string(value)?;
    let mut out = String::with_capacity(compact.len() * 2);
    let b = compact.as_bytes();
    write_value(&compact, b, 0, 0, false, &mut out);
    out.push('\n');
    Ok(out)
}

/// Index just past the value starting at `i` (compact JSON: no whitespace).
fn value_end(b: &[u8], i: usize) -> usize {
    match b[i] {
        b'"' => string_end(b, i),
        b'{' | b'[' => {
            let mut depth = 0;
            let mut j = i;
            while j < b.len() {
                match b[j] {
                    b'"' => {
                        j = string_end(b, j);
                        continue;
                    }
                    b'{' | b'[' => depth += 1,
                    b'}' | b']' => {
                        depth -= 1;
                        if depth == 0 {
                            return j + 1;
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
            b.len()
        }
        _ => {
            let mut j = i;
            while j < b.len() && !matches!(b[j], b',' | b'}' | b']') {
                j += 1;
            }
            j
        }
    }
}

fn string_end(b: &[u8], i: usize) -> usize {
    let mut j = i + 1;
    while j < b.len() {
        match b[j] {
            b'\\' => j += 2,
            b'"' => return j + 1,
            _ => j += 1,
        }
    }
    b.len()
}

/// Writes the value at `i`, indented `indent`; `in_list` = it is an item of an array.
fn write_value(
    s: &str,
    b: &[u8],
    i: usize,
    indent: usize,
    in_list: bool,
    out: &mut String,
) -> usize {
    let end = value_end(b, i);
    let col = out.len() - out.rfind('\n').map_or(0, |n| n + 1);
    let width = s[i..end].chars().count() + spaced_extra(&s[i..end]);
    let shallow_item = in_list && depth(b, i, end) <= 3;
    if !matches!(b[i], b'{' | b'[') || col + width <= WIDTH || end - i <= 2 || shallow_item {
        write_inline(&s[i..end], out);
        return end;
    }
    let (open, close) = (b[i] as char, b[end - 1] as char);
    out.push(open);
    let mut j = i + 1;
    let pad = " ".repeat(indent + 2);
    while j < end - 1 {
        out.push('\n');
        out.push_str(&pad);
        if open == '{' {
            let k = string_end(b, j);
            out.push_str(&s[j..k]);
            out.push_str(": ");
            j = k + 1; // past ':'
        }
        j = write_value(s, b, j, indent + 2, open == '[', out);
        if b[j] == b',' {
            out.push(',');
            j += 1;
        }
    }
    out.push('\n');
    out.push_str(&" ".repeat(indent));
    out.push(close);
    end
}

/// Nesting depth of the container at `i..end` (a flat object or array is 1).
fn depth(b: &[u8], i: usize, end: usize) -> usize {
    let (mut d, mut max, mut j) = (0usize, 0usize, i);
    while j < end {
        match b[j] {
            b'"' => {
                j = string_end(b, j);
                continue;
            }
            b'{' | b'[' => {
                d += 1;
                max = max.max(d);
            }
            b'}' | b']' => d = d.saturating_sub(1),
            _ => {}
        }
        j += 1;
    }
    max
}

/// One-line form adds a space after `:` and `,` outside strings.
fn spaced_extra(v: &str) -> usize {
    let b = v.as_bytes();
    let (mut n, mut j) = (0, 0);
    while j < b.len() {
        match b[j] {
            b'"' => {
                j = string_end(b, j);
                continue;
            }
            b':' | b',' => n += 1,
            _ => {}
        }
        j += 1;
    }
    n
}

fn write_inline(v: &str, out: &mut String) {
    let b = v.as_bytes();
    let mut j = 0;
    while j < b.len() {
        match b[j] {
            b'"' => {
                let k = string_end(b, j);
                out.push_str(&v[j..k]);
                j = k;
                continue;
            }
            b':' => out.push_str(": "),
            b',' => out.push_str(", "),
            _ => out.push_str(&v[j..j + v[j..].chars().next().map_or(1, char::len_utf8)]),
        }
        j += v[j..].chars().next().map_or(1, char::len_utf8);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn short_values_stay_on_one_line_long_ones_expand() {
        let long = "y".repeat(WIDTH);
        let v = json!({"a": {"x": 1, "y": long.clone()}, "b": {"x": 1}});
        let text = to_text(&v).expect("text");
        assert_eq!(
            text,
            format!("{{\n  \"a\": {{\n    \"x\": 1,\n    \"y\": \"{long}\"\n  }},\n  \"b\": {{\"x\": 1}}\n}}\n")
        );
    }

    #[test]
    fn list_items_are_one_line_each_unless_deeply_nested() {
        let long = "z".repeat(WIDTH);
        let v = json!({
            "rows": [
                {"name": long.clone(), "breakdown": {"code": 1}, "top": [{"name": "a", "seconds": 1}]},
                {"name": "b"}
            ],
            "sessions": [{"from": "x", "blocks": [{"name": long.clone(), "why": ["w"]}]}]
        });
        let text = to_text(&v).expect("text");
        let lines: Vec<&str> = text.lines().collect();
        // a where-like row (depth 3, its own braces included) is one line
        assert!(
            lines.iter().any(|l| l.trim_start().starts_with('{')
                && l.contains("\"name\": \"zz")
                && l.contains("\"top\"")),
            "{text}"
        );
        // a session (blocks of objects with lists: depth 4) expands; its block is one line
        assert!(
            lines.iter().any(|l| l.trim() == "\"from\": \"x\""),
            "{text}"
        );
        assert!(
            lines.iter().any(|l| l.trim_start().starts_with('{')
                && l.contains("\"name\": \"zz")
                && l.contains("\"why\"")),
            "{text}"
        );
        let back: serde_json::Value = serde_json::from_str(&text).expect("valid");
        assert_eq!(back, v);
    }

    #[test]
    fn strings_with_quotes_brackets_and_unicode_survive() {
        let v = json!({"t": "a \"q\" [b] {c}, d: é — ✓", "n": null, "e": [], "o": {}, "l": [[1, [2, [3]]]]});
        let back: serde_json::Value =
            serde_json::from_str(&to_text(&v).expect("text")).expect("valid");
        assert_eq!(back, v);
    }
}
