use serde_json::{Number, Value};

pub(crate) fn id_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) if !text.is_empty() => Some(text.clone()),
        Value::Number(number) => Some(integer_text(number)),
        _ => None,
    }
}

fn integer_text(number: &Number) -> String {
    match number.as_f64() {
        Some(float) if number.is_f64() && float.fract() == 0.0 && float >= 0.0 => {
            format!("{float:.0}")
        }
        _ => number.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn float_ids_read_as_integers() {
        assert_eq!(
            id_text(&json!(1783430602049.0)).as_deref(),
            Some("1783430602049")
        );
        assert_eq!(
            id_text(&json!(1783430602049u64)).as_deref(),
            Some("1783430602049")
        );
        assert_eq!(
            id_text(&json!("1783430602049")).as_deref(),
            Some("1783430602049")
        );
        assert_eq!(id_text(&json!("")), None);
    }
}
