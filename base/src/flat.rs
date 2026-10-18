use serde::{Deserialize, Serialize};
use std::fmt;

/// A validated, normalized identifier for a flat unit in a housing society.
///
/// Ensures consistency across the codebase:
/// - `wing` is always stored UPPERCASE and trimmed (e.g., "A", "B")
/// - `flat_number` is always trimmed and zero-padded to 4 digits (e.g., "0203")
///
/// # Examples
/// ```
/// use base::flat::FlatIdentifier;
/// let flat = FlatIdentifier::new("a", "203").unwrap();
/// assert_eq!(flat.wing(), "A");
/// assert_eq!(flat.flat_number(), "0203");
/// assert_eq!(flat.to_string(), "A-0203");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FlatIdentifier {
    wing: String,
    flat_number: String,
}

impl FlatIdentifier {
    /// Creates a new `FlatIdentifier` from raw user input, normalizing the values.
    ///
    /// - Wing is trimmed and uppercased.
    /// - Flat number is trimmed and left-padded with zeros to 4 digits.
    /// - Returns `Err` if either field is empty after trimming, or flat_number is not numeric.
    pub fn new(wing: &str, flat_number: &str) -> Result<Self, String> {
        let wing = wing.trim().to_uppercase();
        let flat_number = flat_number.trim().to_string();

        if wing.is_empty() {
            return Err("Wing cannot be empty.".to_string());
        }
        if !wing.chars().all(|c| c.is_ascii_alphabetic()) {
            return Err(format!(
                "Wing '{}' must contain only letters (A-Z), not digits.",
                wing
            ));
        }
        if flat_number.is_empty() {
            return Err("Flat number cannot be empty.".to_string());
        }

        // Validate that flat_number contains only digits
        if !flat_number.chars().all(|c| c.is_ascii_digit()) {
            return Err(format!(
                "Flat number '{}' must contain only digits.",
                flat_number
            ));
        }

        // Zero-pad to 4 digits for consistency (e.g., "203" -> "0203")
        let normalized_flat_number = format!("{:0>4}", flat_number);

        Ok(Self {
            wing,
            flat_number: normalized_flat_number,
        })
    }

    /// Creates a `FlatIdentifier` from already-normalized values without re-validation.
    /// Use this only when reading back from the database where values are known to be clean.
    pub fn from_normalized(wing: String, flat_number: String) -> Self {
        Self { wing, flat_number }
    }

    /// Splits a concatenated code like `"A0301"` into wing+number and normalizes it.
    pub fn parse_concatenated(code: &str) -> Result<Self, String> {
        let code = code.trim();
        let split_at = code
            .find(|c: char| c.is_ascii_digit())
            .ok_or_else(|| format!("'{}' has no flat number", code))?;
        Self::new(&code[..split_at], &code[split_at..])
    }

    /// Returns the normalized wing (e.g., "A").
    pub fn wing(&self) -> &str {
        &self.wing
    }

    /// Returns the normalized flat number (e.g., "0203").
    pub fn flat_number(&self) -> &str {
        &self.flat_number
    }
}

impl fmt::Display for FlatIdentifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}", self.wing, self.flat_number)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalization() {
        let flat = FlatIdentifier::new("  a  ", "203").unwrap();
        assert_eq!(flat.wing(), "A");
        assert_eq!(flat.flat_number(), "0203");
    }

    #[test]
    fn test_already_padded() {
        let flat = FlatIdentifier::new("B", "0101").unwrap();
        assert_eq!(flat.flat_number(), "0101");
    }

    #[test]
    fn test_display() {
        let flat = FlatIdentifier::new("C", "42").unwrap();
        assert_eq!(flat.to_string(), "C-0042");
    }

    #[test]
    fn test_empty_wing_rejected() {
        assert!(FlatIdentifier::new("", "101").is_err());
    }

    #[test]
    fn test_empty_flat_number_rejected() {
        assert!(FlatIdentifier::new("A", "").is_err());
    }

    #[test]
    fn test_non_numeric_flat_number_rejected() {
        assert!(FlatIdentifier::new("A", "12B").is_err());
    }

    #[test]
    fn test_equality() {
        let a = FlatIdentifier::new("a", "203").unwrap();
        let b = FlatIdentifier::new("A", "0203").unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn test_numeric_wing_rejected() {
        assert!(FlatIdentifier::new("3", "0301").is_err());
        assert!(FlatIdentifier::new("K3", "0303").is_err());
    }

    #[test]
    fn test_parse_concatenated_splits_wing_from_number() {
        let flat = FlatIdentifier::parse_concatenated("A0301").unwrap();
        assert_eq!(flat.wing(), "A");
        assert_eq!(flat.flat_number(), "0301");
    }

    #[test]
    fn test_parse_concatenated_lowercases_and_pads() {
        let flat = FlatIdentifier::parse_concatenated("k2002").unwrap();
        assert_eq!(flat.wing(), "K");
        assert_eq!(flat.flat_number(), "2002");

        let flat = FlatIdentifier::parse_concatenated(" a301 ").unwrap();
        assert_eq!(flat.wing(), "A");
        assert_eq!(flat.flat_number(), "0301");
    }

    #[test]
    fn test_parse_concatenated_rejects_malformed_codes() {
        assert!(FlatIdentifier::parse_concatenated("").is_err());
        assert!(
            FlatIdentifier::parse_concatenated("A").is_err(),
            "no digits at all"
        );
        assert!(
            FlatIdentifier::parse_concatenated("1234").is_err(),
            "no wing letters at all"
        );
    }
}
