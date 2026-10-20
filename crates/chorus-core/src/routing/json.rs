//! Field checks for JSON that arrives from outside (OpenRouter's rows, the bundled registry).
//! Each failure is one `<path>: <message>` issue, the first in field order, as Chorus 1's Zod
//! schemas reported their first issue. Integers are JavaScript's: any whole number in the safe
//! range, so `5.0` counts.

use serde_json::{Map, Value};

pub type Obj = Map<String, Value>;

/// Where a value sits, as dotted keys and indexes (`pricing.overrides.0.utc_start`).
#[derive(Debug, Clone, Default)]
pub struct Path(Vec<String>);

impl Path {
    pub fn key(&self, key: impl ToString) -> Self {
        let mut p = self.0.clone();
        p.push(key.to_string());
        Self(p)
    }

    pub fn issue(&self, message: &str) -> String {
        let at = if self.0.is_empty() { "(root)".to_owned() } else { self.0.join(".") };
        format!("{at}: {message}")
    }
}

const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

fn kind(v: Option<&Value>) -> &'static str {
    match v {
        None => "undefined",
        Some(Value::Null) => "null",
        Some(Value::Bool(_)) => "boolean",
        Some(Value::Number(_)) => "number",
        Some(Value::String(_)) => "string",
        Some(Value::Array(_)) => "array",
        Some(Value::Object(_)) => "object",
    }
}

pub fn expected(path: &Path, want: &str, got: Option<&Value>) -> String {
    path.issue(&format!("Invalid input: expected {want}, received {}", kind(got)))
}

/// A whole number in JavaScript's safe range.
pub fn int_of(v: &Value) -> Option<i64> {
    if let Some(i) = v.as_i64() {
        return (i.unsigned_abs() <= MAX_SAFE_INTEGER as u64).then_some(i);
    }
    let f = v.as_f64()?;
    (f.fract() == 0.0 && f.abs() <= MAX_SAFE_INTEGER).then_some(f as i64)
}

pub fn object<'a>(v: Option<&'a Value>, path: &Path) -> Result<&'a Obj, String> {
    v.and_then(Value::as_object).ok_or_else(|| expected(path, "object", v))
}

/// A strict object: any key outside `known` is an issue.
pub fn no_unknown_keys(obj: &Obj, known: &[&str], path: &Path) -> Result<(), String> {
    match obj.keys().find(|k| !known.contains(&k.as_str())) {
        Some(k) => Err(path.issue(&format!("Unrecognized key: \"{k}\""))),
        None => Ok(()),
    }
}

/// A string of at least `min_len` characters (UTF-16 code units, as JavaScript counts).
pub fn string_value(v: Option<&Value>, path: &Path, min_len: usize) -> Result<String, String> {
    let s = v.and_then(Value::as_str).ok_or_else(|| expected(path, "string", v))?;
    if s.encode_utf16().count() < min_len {
        return Err(path.issue(&format!("Too small: expected string to have >={min_len} characters")));
    }
    Ok(s.to_owned())
}

pub fn string(obj: &Obj, key: &str, path: &Path, min_len: usize) -> Result<String, String> {
    string_value(obj.get(key), &path.key(key), min_len)
}

/// Absent or null is `None` (Zod's `nullish`).
pub fn nullish<'a>(obj: &'a Obj, key: &str) -> Option<&'a Value> {
    obj.get(key).filter(|v| !v.is_null())
}

pub fn nullish_string(obj: &Obj, key: &str, path: &Path) -> Result<Option<String>, String> {
    match nullish(obj, key) {
        None => Ok(None),
        Some(v) => v.as_str().map(|s| Some(s.to_owned())).ok_or_else(|| expected(&path.key(key), "string", Some(v))),
    }
}

pub fn number_value(v: Option<&Value>, path: &Path) -> Result<f64, String> {
    v.and_then(Value::as_f64).ok_or_else(|| expected(path, "number", v))
}

pub fn nullish_number(obj: &Obj, key: &str, path: &Path) -> Result<Option<f64>, String> {
    nullish(obj, key).map(|v| number_value(Some(v), &path.key(key))).transpose()
}

/// Absent is `None`; null is an issue (Zod's `optional`).
pub fn optional_number(obj: &Obj, key: &str, path: &Path) -> Result<Option<f64>, String> {
    obj.get(key).map(|v| number_value(Some(v), &path.key(key))).transpose()
}

pub fn int_value(v: Option<&Value>, path: &Path) -> Result<i64, String> {
    let n = v.and_then(Value::as_f64).ok_or_else(|| expected(path, "number", v))?;
    v.and_then(int_of).ok_or_else(|| path.issue(&format!("Invalid input: expected int, received {}", super::js::number(n))))
}

/// An integer within `min..=max`.
pub fn int_in(v: Option<&Value>, path: &Path, min: i64, max: i64) -> Result<i64, String> {
    let i = int_value(v, path)?;
    if i < min {
        let bound = if min == 1 { ">0".to_owned() } else { format!(">={min}") };
        return Err(path.issue(&format!("Too small: expected number to be {bound}")));
    }
    if i > max {
        return Err(path.issue(&format!("Too big: expected number to be <={max}")));
    }
    Ok(i)
}

pub fn bool_value(v: Option<&Value>, path: &Path) -> Result<bool, String> {
    v.and_then(Value::as_bool).ok_or_else(|| expected(path, "boolean", v))
}

pub fn array<'a>(v: Option<&'a Value>, path: &Path) -> Result<&'a Vec<Value>, String> {
    v.and_then(Value::as_array).ok_or_else(|| expected(path, "array", v))
}

pub fn string_array(v: Option<&Value>, path: &Path, min_len: usize) -> Result<Vec<String>, String> {
    array(v, path)?.iter().enumerate().map(|(i, item)| string_value(Some(item), &path.key(i), min_len)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn issues_name_their_path() {
        let root = Path::default();
        assert_eq!(root.issue("x"), "(root): x");
        assert_eq!(root.key("pricing").key(0).key("prompt").issue("x"), "pricing.0.prompt: x");
        let v = json!({ "a": 5.0, "b": 1.5, "c": "s", "d": null, "e": ["x", 1] });
        let o = v.as_object().unwrap();
        assert_eq!(int_value(o.get("a"), &root.key("a")), Ok(5));
        assert_eq!(int_value(o.get("b"), &root.key("b")), Err("b: Invalid input: expected int, received 1.5".into()));
        assert_eq!(string(o, "missing", &root, 1), Err("missing: Invalid input: expected string, received undefined".into()));
        assert_eq!(string(o, "c", &root, 2), Err("c: Too small: expected string to have >=2 characters".into()));
        assert_eq!(nullish_string(o, "d", &root), Ok(None));
        assert_eq!(optional_number(o, "d", &root), Err("d: Invalid input: expected number, received null".into()));
        assert_eq!(string_array(o.get("e"), &root.key("e"), 0), Err("e.1: Invalid input: expected string, received number".into()));
        assert_eq!(int_in(o.get("a"), &root.key("a"), 6, 9), Err("a: Too small: expected number to be >=6".into()));
        assert_eq!(int_of(&json!(9_007_199_254_740_992_i64)), None, "beyond the safe range");
    }
}
