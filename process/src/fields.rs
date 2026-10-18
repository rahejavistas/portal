use crate::FieldType;
use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::Deserialize;
use std::collections::HashMap;
use std::str::FromStr;
use uuid::Uuid;

#[derive(Deserialize)]
struct DeclarationValue {
    agreed: bool,
    #[serde(default)]
    blanks: HashMap<String, String>,
}

/// The `##TOKEN##` names a declaration's text needs a blank for — `[A-Z]+` only.
fn declaration_blank_tokens(text: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("##") {
        rest = &rest[start + 2..];
        let Some(end) = rest.find("##") else { break };
        let candidate = &rest[..end];
        if !candidate.is_empty() && candidate.bytes().all(|b| b.is_ascii_uppercase()) {
            tokens.push(candidate);
        }
        rest = &rest[end + 2..];
    }
    tokens
}

fn declaration_text(requirements: Option<&serde_json::Value>) -> Option<&str> {
    requirements?.get("declaration_text")?.as_str()
}

/// Agreed, and every blank the declaration's text calls for is non-empty.
fn declaration_is_satisfied(val: &str, requirements: Option<&serde_json::Value>) -> bool {
    let Ok(parsed) = serde_json::from_str::<DeclarationValue>(val) else {
        return false;
    };
    if !parsed.agreed {
        return false;
    }
    match declaration_text(requirements) {
        Some(text) => declaration_blank_tokens(text)
            .iter()
            .all(|t| parsed.blanks.get(*t).is_some_and(|v| !v.trim().is_empty())),
        None => true,
    }
}

/// Validates the input values against the field's type.
pub fn validate_field(
    field_type: FieldType,
    text_value: Option<&str>,
    document_id: Option<Uuid>,
    is_optional: bool,
    options: Option<&[String]>,
    requirements: Option<&serde_json::Value>,
) -> Result<(), String> {
    if !is_optional && !is_field_filled(field_type, text_value, document_id, requirements) {
        return Err("This field is mandatory and cannot be empty".to_string());
    }

    if let Some(val) = text_value.filter(|v| !v.trim().is_empty()) {
        validate_field_value(field_type, val, options, requirements)?;
    }

    Ok(())
}

fn validate_field_value(
    field_type: FieldType,
    val: &str,
    options: Option<&[String]>,
    requirements: Option<&serde_json::Value>,
) -> Result<(), String> {
    match field_type {
        FieldType::Text | FieldType::File => Ok(()),
        FieldType::Int => val
            .parse::<i64>()
            .map(|_| ())
            .map_err(|_| format!("'{val}' is not a whole number")),
        FieldType::Decimal => Decimal::from_str(val)
            .map(|_| ())
            .map_err(|_| format!("'{val}' is not a decimal number")),
        FieldType::Date => NaiveDate::parse_from_str(val, "%Y-%m-%d")
            .map(|_| ())
            .map_err(|_| format!("'{val}' is not a valid date (expected YYYY-MM-DD)")),
        FieldType::Bool => match val {
            "true" | "false" => Ok(()),
            _ => Err(format!("'{val}' must be true or false")),
        },
        FieldType::Enum => {
            let opts = options.unwrap_or(&[]);
            if opts.iter().any(|o| o == val) {
                Ok(())
            } else {
                Err(format!("'{val}' is not one of the allowed options"))
            }
        }
        FieldType::Declaration => {
            if declaration_is_satisfied(val, requirements) {
                Ok(())
            } else {
                Err("Declaration must be agreed to with every blank filled in".to_string())
            }
        }
        FieldType::NoDues => Decimal::from_str(val)
            .map(|_| ())
            .map_err(|_| format!("'{val}' is not a valid dues amount")),
    }
}

