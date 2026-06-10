use uuid::Uuid;

/// Validates the input values against the field's type.
pub fn validate_field(
    field_type: &str,
    text_value: Option<&str>,
    document_id: Option<Uuid>,
) -> Result<(), String> {
    match field_type {
        "document" => {
            if document_id.is_none() {
                return Err("Document ID is required".to_string());
            }
            Ok(())
        }
        "text" => match text_value {
            Some(val) if !val.trim().is_empty() => Ok(()),
            _ => Err("Text value is required and cannot be empty".to_string()),
        },
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
