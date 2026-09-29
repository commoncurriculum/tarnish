//! Conversions into values, which `json!` interpolates through.

use std::borrow::Cow;

use super::{Map, Number, Value};

/// A value `json!` interpolates by reference, which it copies.
pub trait ToValue {
    fn to_value(&self) -> Value;
}

impl ToValue for Value {
    fn to_value(&self) -> Value {
        self.clone()
    }
}

impl ToValue for Map {
    fn to_value(&self) -> Value {
        Value::Object(self.clone())
    }
}

impl ToValue for Number {
    fn to_value(&self) -> Value {
        Value::Number(self.clone())
    }
}

impl ToValue for str {
    fn to_value(&self) -> Value {
        Value::String(self.to_owned())
    }
}

impl ToValue for String {
    fn to_value(&self) -> Value {
        Value::String(self.clone())
    }
}

impl ToValue for Cow<'_, str> {
    fn to_value(&self) -> Value {
        Value::String(self.to_string())
    }
}

impl ToValue for char {
    fn to_value(&self) -> Value {
        Value::String(self.to_string())
    }
}

impl ToValue for bool {
    fn to_value(&self) -> Value {
        Value::Bool(*self)
    }
}

impl ToValue for f64 {
    fn to_value(&self) -> Value {
        Number::from_f64(*self).map_or(Value::Null, Value::Number)
    }
}

macro_rules! integer_to_value {
    ($($integer:ty),*) => {
        $(
            impl ToValue for $integer {
                fn to_value(&self) -> Value {
                    Value::Number(Number::from(*self))
                }
            }
        )*
    };
}

integer_to_value!(i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);

impl<T: ToValue> ToValue for Option<T> {
    fn to_value(&self) -> Value {
        self.as_ref().map_or(Value::Null, ToValue::to_value)
    }
}

impl<T: ToValue> ToValue for [T] {
    fn to_value(&self) -> Value {
        Value::Array(self.iter().map(ToValue::to_value).collect())
    }
}

impl<T: ToValue> ToValue for Vec<T> {
    fn to_value(&self) -> Value {
        self.as_slice().to_value()
    }
}

impl<T: ToValue + ?Sized> ToValue for &T {
    fn to_value(&self) -> Value {
        (**self).to_value()
    }
}

impl<T: ToValue + ?Sized> ToValue for &mut T {
    fn to_value(&self) -> Value {
        (**self).to_value()
    }
}

/// A value `json!` interpolates, which it takes over; a reference it copies.
pub trait IntoValue {
    fn into_value(self) -> Value;
}

impl<T: ToValue + ?Sized> IntoValue for &T {
    fn into_value(self) -> Value {
        self.to_value()
    }
}

impl IntoValue for Value {
    fn into_value(self) -> Value {
        self
    }
}

impl IntoValue for Map {
    fn into_value(self) -> Value {
        Value::Object(self)
    }
}

impl IntoValue for String {
    fn into_value(self) -> Value {
        Value::String(self)
    }
}

impl IntoValue for Number {
    fn into_value(self) -> Value {
        Value::Number(self)
    }
}

impl IntoValue for Cow<'_, str> {
    fn into_value(self) -> Value {
        Value::String(self.into_owned())
    }
}

impl<T: IntoValue> IntoValue for Vec<T> {
    fn into_value(self) -> Value {
        Value::Array(self.into_iter().map(IntoValue::into_value).collect())
    }
}

impl<T: IntoValue> IntoValue for Option<T> {
    fn into_value(self) -> Value {
        self.map_or(Value::Null, IntoValue::into_value)
    }
}

macro_rules! copy_into_value {
    ($($copy:ty),*) => {
        $(
            impl IntoValue for $copy {
                fn into_value(self) -> Value {
                    self.to_value()
                }
            }
        )*
    };
}

copy_into_value!(
    bool, char, f64, i8, i16, i32, i64, isize, u8, u16, u32, u64, usize
);

impl From<Map> for Value {
    fn from(map: Map) -> Value {
        Value::Object(map)
    }
}

impl From<Vec<Value>> for Value {
    fn from(items: Vec<Value>) -> Value {
        Value::Array(items)
    }
}

impl From<String> for Value {
    fn from(text: String) -> Value {
        Value::String(text)
    }
}

impl From<&str> for Value {
    fn from(text: &str) -> Value {
        Value::String(text.to_owned())
    }
}

impl From<bool> for Value {
    fn from(value: bool) -> Value {
        Value::Bool(value)
    }
}

impl From<Number> for Value {
    fn from(number: Number) -> Value {
        Value::Number(number)
    }
}

impl From<f64> for Value {
    fn from(value: f64) -> Value {
        value.to_value()
    }
}

impl From<i64> for Value {
    fn from(value: i64) -> Value {
        Value::Number(value.into())
    }
}

impl From<u64> for Value {
    fn from(value: u64) -> Value {
        Value::Number(value.into())
    }
}

impl From<usize> for Value {
    fn from(value: usize) -> Value {
        Value::Number(value.into())
    }
}

impl From<i32> for Value {
    fn from(value: i32) -> Value {
        Value::Number(value.into())
    }
}
