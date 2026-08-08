use crate::db::BudgetVarianceRow;
use rust_decimal::Decimal;

#[derive(Debug, Clone, serde::Serialize)]
pub struct VarianceRow {
    pub account_code: String,
    pub account_name: String,
    pub account_type: String,
    pub budgeted_amount: Decimal,
    pub actual_amount: Decimal,
    pub variance_amount: Decimal,
    /// `None` when budgeted_amount is zero — a percentage against zero is meaningless,
    /// not "infinite," so callers must handle the absence explicitly rather than seeing
    /// a suspicious 0% or a divide-by-zero.
    pub variance_pct: Option<Decimal>,
}

pub fn compute_variance(row: BudgetVarianceRow) -> VarianceRow {
    let variance_amount = row.actual_amount - row.budgeted_amount;
    let variance_pct = if row.budgeted_amount != Decimal::ZERO {
        Some((variance_amount / row.budgeted_amount * Decimal::from(100)).round_dp(2))
    } else {
        None
    };
    VarianceRow {
        account_code: row.account_code,
        account_name: row.account_name,
        account_type: row.account_type,
        budgeted_amount: row.budgeted_amount,
        actual_amount: row.actual_amount,
        variance_amount,
        variance_pct,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AccountId;
    use rust_decimal_macros::dec;

    fn row(budgeted: Decimal, actual: Decimal) -> BudgetVarianceRow {
        BudgetVarianceRow {
            account_id: AccountId(1),
            account_code: "5100".to_string(),
            account_name: "Security Expense".to_string(),
            account_type: "EXPENSE".to_string(),
            budgeted_amount: budgeted,
            actual_amount: actual,
        }
    }

    #[test]
    fn over_budget_is_positive_variance() {
        let v = compute_variance(row(dec!(10000), dec!(12000)));
        assert_eq!(v.variance_amount, dec!(2000));
        assert_eq!(v.variance_pct, Some(dec!(20.00)));
    }

    #[test]
    fn under_budget_is_negative_variance() {
        let v = compute_variance(row(dec!(10000), dec!(8000)));
        assert_eq!(v.variance_amount, dec!(-2000));
        assert_eq!(v.variance_pct, Some(dec!(-20.00)));
    }

    #[test]
    fn zero_budget_gives_no_percentage_not_a_panic() {
        let v = compute_variance(row(dec!(0), dec!(500)));
        assert_eq!(v.variance_amount, dec!(500));
        assert_eq!(v.variance_pct, None);
    }
}
