use crate::FieldType;
use chrono::NaiveDate;
use rust_decimal::Decimal;
use std::str::FromStr;
use uuid::Uuid;

/// Validates the input values against the field's type.
pub fn validate_field(
    field_type: FieldType,
    text_value: Option<&str>,
    document_id: Option<Uuid>,
    is_optional: bool,
    options: Option<&[String]>,
) -> Result<(), String> {
    if !is_optional && !is_field_filled(field_type, text_value, document_id) {
        return Err("This field is mandatory and cannot be empty".to_string());
    }

    if let Some(val) = text_value.filter(|v| !v.trim().is_empty()) {
        validate_field_value(field_type, val, options)?;
    }

    Ok(())
}

fn validate_field_value(
    field_type: FieldType,
    val: &str,
    options: Option<&[String]>,
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
    }
}

/// Checks if the field is considered complete (filled).
pub fn is_field_filled(
    field_type: FieldType,
    text_value: Option<&str>,
    document_id: Option<Uuid>,
) -> bool {
    match field_type {
        FieldType::File => document_id.is_some(),
        FieldType::Text
        | FieldType::Int
        | FieldType::Decimal
        | FieldType::Date
        | FieldType::Bool
        | FieldType::Enum => text_value.map(|v| !v.trim().is_empty()).unwrap_or(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn int_rejects_non_numeric() {
        assert!(validate_field(FieldType::Int, Some("42"), None, false, None).is_ok());
        assert!(validate_field(FieldType::Int, Some("4.2"), None, false, None).is_err());
        assert!(validate_field(FieldType::Int, Some("abc"), None, false, None).is_err());
    }

    #[test]
    fn decimal_accepts_fractional_values() {
        assert!(validate_field(FieldType::Decimal, Some("1234.56"), None, false, None).is_ok());
        assert!(validate_field(FieldType::Decimal, Some("abc"), None, false, None).is_err());
    }

    #[test]
    fn date_requires_iso_format() {
        assert!(validate_field(FieldType::Date, Some("2026-08-21"), None, false, None).is_ok());
        assert!(validate_field(FieldType::Date, Some("21/08/2026"), None, false, None).is_err());
        assert!(validate_field(FieldType::Date, Some("not-a-date"), None, false, None).is_err());
    }

    #[test]
    fn bool_only_accepts_true_or_false() {
        assert!(validate_field(FieldType::Bool, Some("true"), None, false, None).is_ok());
        assert!(validate_field(FieldType::Bool, Some("false"), None, false, None).is_ok());
        assert!(validate_field(FieldType::Bool, Some("yes"), None, false, None).is_err());
    }

    #[test]
    fn enum_rejects_values_outside_the_configured_options() {
        let options = vec![
            "Owner".to_string(),
            "Tenant".to_string(),
            "Agent".to_string(),
        ];
        assert!(
            validate_field(FieldType::Enum, Some("Tenant"), None, false, Some(&options)).is_ok()
        );
        assert!(validate_field(
            FieldType::Enum,
            Some("Squatter"),
            None,
            false,
            Some(&options)
        )
        .is_err());
        assert!(validate_field(FieldType::Enum, Some("Tenant"), None, false, None).is_err());
    }

    #[test]
    fn file_field_is_filled_only_by_a_document_id_never_by_text() {
        assert!(!is_field_filled(FieldType::File, Some("some text"), None));
        assert!(is_field_filled(FieldType::File, None, Some(Uuid::new_v4())));
    }

    #[test]
    fn optional_empty_field_of_any_type_passes_validation() {
        assert!(validate_field(FieldType::Int, None, None, true, None).is_ok());
        assert!(validate_field(FieldType::Date, Some(""), None, true, None).is_ok());
    }
}
