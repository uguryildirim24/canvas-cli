//! Serde helpers for Canvas JSON (IDs, timestamps, dates, URLs, Supplied).

use std::cell::RefCell;
use std::fmt;

use jiff::Timestamp;
use jiff::civil::Date;
use reqwest::Url;
use serde::de::{self, Deserialize, Deserializer, Visitor};
use serde::{Serialize, Serializer};

thread_local! {
    static ORIGIN: RefCell<Option<Url>> = const { RefCell::new(None) };
}

/// Run `f` with `origin` as the base for relative URL deserialization.
pub fn with_origin<R>(origin: &Url, f: impl FnOnce() -> R) -> R {
    ORIGIN.with(|slot| {
        struct RestoreOrigin<'a>(&'a RefCell<Option<Url>>, Option<Url>);
        impl Drop for RestoreOrigin<'_> {
            fn drop(&mut self) {
                self.0.replace(self.1.take());
            }
        }
        let _restore = RestoreOrigin(slot, slot.replace(Some(origin.clone())));
        f()
    })
}

fn current_origin() -> Option<Url> {
    ORIGIN.with(|slot| slot.borrow().clone())
}

/// Three-state field for `field_obs` tracked values: absent / JSON null / value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Supplied<T> {
    /// Field was missing from the JSON object.
    #[default]
    Absent,
    /// Field was present as JSON `null`.
    Null,
    /// Field was present with a value.
    Value(T),
}

impl<T> Supplied<T> {
    /// Returns `true` when the field was present (null or value).
    #[must_use]
    pub const fn is_supplied(&self) -> bool {
        !matches!(self, Self::Absent)
    }

    /// Borrow the inner value when present as `Value`.
    #[must_use]
    pub const fn as_value(&self) -> Option<&T> {
        match self {
            Self::Value(v) => Some(v),
            Self::Absent | Self::Null => None,
        }
    }

    /// Convert to `Option`, collapsing `Absent` and `Null` to `None`.
    #[must_use]
    pub fn into_option(self) -> Option<T> {
        match self {
            Self::Value(v) => Some(v),
            Self::Absent | Self::Null => None,
        }
    }
}

impl<T: Serialize> Serialize for Supplied<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Absent | Self::Null => serializer.serialize_none(),
            Self::Value(v) => v.serialize(serializer),
        }
    }
}

/// `deserialize_with` helper for [`Supplied`].
///
/// Pair with `#[serde(default, deserialize_with = "…")]`. Absent fields use
/// `Default` (`Absent`); JSON `null` becomes `Null`; other values become `Value`.
pub fn deserialize_supplied<'de, T, D>(deserializer: D) -> Result<Supplied<T>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    match Option::<T>::deserialize(deserializer)? {
        Some(value) => Ok(Supplied::Value(value)),
        None => Ok(Supplied::Null),
    }
}

/// Deserialize an `i64` ID from a JSON number or string.
pub fn deserialize_id<'de, D: Deserializer<'de>>(deserializer: D) -> Result<i64, D::Error> {
    struct IdVisitor;

    impl Visitor<'_> for IdVisitor {
        type Value = i64;

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("an i64 id as number or string")
        }

        fn visit_i64<E: de::Error>(self, v: i64) -> Result<i64, E> {
            Ok(v)
        }

        fn visit_u64<E: de::Error>(self, v: u64) -> Result<i64, E> {
            i64::try_from(v).map_err(E::custom)
        }

        fn visit_str<E: de::Error>(self, v: &str) -> Result<i64, E> {
            v.parse().map_err(E::custom)
        }
    }

    deserializer.deserialize_any(IdVisitor)
}

/// Deserialize an optional `i64` ID from a JSON number, string, or null.
pub fn deserialize_opt_id<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<i64>, D::Error> {
    struct OptIdVisitor;

    impl Visitor<'_> for OptIdVisitor {
        type Value = Option<i64>;

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("an optional i64 id as number or string")
        }

        fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
            Ok(Some(v))
        }

        fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
            i64::try_from(v).map(Some).map_err(E::custom)
        }

        fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
            if v.is_empty() {
                return Ok(None);
            }
            v.parse().map(Some).map_err(E::custom)
        }
    }

    deserializer.deserialize_any(OptIdVisitor)
}

