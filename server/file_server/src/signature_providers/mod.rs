//! Plug-and-play external e-signature providers. `manual` (the default) needs no
//! implementation here — a designation-holder just uploads a signed replacement
//! themselves via the existing `documents/signatures/replace-*` endpoints. An external
//! provider (currently only SignYu) instead sends the document out for e-signature and
//! reports back via `get_document`/webhook; adding a second provider tomorrow just
//! means a new module here plus one match arm in `handlers::signatures::provider_for`.

pub mod signyu;

/// Contact info an external provider needs to identify one signer.
#[derive(Debug, Clone)]
pub struct SignerContact {
    pub name: String,
    pub phone: String,
    pub email: String,
}

/// Result of successfully creating + sending a document for signature.
pub struct SendDocumentResult {
    pub external_document_id: String,
    /// One external signer id per input signer, same order as given to `add_signers`.
    pub external_signer_ids: Vec<String>,
    /// One signing-page link per input signer (same order), if the provider returned
    /// one — lets a signer open the provider's signing page directly from our own
    /// queue instead of having to find the provider's email.
    pub sign_urls: Vec<Option<String>>,
}

/// A single signer's progress as reported by the provider.
#[derive(Debug)]
pub struct SignerProgress {
    pub external_signer_id: String,
    pub has_signed: bool,
}

/// Current state of a document at the provider, as of the last poll/webhook.
#[derive(Debug)]
pub struct DocumentStatusResult {
    pub is_completed: bool,
    /// Only set once `is_completed` — a temporary, presigned link to the fully signed PDF.
    pub download_url: Option<String>,
    pub signer_progress: Vec<SignerProgress>,
}
