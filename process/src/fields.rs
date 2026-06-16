use uuid::Uuid;

/// Validates the input values against the field's type.
pub fn validate_field(
    field_type: &str,
    text_value: Option<&str>,
    document_id: Option<Uuid>,
    is_optional: bool,
) -> Result<(), String> {
    if !is_optional && !is_field_filled(field_type, text_value, document_id) {
        return Err("This field is mandatory and cannot be empty".to_string());
    }

    match field_type {
        "document" | "text" => Ok(()),
        _ => Err(format!("Unknown field type: {}", field_type)),
    }
}

/// Checks if the field is considered complete (filled).
pub fn is_field_filled(
    field_type: &str,
    text_value: Option<&str>,
    document_id: Option<Uuid>,
) -> bool {
    match field_type {
        "document" => document_id.is_some(),
        "text" => match text_value {
            Some(val) => !val.trim().is_empty(),
            None => false,
        },
        _ => false,
    }
}
