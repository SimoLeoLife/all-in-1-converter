//! MySqlDataReader-style column access on mysql_async rows.

use anyhow::{anyhow, bail, Result};
use mysql_async::{Row, Value};

const NULL_MESSAGE: &str = "Data is Null. This method or property cannot be called on Null values.";

fn value(row: &Row, column: impl ColumnRef) -> Result<&Value> {
    let index = column.index(row).ok_or_else(|| anyhow!("Could not find specified column in results: {}", column.name()))?;
    row.as_ref(index).ok_or_else(|| anyhow!("Could not find specified column in results: {}", column.name()))
}

pub trait ColumnRef: Copy {
    fn index(self, row: &Row) -> Option<usize>;
    fn name(self) -> String;
}

impl ColumnRef for usize {
    fn index(self, _row: &Row) -> Option<usize> {
        Some(self)
    }
    fn name(self) -> String {
        self.to_string()
    }
}

impl ColumnRef for &str {
    fn index(self, row: &Row) -> Option<usize> {
        row.columns_ref().iter().position(|c| c.name_str().eq_ignore_ascii_case(self))
    }
    fn name(self) -> String {
        self.to_string()
    }
}

pub fn is_null(row: &Row, column: impl ColumnRef) -> Result<bool> {
    Ok(matches!(value(row, column)?, Value::NULL))
}

/// `reader.GetInt32(column)` / `Convert.ToInt32(reader.GetValue(column))`.
pub fn get_i32(row: &Row, column: impl ColumnRef) -> Result<i32> {
    let v = value(row, column)?;
    let out = match v {
        Value::NULL => bail!(NULL_MESSAGE),
        Value::Int(i) => i32::try_from(*i).ok(),
        Value::UInt(u) => i32::try_from(*u).ok(),
        Value::Float(f) => Some(f.round_ties_even() as i32),
        Value::Double(f) => Some(f.round_ties_even() as i32),
        Value::Bytes(bytes) => {
            let text = String::from_utf8_lossy(bytes);
            let text = text.trim();
            text.parse::<i32>().ok().or_else(|| text.parse::<f64>().ok().map(|f| f.round_ties_even() as i32))
        }
        _ => None,
    };
    out.ok_or_else(|| anyhow!("Input string was not in a correct format."))
}

/// `Convert.ToDouble(reader.GetValue(column))`.
pub fn get_f64(row: &Row, column: impl ColumnRef) -> Result<f64> {
    let v = value(row, column)?;
    let out = match v {
        Value::NULL => bail!(NULL_MESSAGE),
        Value::Int(i) => Some(*i as f64),
        Value::UInt(u) => Some(*u as f64),
        Value::Float(f) => Some(*f as f64),
        Value::Double(f) => Some(*f),
        Value::Bytes(bytes) => String::from_utf8_lossy(bytes).trim().parse::<f64>().ok(),
        _ => None,
    };
    out.ok_or_else(|| anyhow!("Input string was not in a correct format."))
}

/// `reader.GetString(column)`.
pub fn get_string(row: &Row, column: impl ColumnRef) -> Result<String> {
    match value(row, column)? {
        Value::NULL => bail!(NULL_MESSAGE),
        other => Ok(value_to_string(other)),
    }
}

/// `reader[column].ToString()` (DBNull is "").
pub fn get_display(row: &Row, column: impl ColumnRef) -> Result<String> {
    Ok(value_to_string(value(row, column)?))
}

pub fn value_to_string(value: &Value) -> String {
    match value {
        Value::NULL => String::new(),
        Value::Bytes(bytes) => String::from_utf8_lossy(bytes).into_owned(),
        Value::Int(i) => i.to_string(),
        Value::UInt(u) => u.to_string(),
        Value::Float(f) => crate::dotnet::double_to_string(*f as f64),
        Value::Double(f) => crate::dotnet::double_to_string(*f),
        Value::Date(y, mo, d, h, mi, s, _) => format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}"),
        Value::Time(neg, d, h, mi, s, _) => format!("{}{}{:02}:{:02}:{:02}", if *neg { "-" } else { "" }, if *d > 0 { format!("{d}.") } else { String::new() }, h, mi, s),
    }
}
