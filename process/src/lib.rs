pub mod audit;
pub mod db;
pub mod dues_import;
pub mod fields;
pub mod notification;
pub mod state_machine;
pub mod template_store;
pub mod working_days;

use serde::{Deserialize, Serialize};

/// Wire values match the pre-existing stored data — `File` keeps the historical
/// `"document"` tag so every already-saved template stays valid on deserialize.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FieldType {
    Text,
    Int,
    Decimal,
    Date,
    Bool,
    Enum,
    #[serde(rename = "document")]
    File,
    /// Declaration text with blanks and a required agree checkbox; never optional.
    Declaration,
    /// Read-only, system-computed outstanding dues — never resident-entered.
    #[serde(rename = "no_dues")]
    NoDues,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum StepOwner {
    #[default]
    User,
    Admin,
    Designation {
        code: String,
    },
    /// Any wing's representative may act — not scoped to the process's own flat's wing.
    #[serde(rename = "wing_representative")]
    WingRepresentative,
    /// Only the flat's current active tenant (`tenant_ownerships`), never the owner.
    Tenant,
}

impl StepOwner {
    /// Which side of the process an open step on this owner puts the turn on.
    pub fn turn(&self) -> crate::state_machine::ProcessTurn {
        use crate::state_machine::ProcessTurn;
        match self {
            StepOwner::User | StepOwner::Tenant => ProcessTurn::User,
            StepOwner::Admin | StepOwner::Designation { .. } | StepOwner::WingRepresentative => {
                ProcessTurn::Admin
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProcessFieldTemplate {
    pub field_id: i32,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub r#type: FieldType,
    pub is_optional: bool,
    pub requirements: Option<serde_json::Value>,
    /// Document fields only. Blocks step completion until the field's document has a
    /// signature requirement attached and signed.
    #[serde(default)]
    pub requires_signature: bool,
    /// Designations eligible to sign this field, in required signing order. Enforced
    /// both when a reviewer attaches signers and again at the moment of signing.
    #[serde(default)]
    pub required_signs_from_designations: Vec<String>,
    /// `Enum` fields only — the closed list of valid values. Admin-configured, so it
    /// stays a plain string list rather than a compile-time Rust enum.
    #[serde(default)]
    pub options: Option<Vec<String>>,
    /// Document fields only — a PUBLIC document residents can download as a reference.
    #[serde(default)]
    pub example_document_id: Option<uuid::Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProcessStepTemplate {
    pub step_id: i32,
    pub name: String,
    pub description: Option<String>,
    pub is_optional: bool,
    #[serde(default)]
    pub filled_by: StepOwner,
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
    pub substeps: Option<Vec<ProcessStepTemplate>>,
    pub fields: Option<Vec<ProcessFieldTemplate>>,
    /// Working days allowed for this process, once the checklist is fully submitted.
    /// `None` means SLA tracking is off for this template.
    #[serde(default)]
    pub tat_working_days: Option<i32>,
    #[serde(default)]
    pub filled_by: StepOwner,
}

/// Test fixture for this crate's own unit tests (can't depend on `quality`'s JSON fixture — cyclic dep).
#[cfg(test)]
pub(crate) mod test_fixtures {
    use super::{FieldType, ProcessFieldTemplate, ProcessStepTemplate, ProcessTemplate, StepOwner};

    fn field(field_id: i32, name: &str, r#type: &str, is_optional: bool) -> ProcessFieldTemplate {
        let r#type = match r#type {
            "document" => FieldType::File,
            "text" => FieldType::Text,
            other => panic!("unknown fixture field type: {other}"),
        };
        ProcessFieldTemplate {
            field_id,
            name: name.to_string(),
            description: None,
            r#type,
            is_optional,
            requirements: None,
            requires_signature: false,
            required_signs_from_designations: Vec::new(),
            options: None,
            example_document_id: None,
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
            fields: None,
            tat_working_days: None,
            filled_by: StepOwner::User,
            substeps: Some(vec![
                ProcessStepTemplate {
                    step_id: 2,
                    name: "Step 1: Onboarding Documents and Payments".to_string(),
                    description: Some(
                        "Submit owner documents, tenant KYC, undertaking, and pay necessary fees"
                            .to_string(),
                    ),
                    is_optional: false,
                    filled_by: StepOwner::User,
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
                            filled_by: StepOwner::User,
                            substeps: None,
                            fields: Some(vec![
                                field(101, "Sale Agreement / Chain Agreement", "document", false),
                                field(102, "Possession Letter", "document", false),
                                field(103, "Parking Allotment Letter", "document", false),
                                field(104, "Loan Sanction Letter (If Loan Availed)", "document", true),
                                field(105, "Owner KYC Documents", "document", false),
                                field(106, "ADDA Enrollment Form", "document", false),
                            ]),
                        },
                        ProcessStepTemplate {
                            step_id: 4,
                            name: "Tenant KYC".to_string(),
                            description: Some("Submit KYC documents for the tenant".to_string()),
                            is_optional: false,
                            filled_by: StepOwner::User,
                            substeps: None,
                            fields: Some(vec![field(107, "Tenant KYC Documents", "document", false)]),
                        },
                        ProcessStepTemplate {
                            step_id: 5,
                            name: "Undertaking".to_string(),
                            description: Some(
                                "Submit signed undertaking on 200/- stamp paper in society format"
                                    .to_string(),
                            ),
                            is_optional: false,
                            filled_by: StepOwner::User,
                            substeps: None,
                            fields: Some(vec![field(108, "Undertaking (Stamp Paper)", "document", false)]),
                        },
                        ProcessStepTemplate {
                            step_id: 6,
                            name: "Payments".to_string(),
                            description: Some(
                                "Pay shifting charges and one-year NOC charges".to_string(),
                            ),
                            is_optional: false,
                            filled_by: StepOwner::User,
                            substeps: None,
                            fields: Some(vec![
                                field(109, "One Year NOC Charges (11,800/-)", "document", false),
                                field(110, "Shifting Charges (8,614/-)", "document", false),
                                field(116, "Additional Notes (e.g. file passwords)", "text", true),
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
                    filled_by: StepOwner::Admin,
                    substeps: None,
                    fields: Some(vec![field(111, "Meeting Remarks & Status", "text", false)]),
                },
                ProcessStepTemplate {
                    step_id: 8,
                    name: "Step 3: NOC Issuance".to_string(),
                    description: Some("NOC will be issued by the society administration".to_string()),
                    is_optional: false,
                    filled_by: StepOwner::Admin,
                    substeps: None,
                    fields: Some(vec![field(112, "Issued NOC Document", "document", false)]),
                },
                ProcessStepTemplate {
                    step_id: 9,
                    name: "Step 4: Agreement Registration & Police Verification".to_string(),
                    description: Some(
                        "Submit registered agreement and police verification details".to_string(),
                    ),
                    is_optional: false,
                    filled_by: StepOwner::User,
                    substeps: None,
                    fields: Some(vec![
                        field(113, "Registered Agreement", "document", false),
                        field(114, "Police Verification Form / Receipt", "document", false),
                    ]),
                },
                ProcessStepTemplate {
                    step_id: 10,
                    name: "Step 5: Shifting Permission".to_string(),
                    description: Some(
                        "Receive final shifting permission from the society office".to_string(),
                    ),
                    is_optional: false,
                    filled_by: StepOwner::Admin,
                    substeps: None,
                    fields: Some(vec![field(115, "Shifting Permission Letter", "document", false)]),
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
    fn field_type_no_dues_serializes_to_the_wire_name_the_frontend_expects() {
        assert_eq!(
            serde_json::to_value(FieldType::NoDues).unwrap(),
            serde_json::json!("no_dues")
        );
        assert_eq!(
            serde_json::from_value::<FieldType>(serde_json::json!("no_dues")).unwrap(),
            FieldType::NoDues
        );
        assert!(serde_json::from_value::<FieldType>(serde_json::json!("nodues")).is_err());
    }

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