/// Checks if the field is considered complete (filled).
pub fn is_field_filled(
    field_type: FieldType,
    text_value: Option<&str>,
    document_id: Option<Uuid>,
    requirements: Option<&serde_json::Value>,
) -> bool {
    match field_type {
        FieldType::File => document_id.is_some(),
        FieldType::Text
        | FieldType::Int
        | FieldType::Decimal
        | FieldType::Date
        | FieldType::Bool
        | FieldType::Enum => text_value.map(|v| !v.trim().is_empty()).unwrap_or(false),
        FieldType::Declaration => text_value
            .map(|v| declaration_is_satisfied(v, requirements))
            .unwrap_or(false),
        FieldType::NoDues => text_value
            .and_then(|v| Decimal::from_str(v).ok())
            .map(|d| d.is_zero())
            .unwrap_or(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn int_rejects_non_numeric() {
        assert!(validate_field(FieldType::Int, Some("42"), None, false, None, None).is_ok());
        assert!(validate_field(FieldType::Int, Some("4.2"), None, false, None, None).is_err());
        assert!(validate_field(FieldType::Int, Some("abc"), None, false, None, None).is_err());
    }

    #[test]
    fn decimal_accepts_fractional_values() {
        assert!(
            validate_field(FieldType::Decimal, Some("1234.56"), None, false, None, None).is_ok()
        );
        assert!(validate_field(FieldType::Decimal, Some("abc"), None, false, None, None).is_err());
    }

    #[test]
    fn date_requires_iso_format() {
        assert!(
            validate_field(FieldType::Date, Some("2026-08-21"), None, false, None, None).is_ok()
        );
        assert!(
            validate_field(FieldType::Date, Some("21/08/2026"), None, false, None, None).is_err()
        );
        assert!(
            validate_field(FieldType::Date, Some("not-a-date"), None, false, None, None).is_err()
        );
    }

    #[test]
    fn bool_only_accepts_true_or_false() {
        assert!(validate_field(FieldType::Bool, Some("true"), None, false, None, None).is_ok());
        assert!(validate_field(FieldType::Bool, Some("false"), None, false, None, None).is_ok());
        assert!(validate_field(FieldType::Bool, Some("yes"), None, false, None, None).is_err());
    }

    #[test]
    fn enum_rejects_values_outside_the_configured_options() {
        let options = vec![
            "Owner".to_string(),
            "Tenant".to_string(),
            "Agent".to_string(),
        ];
        assert!(validate_field(
            FieldType::Enum,
            Some("Tenant"),
            None,
            false,
            Some(&options),
            None
        )
        .is_ok());
        assert!(validate_field(
            FieldType::Enum,
            Some("Squatter"),
            None,
            false,
            Some(&options),
            None
        )
        .is_err());
        assert!(validate_field(FieldType::Enum, Some("Tenant"), None, false, None, None).is_err());
    }

    #[test]
    fn file_field_is_filled_only_by_a_document_id_never_by_text() {
        assert!(!is_field_filled(
            FieldType::File,
            Some("some text"),
            None,
            None
        ));
        assert!(is_field_filled(
            FieldType::File,
            None,
            Some(Uuid::new_v4()),
            None
        ));
    }

    #[test]
    fn optional_empty_field_of_any_type_passes_validation() {
        assert!(validate_field(FieldType::Int, None, None, true, None, None).is_ok());
        assert!(validate_field(FieldType::Date, Some(""), None, true, None, None).is_ok());
    }

    #[test]
    fn declaration_requires_the_agreed_flag_set_true() {
        assert!(is_field_filled(
            FieldType::Declaration,
            Some(r#"{"agreed":true,"blanks":{"NAME":"A"}}"#),
            None,
            None
        ));
        assert!(!is_field_filled(
            FieldType::Declaration,
            Some(r#"{"agreed":false,"blanks":{}}"#),
            None,
            None
        ));
        assert!(!is_field_filled(
            FieldType::Declaration,
            Some("not json"),
            None,
            None
        ));
        assert!(!is_field_filled(FieldType::Declaration, None, None, None));
    }

    #[test]
    fn declaration_rejects_agreed_true_with_an_empty_or_missing_blank() {
        let requirements = serde_json::json!({ "declaration_text": "I, ##NAME##, of ##FLAT##." });

        assert!(is_field_filled(
            FieldType::Declaration,
            Some(r#"{"agreed":true,"blanks":{"NAME":"Jane","FLAT":"A-203"}}"#),
            None,
            Some(&requirements)
        ));
        assert!(!is_field_filled(
            FieldType::Declaration,
            Some(r#"{"agreed":true,"blanks":{"NAME":"Jane","FLAT":""}}"#),
            None,
            Some(&requirements)
        ));
        assert!(!is_field_filled(
            FieldType::Declaration,
            Some(r#"{"agreed":true,"blanks":{"NAME":"Jane"}}"#),
            None,
            Some(&requirements)
        ));
        assert!(validate_field(
            FieldType::Declaration,
            Some(r#"{"agreed":true,"blanks":{"NAME":"Jane"}}"#),
            None,
            false,
            None,
            Some(&requirements)
        )
        .is_err());
    }

    #[test]
    fn no_dues_is_filled_only_when_the_amount_is_exactly_zero() {
        assert!(is_field_filled(FieldType::NoDues, Some("0"), None, None));
        assert!(is_field_filled(FieldType::NoDues, Some("0.00"), None, None));
        assert!(!is_field_filled(
            FieldType::NoDues,
            Some("500.00"),
            None,
            None
        ));
        assert!(!is_field_filled(FieldType::NoDues, None, None, None));
    }
}
