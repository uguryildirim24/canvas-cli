//! Convert `canvas_api::Supplied` into store [`FieldWrite`] values.

use canvas_api::Supplied as ApiSupplied;
use jiff::Timestamp;

use crate::store::{FieldGroup, FieldWrite};

/// Map API three-state supply onto a [`FieldWrite`].
///
/// - `Absent` → skip (no write)
/// - `Null` → `FieldWrite { value: None }`
/// - `Value(v)` → `FieldWrite { value: Some(serialized) }`
pub fn push_api_str(
    fields: &mut Vec<FieldWrite>,
    name: &'static str,
    group: FieldGroup,
    supplied: &ApiSupplied<String>,
) {
    match supplied {
        ApiSupplied::Absent => {}
        ApiSupplied::Null => fields.push(FieldWrite {
            name,
            group,
            value: None,
        }),
        ApiSupplied::Value(v) => fields.push(FieldWrite {
            name,
            group,
            value: Some(v.clone()),
        }),
    }
}

/// Push an API-supplied floating value as a decimal string.
pub fn push_api_f64(
    fields: &mut Vec<FieldWrite>,
    name: &'static str,
    group: FieldGroup,
    supplied: &ApiSupplied<f64>,
) {
    match supplied {
        ApiSupplied::Absent => {}
        ApiSupplied::Null => fields.push(FieldWrite {
            name,
            group,
            value: None,
        }),
        ApiSupplied::Value(v) => fields.push(FieldWrite {
            name,
            group,
            value: Some(v.to_string()),
        }),
    }
}

/// Push an API-supplied value rendered with [`ToString`].
pub fn push_api_to_string<T: ToString>(
    fields: &mut Vec<FieldWrite>,
    name: &'static str,
    group: FieldGroup,
    supplied: &ApiSupplied<T>,
) {
    match supplied {
        ApiSupplied::Absent => {}
        ApiSupplied::Null => fields.push(FieldWrite {
            name,
            group,
            value: None,
        }),
        ApiSupplied::Value(v) => fields.push(FieldWrite {
            name,
            group,
            value: Some(v.to_string()),
        }),
    }
}

/// Push an optional string as a supplied field when `Some`.
pub fn push_opt_str(
    fields: &mut Vec<FieldWrite>,
    name: &'static str,
    group: FieldGroup,
    value: Option<&str>,
) {
    if let Some(v) = value {
        fields.push(FieldWrite {
            name,
            group,
            value: Some(v.to_owned()),
        });
    }
}

/// Push an optional bool as `"true"` / `"false"`.
pub fn push_opt_bool(
    fields: &mut Vec<FieldWrite>,
    name: &'static str,
    group: FieldGroup,
    value: Option<bool>,
) {
    if let Some(v) = value {
        fields.push(FieldWrite {
            name,
            group,
            value: Some(if v { "true" } else { "false" }.to_owned()),
        });
    }
}

/// Push an optional i64 as a decimal string.
pub fn push_opt_i64(
    fields: &mut Vec<FieldWrite>,
    name: &'static str,
    group: FieldGroup,
    value: Option<i64>,
) {
    if let Some(v) = value {
        fields.push(FieldWrite {
            name,
            group,
            value: Some(v.to_string()),
        });
    }
}

/// Push an optional timestamp as its RFC3339 string.
pub fn push_opt_ts(
    fields: &mut Vec<FieldWrite>,
    name: &'static str,
    group: FieldGroup,
    value: Option<Timestamp>,
) {
    if let Some(v) = value {
        fields.push(FieldWrite {
            name,
            group,
            value: Some(v.to_string()),
        });
    }
}

/// Parse an optional f64 from a field write value.
pub fn parse_opt_f64(value: Option<&String>) -> Result<Option<f64>, std::num::ParseFloatError> {
    value.map(|s| s.parse()).transpose()
}

/// Parse an optional bool from `"true"` / `"1"`.
#[must_use]
pub fn parse_opt_bool(value: Option<&String>) -> Option<bool> {
    value.map(|s| s == "true" || s == "1")
}
