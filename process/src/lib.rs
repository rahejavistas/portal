pub mod db;
pub mod fields;
pub mod notification;
pub mod state_machine;
pub mod template_store;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProcessFieldTemplate {
    pub field_id: i32,
    pub name: String,
    pub r#type: String, // "document" or "text"
    pub is_optional: bool,
    pub filled_by: String, // "user", "admin"
    pub requirements: Option<serde_json::Value>,
    /// Document fields only. Blocks step completion until the field's document has a
    /// signature requirement attached and signed.
    #[serde(default)]
    pub requires_signature: bool,
    /// Designations eligible to sign this field, in required signing order. Enforced
    /// both when a reviewer attaches signers and again at the moment of signing.
    #[serde(default)]
    pub required_signs_from_designations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProcessStepTemplate {
    pub step_id: i32,
    pub name: String,
    pub description: Option<String>,
    pub is_optional: bool,
    pub is_review_required: bool,
    pub fields: Option<Vec<ProcessFieldTemplate>>,
    pub substeps: Option<Vec<ProcessStepTemplate>>,
}

/// Carries no identity of its own — the version is tracked by the wrapping DB row.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProcessTemplate {
    pub name: String,
    pub description: Option<String>,
    pub step_id: i32,
    pub is_optional: bool,
    pub is_review_required: bool,
    pub substeps: Option<Vec<ProcessStepTemplate>>,
    pub fields: Option<Vec<ProcessFieldTemplate>>,
}

/// Test fixture for this crate's own unit tests (can't depend on `quality`'s JSON fixture — cyclic dep).
#[cfg(test)]
pub(crate) mod test_fixtures {
    use super::{ProcessFieldTemplate, ProcessStepTemplate, ProcessTemplate};

    fn field(
        field_id: i32,
        name: &str,
        r#type: &str,
        is_optional: bool,
        filled_by: &str,
    ) -> ProcessFieldTemplate {
        ProcessFieldTemplate {
            field_id,
            name: name.to_string(),
            r#type: r#type.to_string(),
            is_optional,
            filled_by: filled_by.to_string(),
            requirements: None,
            requires_signature: false,
            required_signs_from_designations: Vec::new(),
        }
    }

