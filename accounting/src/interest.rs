use crate::db::InterestRule;
use rust_decimal::Decimal;

pub fn compute_interest(
    rule: &InterestRule,
    overdue_amount: Decimal,
    days_overdue: i32,
) -> Decimal {
    if days_overdue <= rule.grace_period_days {
        return Decimal::ZERO;
    }
    let chargeable_days = days_overdue - rule.grace_period_days;
    match rule.compounding.as_str() {
        "MONTHLY" => {
            let months = chargeable_days / 30;
            if months == 0 {
                return Decimal::ZERO;
            }
            let monthly_rate = rule.annual_rate_pct / Decimal::from(12) / Decimal::from(100);
            let mut growth = Decimal::ONE;
            for _ in 0..months {
                growth *= Decimal::ONE + monthly_rate;
            }
            (overdue_amount * (growth - Decimal::ONE)).round_dp(2)
        }
        // SIMPLE, and any unrecognized value — the safer/more common default for society bylaws.
        _ => (overdue_amount * rule.annual_rate_pct / Decimal::from(100)
            * Decimal::from(chargeable_days)
            / Decimal::from(365))
        .round_dp(2),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn simple_rule() -> InterestRule {
        InterestRule {
            interest_rule_id: crate::InterestRuleId(1),
            annual_rate_pct: dec!(21),
            grace_period_days: 5,
            compounding: "SIMPLE".to_string(),
            created_by: 0,
        }
    }

    #[test]
    fn zero_within_grace_period() {
        let r = simple_rule();
        assert_eq!(compute_interest(&r, dec!(10000), 5), Decimal::ZERO);
    }

    #[test]
    fn simple_interest_after_grace() {
        let r = simple_rule();
        // 35 chargeable days at 21%/yr on 10000
        let interest = compute_interest(&r, dec!(10000), 40);
        assert_eq!(interest, dec!(201.37));
    }
}
