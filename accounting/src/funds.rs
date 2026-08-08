use crate::db::FundContributionRule;
use rust_decimal::Decimal;

/// A fund contribution is either a fixed amount, or a percentage of the flat's base
/// maintenance charge for the period — the two conventions actual society bylaws use.
pub fn compute_fund_contribution(
    rule: &FundContributionRule,
    base_maintenance_amount: Decimal,
) -> Decimal {
    match rule.contribution_type.as_str() {
        "FIXED" => rule.amount_or_pct,
        "PCT_OF_MAINTENANCE" => {
            (base_maintenance_amount * rule.amount_or_pct / Decimal::from(100)).round_dp(2)
        }
        _ => Decimal::ZERO,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FundId, FundRuleId};
    use rust_decimal_macros::dec;

    fn rule(contribution_type: &str, amount_or_pct: Decimal) -> FundContributionRule {
        FundContributionRule {
            rule_id: FundRuleId(1),
            fund_id: FundId(1),
            contribution_type: contribution_type.to_string(),
            amount_or_pct,
            effective_from: chrono::NaiveDate::from_ymd_opt(2026, 4, 1).unwrap(),
        }
    }

    #[test]
    fn fixed_contribution_ignores_maintenance_amount() {
        let r = rule("FIXED", dec!(500));
        assert_eq!(compute_fund_contribution(&r, dec!(10000)), dec!(500));
        assert_eq!(compute_fund_contribution(&r, dec!(0)), dec!(500));
    }

    #[test]
    fn percentage_contribution_scales_with_maintenance() {
        let r = rule("PCT_OF_MAINTENANCE", dec!(10));
        assert_eq!(compute_fund_contribution(&r, dec!(5000)), dec!(500.00));
    }

    #[test]
    fn unknown_contribution_type_is_zero_not_a_panic() {
        let r = rule("BOGUS", dec!(500));
        assert_eq!(compute_fund_contribution(&r, dec!(5000)), Decimal::ZERO);
    }
}
