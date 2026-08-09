use crate::db::GstRule;
use rust_decimal::Decimal;

/// Whether GST applies, and the amount. Exempt below the threshold; above it, GST applies to the full amount.
pub fn compute_gst(rule: &GstRule, base_amount: Decimal) -> (bool, Decimal) {
    if base_amount <= rule.per_member_monthly_exempt_threshold {
        return (false, Decimal::ZERO);
    }
    let gst = (base_amount * rule.gst_rate_pct / Decimal::from(100)).round_dp(2);
    (true, gst)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn rule(threshold: Decimal, rate: Decimal) -> GstRule {
        GstRule {
            gst_rule_id: crate::GstRuleId(1),
            per_member_monthly_exempt_threshold: threshold,
            society_turnover_registration_threshold: dec!(2000000),
            gst_rate_pct: rate,
            hsn_sac_code: "9995".to_string(),
            society_gstin: None,
            created_by: 0,
        }
    }

    #[test]
    fn exempt_at_or_below_threshold() {
        let r = rule(dec!(7500), dec!(18));
        assert_eq!(compute_gst(&r, dec!(7500)), (false, Decimal::ZERO));
        assert_eq!(compute_gst(&r, dec!(5000)), (false, Decimal::ZERO));
    }

    #[test]
    fn taxes_full_amount_once_above_threshold() {
        let r = rule(dec!(7500), dec!(18));
        let (applicable, gst) = compute_gst(&r, dec!(8000));
        assert!(applicable);
        assert_eq!(gst, dec!(1440.00));
    }
}