/// Deserialize a [`Timestamp`], accepting offsets (normalized to UTC).
#[allow(dead_code)] // used by models that need required timestamps
pub fn deserialize_timestamp<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Timestamp, D::Error> {
    let raw = String::deserialize(deserializer)?;
    raw.parse().map_err(de::Error::custom)
}

/// Deserialize an optional [`Timestamp`].
pub fn deserialize_opt_timestamp<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Timestamp>, D::Error> {
    let Some(raw) = Option::<String>::deserialize(deserializer)? else {
        return Ok(None);
    };
    if raw.is_empty() {
        return Ok(None);
    }
    raw.parse().map(Some).map_err(de::Error::custom)
}

/// Deserialize a civil [`Date`] (`YYYY-MM-DD`).
#[allow(dead_code)]
pub fn deserialize_date<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Date, D::Error> {
    let raw = String::deserialize(deserializer)?;
    raw.parse().map_err(de::Error::custom)
}

/// Deserialize an optional civil [`Date`].
pub fn deserialize_opt_date<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Date>, D::Error> {
    let Some(raw) = Option::<String>::deserialize(deserializer)? else {
        return Ok(None);
    };
    if raw.is_empty() {
        return Ok(None);
    }
    raw.parse().map(Some).map_err(de::Error::custom)
}

fn parse_url(raw: &str) -> Result<Url, String> {
    if let Ok(url) = Url::parse(raw) {
        return Ok(url);
    }
    let origin = current_origin()
        .ok_or_else(|| "relative URL without an origin (use serde_util::with_origin)".to_owned())?;
    origin
        .join(raw)
        .map_err(|_| "invalid relative URL".to_owned())
}

/// Deserialize a [`Url`], resolving relative references against the thread-local origin.
#[allow(dead_code)]
pub fn deserialize_url<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Url, D::Error> {
    let raw = String::deserialize(deserializer)?;
    parse_url(&raw).map_err(de::Error::custom)
}

/// Deserialize an optional [`Url`].
pub fn deserialize_opt_url<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Url>, D::Error> {
    let Some(raw) = Option::<String>::deserialize(deserializer)? else {
        return Ok(None);
    };
    if raw.is_empty() {
        return Ok(None);
    }
    parse_url(&raw).map(Some).map_err(de::Error::custom)
}

/// Serialize a [`Url`] as a string.
#[allow(dead_code)]
pub fn serialize_url<S: Serializer>(url: &Url, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(url.as_str())
}

/// Serialize an optional [`Url`] as a string or null.
#[allow(dead_code)]
pub fn serialize_opt_url<S: Serializer>(
    url: &Option<Url>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match url {
        Some(u) => serializer.serialize_str(u.as_str()),
        None => serializer.serialize_none(),
    }
}

/// Deserialize a tracked URL, preserving null and resolving relative values.
pub fn deserialize_supplied_url<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Supplied<Url>, D::Error> {
    #[derive(serde::Deserialize)]
    struct RelativeUrl(#[serde(deserialize_with = "deserialize_url")] Url);
    Ok(match Option::<RelativeUrl>::deserialize(deserializer)? {
        Some(value) => Supplied::Value(value.0),
        None => Supplied::Null,
    })
}

/// Deserialize an optional vector of IDs, accepting strings and numbers per element.
pub fn deserialize_opt_ids<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Vec<i64>>, D::Error> {
    #[derive(serde::Deserialize)]
    struct Id(#[serde(deserialize_with = "deserialize_id")] i64);
    Ok(Option::<Vec<Id>>::deserialize(deserializer)?
        .map(|ids| ids.into_iter().map(|id| id.0).collect()))
}