    pub(crate) fn tenant_onboarding_fixture() -> ProcessTemplate {
        ProcessTemplate {
            name: "Tenant Onboarding Process".to_string(),
            description: Some(
                "Stages for tenant onboarding from owner document submission to shifting permission"
                    .to_string(),
            ),
            step_id: 1,
            is_optional: false,
            is_review_required: false,
            fields: None,
            substeps: Some(vec![
                ProcessStepTemplate {
                    step_id: 2,
                    name: "Step 1: Onboarding Documents and Payments".to_string(),
                    description: Some(
                        "Submit owner documents, tenant KYC, undertaking, and pay necessary fees"
                            .to_string(),
                    ),
                    is_optional: false,
                    is_review_required: true,
                    fields: None,
                    substeps: Some(vec![
                        ProcessStepTemplate {
                            step_id: 3,
                            name: "Owner Documents".to_string(),
                            description: Some(
                                "Submit ownership documentation (if not submitted already)"
                                    .to_string(),
                            ),
                            is_optional: false,
                            is_review_required: false,
                            substeps: None,
                            fields: Some(vec![
                                field(101, "Sale Agreement / Chain Agreement", "document", false, "user"),
                                field(102, "Possession Letter", "document", false, "user"),
                                field(103, "Parking Allotment Letter", "document", false, "user"),
                                field(104, "Loan Sanction Letter (If Loan Availed)", "document", true, "user"),
                                field(105, "Owner KYC Documents", "document", false, "user"),
                                field(106, "ADDA Enrollment Form", "document", false, "user"),
                            ]),
                        },
                        ProcessStepTemplate {
                            step_id: 4,
                            name: "Tenant KYC".to_string(),
                            description: Some("Submit KYC documents for the tenant".to_string()),
                            is_optional: false,
                            is_review_required: false,
                            substeps: None,
                            fields: Some(vec![field(107, "Tenant KYC Documents", "document", false, "user")]),
                        },
                        ProcessStepTemplate {
                            step_id: 5,
                            name: "Undertaking".to_string(),
                            description: Some(
                                "Submit signed undertaking on 200/- stamp paper in society format"
                                    .to_string(),
                            ),
                            is_optional: false,
                            is_review_required: false,
                            substeps: None,
                            fields: Some(vec![field(108, "Undertaking (Stamp Paper)", "document", false, "user")]),
                        },
                        ProcessStepTemplate {
                            step_id: 6,
                            name: "Payments".to_string(),
                            description: Some(
                                "Pay shifting charges and one-year NOC charges".to_string(),
                            ),
                            is_optional: false,
                            is_review_required: false,
                            substeps: None,
                            fields: Some(vec![
                                field(109, "One Year NOC Charges (11,800/-)", "document", false, "user"),
                                field(110, "Shifting Charges (8,614/-)", "document", false, "user"),
                                field(116, "Additional Notes (e.g. file passwords)", "text", true, "user"),
                            ]),
                        },
                    ]),
                },
                ProcessStepTemplate {
                    step_id: 7,
                    name: "Step 2: Meeting with Committee".to_string(),
                    description: Some(
                        "Schedule and complete the meeting with Office Bearers / Managing Committee"
                            .to_string(),
                    ),
                    is_optional: false,
                    is_review_required: true,
                    substeps: None,
                    fields: Some(vec![field(111, "Meeting Remarks & Status", "text", false, "admin")]),
                },
                ProcessStepTemplate {
                    step_id: 8,
                    name: "Step 3: NOC Issuance".to_string(),
                    description: Some("NOC will be issued by the society administration".to_string()),
                    is_optional: false,
                    is_review_required: true,
                    substeps: None,
                    fields: Some(vec![field(112, "Issued NOC Document", "document", false, "admin")]),
                },
                ProcessStepTemplate {
                    step_id: 9,
                    name: "Step 4: Agreement Registration & Police Verification".to_string(),
                    description: Some(
                        "Submit registered agreement and police verification details".to_string(),
                    ),
                    is_optional: false,
                    is_review_required: true,
                    substeps: None,
                    fields: Some(vec![
                        field(113, "Registered Agreement", "document", false, "user"),
                        field(114, "Police Verification Form / Receipt", "document", false, "user"),
                    ]),
                },
                ProcessStepTemplate {
                    step_id: 10,
                    name: "Step 5: Shifting Permission".to_string(),
                    description: Some(
                        "Receive final shifting permission from the society office".to_string(),
                    ),
                    is_optional: false,
                    is_review_required: true,
                    substeps: None,
                    fields: Some(vec![field(115, "Shifting Permission Letter", "document", false, "admin")]),
                },
            ]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_fixtures::tenant_onboarding_fixture;
    use super::*;

    #[test]
    fn test_load_test_template_fixture() {
        let template = tenant_onboarding_fixture();
        assert_eq!(template.step_id, 1);

        let substeps = template.substeps.expect("Substeps should exist");
        assert_eq!(substeps.len(), 5);
        assert_eq!(substeps[0].step_id, 2);
        assert_eq!(
            substeps[0].name,
            "Step 1: Onboarding Documents and Payments"
        );
    }

    #[test]
    fn test_no_overlapping_ids_in_and_across_templates() {
        let template_versions = vec!["tenant_onboarding"];
        let mut all_step_ids = std::collections::HashSet::new();
        let mut all_field_ids = std::collections::HashSet::new();

        for version in template_versions {
            let template = tenant_onboarding_fixture();

            // Check uniqueness within this single template
            let mut template_step_ids = std::collections::HashSet::new();
            let mut template_field_ids = std::collections::HashSet::new();

            // Root step ID
            assert!(
                template_step_ids.insert(template.step_id),
                "Duplicate root step_id {} in template {}",
                template.step_id,
                version
            );

            // Fields in root
            if let Some(ref fields) = template.fields {
                for field in fields {
                    assert!(
                        template_field_ids.insert(field.field_id),
                        "Duplicate field_id {} in root of template {}",
                        field.field_id,
                        version
                    );
                }
            }

            // Recursive helper for substeps
            fn collect_substep_ids(
                substep: &ProcessStepTemplate,
                step_ids: &mut std::collections::HashSet<i32>,
                field_ids: &mut std::collections::HashSet<i32>,
                version: &str,
            ) {
                assert!(
                    step_ids.insert(substep.step_id),
                    "Duplicate step_id {} in template {}",
                    substep.step_id,
                    version
                );
                if let Some(ref fields) = substep.fields {
                    for field in fields {
                        assert!(
                            field_ids.insert(field.field_id),
                            "Duplicate field_id {} in template {}",
                            field.field_id,
                            version
                        );
                    }
                }
                if let Some(ref substeps) = substep.substeps {
                    for sub in substeps {
                        collect_substep_ids(sub, step_ids, field_ids, version);
                    }
                }
            }

            if let Some(ref substeps) = template.substeps {
                for sub in substeps {
                    collect_substep_ids(
                        sub,
                        &mut template_step_ids,
                        &mut template_field_ids,
                        version,
                    );
                }
            }

            // Now check across templates
            for step_id in &template_step_ids {
                assert!(
                    all_step_ids.insert(*step_id),
                    "Overlapping step_id {} across templates",
                    step_id
                );
            }
            for field_id in &template_field_ids {
                assert!(
                    all_field_ids.insert(*field_id),
                    "Overlapping field_id {} across templates",
                    field_id
                );
            }
        }
    }
}
