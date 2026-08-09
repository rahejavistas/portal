use crate::db::TdsRule;
use rust_decimal::Decimal;

pub struct TdsCalculation<'a> {
    pub rule: &'a TdsRule,
    pub tds_amount: Decimal,
}

/// Whether TDS applies, and how much — crosses the threshold if this payment or the
/// running year-to-date total for this vendor exceeds the rule's threshold.
pub fn resolve_tds<'a>(
    rules: &'a [TdsRule],
    payment_category: &str,
    payment_amount: Decimal,
    ytd_vendor_amount: Decimal,
) -> Option<TdsCalculation<'a>> {
    let rule = rules
        .iter()
        .find(|r| r.payment_category == payment_category)?;
    let crosses_threshold = payment_amount >= rule.threshold_single_payment
        || (ytd_vendor_amount + payment_amount) >= rule.threshold_annual_aggregate;
    if !crosses_threshold {
        return None;
    }
    let tds_amount = (payment_amount * rule.rate_pct / Decimal::from(100)).round_dp(2);
    Some(TdsCalculation { rule, tds_amount })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TdsRuleId;
    use rust_decimal_macros::dec;

    fn contractor_rule() -> TdsRule {
        TdsRule {
            tds_rule_id: TdsRuleId(1),
            section: "194C".to_string(),
            payment_category: "contractor".to_string(),
            rate_pct: dec!(1),
            threshold_single_payment: dec!(30000),
            threshold_annual_aggregate: dec!(100000),
            created_by: 1,
        }
    }

    #[test]
    fn no_tds_below_both_thresholds() {
        let rules = vec![contractor_rule()];
        assert!(resolve_tds(&rules, "contractor", dec!(5000), dec!(10000)).is_none());
    }

    #[test]
    fn tds_applies_when_single_payment_crosses_threshold() {
        let rules = vec![contractor_rule()];
        let calc = resolve_tds(&rules, "contractor", dec!(35000), dec!(0)).unwrap();
        assert_eq!(calc.tds_amount, dec!(350.00));
    }

    #[test]
    fn tds_applies_when_annual_aggregate_crosses_even_if_single_payment_is_small() {
        let rules = vec![contractor_rule()];
        // Only 5000 this time, but 96000 already paid this FY -> 101000 total, over 100000.
        let calc = resolve_tds(&rules, "contractor", dec!(5000), dec!(96000)).unwrap();
        assert_eq!(calc.tds_amount, dec!(50.00));
    }

    #[test]
    fn no_matching_category_means_no_tds() {
        let rules = vec![contractor_rule()];
        assert!(resolve_tds(&rules, "rent", dec!(50000), dec!(0)).is_none());
    }
}
