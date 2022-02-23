#[macro_export]
macro_rules! as_variant_ref {
    ($value:expr, $pattern:path) => {
        match &$value {
            $pattern(v) => Some(v),
            _ => None,
        }
    };
}

#[macro_export]
macro_rules! as_variant {
    ($value:expr, $pattern:path) => {
        match $value {
            $pattern(v) => Some(v),
            _ => None,
        }
    };
}
