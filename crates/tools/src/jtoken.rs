//! The bits of Newtonsoft `JToken` behaviour the merge tools rely on, over `serde_json::Value`.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Result};
use serde_json::Value;

/// .NET's message for a null dereference (the C# code let these escape into its catch).
pub const NULL_REFERENCE: &str = "Object reference not set to an instance of an object.";

/// `token[key]` on a JObject (`None` when missing or when `token` is not an object).
pub fn index<'a>(token: Option<&'a Value>, key: &str) -> Option<&'a Value> {
    token.and_then(|t| t.as_object()).and_then(|o| o.get(key))
}

/// `token[key]!` / a dereference that throws when missing.
pub fn req<'a>(token: Option<&'a Value>, key: &str) -> Result<&'a Value> {
    index(token, key).ok_or_else(|| anyhow!(NULL_REFERENCE))
}

/// `JToken.ToString()` of a value.
pub fn to_string(token: &Value) -> String {
    match token {
        Value::Null => String::new(),
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        Value::String(s) => s.clone(),
        Value::Number(n) => match n.as_f64() {
            Some(f) if !n.is_i64() && !n.is_u64() && f.fract() == 0.0 && f.abs() < 1e15 => format!("{f:.0}"),
            _ => n.to_string(),
        },
        other => serde_json::to_string_pretty(other).unwrap_or_default(),
    }
}

/// `token["key"].ToString()` where a missing key throws.
pub fn key_string(token: &Value, key: &str) -> Result<String> {
    Ok(to_string(req(Some(token), key)?))
}

/// `JToken.Value<int>()`.
pub fn value_int(token: &Value) -> Result<i32> {
    match token {
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i32::try_from(i).map_err(|_| anyhow!("Value was either too large or too small for an Int32."))
            } else {
                let f = n.as_f64().unwrap_or(0.0).round_ties_even();
                if f < i32::MIN as f64 || f > i32::MAX as f64 {
                    bail!("Value was either too large or too small for an Int32.");
                }
                Ok(f as i32)
            }
        }
        Value::String(s) => s.trim().parse::<i32>().map_err(|_| anyhow!("The input string '{s}' was not in a correct format.")),
        Value::Bool(b) => Ok(*b as i32),
        Value::Null => bail!("Null object cannot be converted to a value type."),
        Value::Array(_) => bail!("Cannot cast Newtonsoft.Json.Linq.JArray to System.Int32."),
        Value::Object(_) => bail!("Cannot cast Newtonsoft.Json.Linq.JObject to System.Int32."),
    }
}

/// `JToken.Value<string>()`: `None` for JSON null.
pub fn value_string(token: &Value) -> Result<Option<String>> {
    match token {
        Value::Null => Ok(None),
        Value::Array(_) => bail!("Cannot cast Newtonsoft.Json.Linq.JArray to System.String."),
        Value::Object(_) => bail!("Cannot cast Newtonsoft.Json.Linq.JObject to System.String."),
        other => Ok(Some(to_string(other))),
    }
}

/// `Cast<JObject>()` check for one element.
pub fn as_jobject(token: &Value) -> Result<&serde_json::Map<String, Value>> {
    token
        .as_object()
        .ok_or_else(|| anyhow!("Unable to cast object of type 'Newtonsoft.Json.Linq.JValue' to type 'Newtonsoft.Json.Linq.JObject'."))
}

pub fn duplicate_key(key: &str) -> anyhow::Error {
    anyhow!("An item with the same key has already been added. Key: {key}")
}

/// `Directory.GetFiles(dir, prefix + "*.json")`, in the order NTFS lists them (case-insensitive).
pub fn json_files(dir: &Path, prefix: &str) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|entries| entries.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    let prefix = prefix.to_uppercase();
    files.retain(|p| {
        p.is_file()
            && habbo_core::paths::has_ext(p, "json")
            && habbo_core::paths::file_name(p).to_uppercase().starts_with(&prefix)
    });
    files.sort_by_cached_key(|p| (habbo_core::paths::file_name(p).to_uppercase(), habbo_core::paths::file_name(p)));
    files
}

/// The JSON load of the C# IO helpers, telling "file not found" (which the merge tools catch)
/// apart from every other error (which escapes the operation).
pub enum Loaded {
    Ok(serde_json::Map<String, Value>),
    NotFound(String),
}

pub fn load(path: &Path) -> Result<Loaded> {
    // JsonReadHelper.ValidateJsonFilePath: extension and directory checks come first.
    if habbo_core::paths::has_ext(path, "json") && !path.is_dir() && !path.is_file() {
        return Ok(Loaded::NotFound(format!("JSON file not found: {}", path.display())));
    }
    Ok(Loaded::Ok(habbo_core::json::load_object(path)?))
}

/// `Comparer<string>.Default` (culture-sensitive, ICU root collation) approximated:
/// punctuation < digits < letters, letters compared case-insensitively first, then
/// lowercase before uppercase, then ordinal.
/// NOTE(port): .NET sorted ProductData codes with the current culture; this approximates the
/// ICU root order for the ASCII codes Habbo uses. Exotic characters fall back to code points.
pub fn culture_compare(a: &str, b: &str) -> Ordering {
    let primary = |s: &str| s.chars().map(primary_weight).collect::<Vec<u32>>();
    primary(a)
        .cmp(&primary(b))
        .then_with(|| {
            let tertiary = |s: &str| s.chars().map(|c| c.is_uppercase() as u8).collect::<Vec<u8>>();
            tertiary(a).cmp(&tertiary(b))
        })
        .then_with(|| a.cmp(b))
}

fn primary_weight(c: char) -> u32 {
    const PUNCTUATION: &str = " _-,;:!?.'\"()[]{}@*/\\&#%`^+<=>|~$";
    if let Some(i) = PUNCTUATION.find(c) {
        return 1 + i as u32;
    }
    if c.is_ascii_digit() {
        return 100 + c as u32 - '0' as u32;
    }
    if c.is_ascii_alphabetic() {
        return 200 + c.to_ascii_lowercase() as u32 - 'a' as u32;
    }
    if c.is_whitespace() || c.is_control() {
        return 0;
    }
    1000 + c.to_lowercase().next().unwrap_or(c) as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn newtonsoft_conversions() {
        assert_eq!(to_string(&json!("a")), "a");
        assert_eq!(to_string(&json!(12)), "12");
        assert_eq!(to_string(&json!(null)), "");
        assert_eq!(value_int(&json!("42")).unwrap(), 42);
        assert_eq!(value_int(&json!(2.5)).unwrap(), 2);
        assert!(value_int(&json!(null)).is_err());
    }

    #[test]
    fn culture_order() {
        let mut codes = vec!["b", "B", "a_2", "a2", "A1", "10", "9"];
        codes.sort_by(|a, b| culture_compare(a, b));
        assert_eq!(codes, ["10", "9", "a_2", "A1", "a2", "b", "B"]);
    }
}
