pub mod db;
pub mod fields;
pub mod state_machine;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProcessFieldTemplate {
    pub field_id: i32,
    pub name: String,
    pub r#type: String, // "document", "option", "text"
    pub is_optional: bool,
    pub filled_by: String, // "user", "admin"
    pub requirements: Option<serde_json::Value>,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProcessTemplate {
    pub template_id: String,
    pub name: String,
    pub description: Option<String>,
    pub step_id: i32,
    pub is_optional: bool,
    pub is_review_required: bool,
    pub substeps: Option<Vec<ProcessStepTemplate>>,
    pub fields: Option<Vec<ProcessFieldTemplate>>,
}

impl ProcessTemplate {
    /// Load the tenant onboarding process template compiled into the binary.
    pub fn load_tenant_onboarding() -> Result<Self, serde_json::Error> {
        let content = include_str!("../templates/tenant_onboarding.json");
        serde_json::from_str(content)
    }

    /// Load a process template by its version/template_id.
    pub fn load_by_version(template_id: &str) -> Result<Self, String> {
        match template_id {
            "tenant_onboarding_v1" | "tenant_onboarding" => {
                Self::load_tenant_onboarding().map_err(|e| e.to_string())
            }
            _ => Err(format!("Unknown template version: {}", template_id)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_tenant_onboarding() {
        let template =
            ProcessTemplate::load_tenant_onboarding().expect("Failed to load and parse template");
        assert_eq!(template.template_id, "tenant_onboarding_v1");
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
    fn test_load_by_version() {
        let template = ProcessTemplate::load_by_version("tenant_onboarding_v1").unwrap();
        assert_eq!(template.template_id, "tenant_onboarding_v1");

        let template_alias = ProcessTemplate::load_by_version("tenant_onboarding").unwrap();
        assert_eq!(template_alias.template_id, "tenant_onboarding_v1");

        let err = ProcessTemplate::load_by_version("invalid_version");
        assert!(err.is_err());
        assert_eq!(
            err.unwrap_err(),
            "Unknown template version: invalid_version"
        );
    }

    #[test]
    fn test_no_overlapping_ids_in_and_across_templates() {
        let template_versions = vec!["tenant_onboarding_v1"];
        let mut all_step_ids = std::collections::HashSet::new();
        let mut all_field_ids = std::collections::HashSet::new();

        for version in template_versions {
            let template = ProcessTemplate::load_by_version(version).unwrap();

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
