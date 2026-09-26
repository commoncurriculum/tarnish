//! `json!`, serde_json's macro, building these values, and `object!`.

#[macro_export]
#[doc(hidden)]
macro_rules! json_internal {
    (@array [$($elems:expr,)*]) => {
        vec![$($elems,)*]
    };
    (@array [$($elems:expr),*]) => {
        vec![$($elems),*]
    };
    (@array [$($elems:expr,)*] null $($rest:tt)*) => {
        $crate::json_internal!(@array [$($elems,)* $crate::json_internal!(null)] $($rest)*)
    };
    (@array [$($elems:expr,)*] true $($rest:tt)*) => {
        $crate::json_internal!(@array [$($elems,)* $crate::json_internal!(true)] $($rest)*)
    };
    (@array [$($elems:expr,)*] false $($rest:tt)*) => {
        $crate::json_internal!(@array [$($elems,)* $crate::json_internal!(false)] $($rest)*)
    };
    (@array [$($elems:expr,)*] [$($array:tt)*] $($rest:tt)*) => {
        $crate::json_internal!(@array [$($elems,)* $crate::json_internal!([$($array)*])] $($rest)*)
    };
    (@array [$($elems:expr,)*] {$($map:tt)*} $($rest:tt)*) => {
        $crate::json_internal!(@array [$($elems,)* $crate::json_internal!({$($map)*})] $($rest)*)
    };
    (@array [$($elems:expr,)*] $next:expr, $($rest:tt)*) => {
        $crate::json_internal!(@array [$($elems,)* $crate::json_internal!($next),] $($rest)*)
    };
    (@array [$($elems:expr,)*] $last:expr) => {
        $crate::json_internal!(@array [$($elems,)* $crate::json_internal!($last)])
    };
    (@array [$($elems:expr),*] , $($rest:tt)*) => {
        $crate::json_internal!(@array [$($elems,)*] $($rest)*)
    };
    // An object literal's keys are distinct, so each is pushed without a search.
    (@object $object:ident () () ()) => {};
    (@object $object:ident [$($key:tt)+] ($value:expr) , $($rest:tt)*) => {
        $object.push(($($key)+).into(), $value);
        $crate::json_internal!(@object $object () ($($rest)*) ($($rest)*));
    };
    (@object $object:ident [$($key:tt)+] ($value:expr)) => {
        $object.push(($($key)+).into(), $value);
    };
    (@object $object:ident ($($key:tt)+) (: null $($rest:tt)*) $copy:tt) => {
        $crate::json_internal!(@object $object [$($key)+] ($crate::json_internal!(null)) $($rest)*);
    };
    (@object $object:ident ($($key:tt)+) (: true $($rest:tt)*) $copy:tt) => {
        $crate::json_internal!(@object $object [$($key)+] ($crate::json_internal!(true)) $($rest)*);
    };
    (@object $object:ident ($($key:tt)+) (: false $($rest:tt)*) $copy:tt) => {
        $crate::json_internal!(@object $object [$($key)+] ($crate::json_internal!(false)) $($rest)*);
    };
    (@object $object:ident ($($key:tt)+) (: [$($array:tt)*] $($rest:tt)*) $copy:tt) => {
        $crate::json_internal!(@object $object [$($key)+] ($crate::json_internal!([$($array)*])) $($rest)*);
    };
    (@object $object:ident ($($key:tt)+) (: {$($map:tt)*} $($rest:tt)*) $copy:tt) => {
        $crate::json_internal!(@object $object [$($key)+] ($crate::json_internal!({$($map)*})) $($rest)*);
    };
    (@object $object:ident ($($key:tt)+) (: $value:expr , $($rest:tt)*) $copy:tt) => {
        $crate::json_internal!(@object $object [$($key)+] ($crate::json_internal!($value)) , $($rest)*);
    };
    (@object $object:ident ($($key:tt)+) (: $value:expr) $copy:tt) => {
        $crate::json_internal!(@object $object [$($key)+] ($crate::json_internal!($value)));
    };
    (@object $object:ident () (($key:expr) : $($rest:tt)*) $copy:tt) => {
        $crate::json_internal!(@object $object ($key) (: $($rest)*) (: $($rest)*));
    };
    (@object $object:ident ($($key:tt)*) ($tt:tt $($rest:tt)*) $copy:tt) => {
        $crate::json_internal!(@object $object ($($key)* $tt) ($($rest)*) ($($rest)*));
    };
    // The commas between an object literal's entries, which room for its entries is one more
    // than. A comma inside a value's brackets is in a token tree of its own.
    (@commas) => {
        0usize
    };
    (@commas , $($rest:tt)*) => {
        1usize + $crate::json_internal!(@commas $($rest)*)
    };
    (@commas $first:tt $($rest:tt)*) => {
        $crate::json_internal!(@commas $($rest)*)
    };
    (null) => {
        $crate::json::Value::Null
    };
    (true) => {
        $crate::json::Value::Bool(true)
    };
    (false) => {
        $crate::json::Value::Bool(false)
    };
    ([]) => {
        $crate::json::Value::Array(vec![])
    };
    ([ $($tt:tt)+ ]) => {
        $crate::json::Value::Array($crate::json_internal!(@array [] $($tt)+))
    };
    ({}) => {
        $crate::json::Value::Object($crate::json::Map::new())
    };
    ({ $($tt:tt)+ }) => {
        $crate::json::Value::Object($crate::json_object!({ $($tt)+ }))
    };
    ($other:expr) => {
        $crate::json::IntoValue::into_value($other)
    };
}

pub use crate::json_internal as json;

/// `json!({...})` as the object's `Map`.
#[macro_export]
macro_rules! json_object {
    ({}) => {
        $crate::json::Map::new()
    };
    ({ $($tt:tt)+ }) => {{
        let mut object =
            $crate::json::Map::with_capacity(1 + $crate::json_internal!(@commas $($tt)+));
        $crate::json_internal!(@object object () ($($tt)+) ($($tt)+));
        object
    }};
}

pub use crate::json_object as object;
