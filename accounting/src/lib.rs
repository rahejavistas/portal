pub mod bank_reconciliation;
pub mod billing;
pub mod budgets;
pub mod claims;
pub mod db;
pub mod fiscal_year;
pub mod funds;
pub mod gst;
pub mod interest;
pub mod ledger;
pub mod payments;
pub mod pdf;
pub mod reports;
pub mod tds;
pub mod vendors;

use uuid::Uuid;

/// India's 28 states and 8 union territories — a genuinely fixed, non-admin-extensible
/// set (unlike designation/permission codes), so it's validated against this list
/// rather than stored as free text. Beyond the printed address, this is what a future
/// bylaws-reading feature needs to look up the right state's Co-operative Societies Act.
pub mod geography {
    pub const INDIAN_STATES_AND_UTS: &[&str] = &[
        "Andhra Pradesh",
        "Arunachal Pradesh",
        "Assam",
        "Bihar",
        "Chhattisgarh",
        "Goa",
        "Gujarat",
        "Haryana",
        "Himachal Pradesh",
        "Jharkhand",
        "Karnataka",
        "Kerala",
        "Madhya Pradesh",
        "Maharashtra",
        "Manipur",
        "Meghalaya",
        "Mizoram",
        "Nagaland",
        "Odisha",
        "Punjab",
        "Rajasthan",
        "Sikkim",
        "Tamil Nadu",
        "Telangana",
        "Tripura",
        "Uttar Pradesh",
        "Uttarakhand",
        "West Bengal",
        "Andaman and Nicobar Islands",
        "Chandigarh",
        "Dadra and Nagar Haveli and Daman and Diu",
        "Delhi",
        "Jammu and Kashmir",
        "Ladakh",
        "Lakshadweep",
        "Puducherry",
    ];
}

/// Well-known chart-of-accounts codes, seeded by the accounting_p0/p1 migrations.
/// Referenced by code (not id) since ids are assigned at seed time.
pub mod accounts {
    pub const BANK_OPERATING: &str = "1000";
    pub const MEMBER_DUES_RECEIVABLE: &str = "1200";
    pub const GST_PAYABLE: &str = "2100";
    pub const VENDOR_PAYABLES: &str = "2200";
    pub const TDS_PAYABLE: &str = "2300";
    pub const MEMBER_ADVANCE: &str = "2400";
    pub const RETAINED_EARNINGS: &str = "3000";
    pub const MAINTENANCE_INCOME: &str = "4000";
    pub const LATE_PAYMENT_INTEREST_INCOME: &str = "4100";
}

/// Newtype IDs for every entity this crate owns, so e.g. a `PaymentId` can never be
/// passed where an `InvoiceId` is expected — the compiler catches it, not a review.
/// `#[sqlx(transparent)]` makes each one bind/decode exactly like its inner type, so
/// these cost nothing at the query layer.
macro_rules! entity_id {
    ($name:ident, $inner:ty) => {
        #[derive(
            Debug,
            Clone,
            Copy,
            PartialEq,
            Eq,
            Hash,
            serde::Serialize,
            serde::Deserialize,
            sqlx::Type,
        )]
        #[sqlx(transparent)]
        pub struct $name(pub $inner);

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    };
}

entity_id!(InvoiceId, Uuid);
entity_id!(TransactionId, Uuid);
entity_id!(PaymentId, Uuid);
entity_id!(ReceiptId, Uuid);
entity_id!(AccountId, i32);
entity_id!(FiscalYearId, i32);
entity_id!(BillingPlanId, i32);
entity_id!(InterestRuleId, i32);
entity_id!(GstRuleId, i32);
entity_id!(ClaimId, Uuid);
entity_id!(FundId, i32);
entity_id!(FundRuleId, i32);
entity_id!(VendorId, i32);
entity_id!(PoId, Uuid);
entity_id!(ExpenseId, Uuid);
entity_id!(TdsRuleId, i32);
entity_id!(BankImportId, Uuid);
entity_id!(BankLineId, i64);
entity_id!(BudgetId, i32);
