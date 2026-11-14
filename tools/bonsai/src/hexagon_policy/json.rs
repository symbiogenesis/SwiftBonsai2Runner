//! Strict bounded JSON before object maps can discard ambiguous evidence.

use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Number, Value};
use std::collections::BTreeSet;
use std::fmt;

struct Seed(usize, usize);

impl<'de> DeserializeSeed<'de> for Seed {
    type Value = Value;
    fn deserialize<D: de::Deserializer<'de>>(self, deserializer: D) -> Result<Value, D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for Seed {
    type Value = Value;
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("bounded Hexagon JSON")
    }
    fn visit_bool<E: de::Error>(self, value: bool) -> Result<Value, E> {
        Ok(Value::Bool(value))
    }
    fn visit_i64<E: de::Error>(self, value: i64) -> Result<Value, E> {
        Ok(value.into())
    }
    fn visit_u64<E: de::Error>(self, value: u64) -> Result<Value, E> {
        Ok(value.into())
    }
    fn visit_f64<E: de::Error>(self, value: f64) -> Result<Value, E> {
        Number::from_f64(value)
            .map(Value::Number)
            .ok_or_else(|| E::custom("Nonfinite Hexagon JSON number."))
    }
    fn visit_str<E: de::Error>(self, value: &str) -> Result<Value, E> {
        Ok(value.into())
    }
    fn visit_string<E: de::Error>(self, value: String) -> Result<Value, E> {
        Ok(value.into())
    }
    fn visit_unit<E: de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Value, A::Error> {
        if self.0 >= self.1 {
            return Err(de::Error::custom("Hexagon JSON exceeds its depth bound."));
        }
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element_seed(Self(self.0 + 1, self.1))? {
            values.push(value);
        }
        Ok(Value::Array(values))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut object: A) -> Result<Value, A::Error> {
        if self.0 >= self.1 {
            return Err(de::Error::custom("Hexagon JSON exceeds its depth bound."));
        }
        let mut names = BTreeSet::new();
        let mut values = Map::new();
        while let Some(name) = object.next_key::<String>()? {
            // The native ABI uses ASCII names. Reject non-ASCII aliases rather
            // than depending on a caller's Unicode/case-folding implementation.
            if !name.is_ascii() || !names.insert(name.to_ascii_lowercase()) {
                return Err(de::Error::custom(
                    "Hexagon JSON contains ambiguous object names.",
                ));
            }
            values.insert(name, object.next_value_seed(Self(self.0 + 1, self.1))?);
        }
        Ok(Value::Object(values))
    }
}

pub(super) fn parse(text: &str) -> crate::Result<Value> {
    parse_bounded(text, 65536, 32)
}

pub(super) fn parse_bounded(text: &str, maximum: usize, depth: usize) -> crate::Result<Value> {
    if text.len() > maximum {
        return Err("Hexagon JSON exceeds its byte bound.".into());
    }
    let mut decoder = serde_json::Deserializer::from_str(text);
    let value = Seed(0, depth)
        .deserialize(&mut decoder)
        .map_err(|error| error.to_string())?;
    decoder.end().map_err(|error| error.to_string())?;
    if !value.is_object() {
        return Err("Hexagon JSON must contain one object.".into());
    }
    Ok(value)
}
