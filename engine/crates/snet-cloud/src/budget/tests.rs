use super::*;

fn limits() -> TaskLimits {
    TaskLimits {
        requests: 3,
        bytes_sent: 1_000,
        input_tokens: 5_000,
        output_tokens: 2_000,
        cost_micro: 100_000,
    }
}

fn prices() -> Prices {
    Prices {
        currency: "USD".to_string(),
        input_micro_per_million_tokens: 3_000_000,
        output_micro_per_million_tokens: 15_000_000,
    }
}

fn charge(requests: u128, bytes: u128, input: u128, output: u128, cost: u128) -> Charge {
    Charge {
        requests,
        bytes_sent: bytes,
        input_tokens: input,
        output_tokens: output,
        cost_micro: cost,
    }
}

fn budget() -> TaskBudget {
    TaskBudget::new("task-1", limits()).unwrap()
}

#[test]
fn worst_case_counts_body_bytes_allowance_and_output_cap() {
    let worst = Charge::worst_case(100, 50, 200, &prices()).unwrap();
    assert_eq!(worst.requests, 1);
    assert_eq!(worst.bytes_sent, 100);
    assert_eq!(worst.input_tokens, 150);
    assert_eq!(worst.output_tokens, 200);
    assert_eq!(worst.cost_micro, 150 * 3 + 200 * 15);
}

#[test]
fn cost_rounds_up_per_direction() {
    let cheap = Prices {
        currency: "USD".to_string(),
        input_micro_per_million_tokens: 1,
        output_micro_per_million_tokens: 2,
    };
    assert_eq!(Charge::for_usage(0, 1, 0, &cheap).unwrap().cost_micro, 1);
    assert_eq!(
        Charge::for_usage(0, 1_000_000, 0, &cheap)
            .unwrap()
            .cost_micro,
        1
    );
    assert_eq!(
        Charge::for_usage(0, 1_000_001, 0, &cheap)
            .unwrap()
            .cost_micro,
        2
    );
    assert_eq!(Charge::for_usage(0, 1, 1, &cheap).unwrap().cost_micro, 2);
    assert_eq!(
        Charge::for_usage(0, 0, 500_000, &cheap).unwrap().cost_micro,
        1
    );
    assert_eq!(Charge::for_usage(0, 0, 0, &cheap).unwrap().cost_micro, 0);
}

#[test]
fn each_limit_admits_its_value_and_refuses_one_unit_over() {
    let cases: [(&str, Charge, Charge); 5] = [
        ("requests", charge(3, 0, 0, 0, 0), charge(4, 0, 0, 0, 0)),
        (
            "bytes sent",
            charge(0, 1_000, 0, 0, 0),
            charge(0, 1_001, 0, 0, 0),
        ),
        (
            "input tokens",
            charge(0, 0, 5_000, 0, 0),
            charge(0, 0, 5_001, 0, 0),
        ),
        (
            "output tokens",
            charge(0, 0, 0, 2_000, 0),
            charge(0, 0, 0, 2_001, 0),
        ),
        (
            "cost",
            charge(0, 0, 0, 0, 100_000),
            charge(0, 0, 0, 0, 100_001),
        ),
    ];
    for (quantity, at_limit, over_limit) in cases {
        assert!(
            budget().reserve(at_limit).is_ok(),
            "{quantity} at its limit"
        );
        match budget().reserve(over_limit) {
            Err(BudgetError::LimitExceeded {
                quantity: refused, ..
            }) => assert_eq!(refused, quantity),
            other => panic!("{quantity} one over: {other:?}"),
        }
    }
}

#[test]
fn a_reservation_blocks_a_second_request_that_would_exceed() {
    let budget = budget();
    let first = budget.reserve(charge(1, 600, 0, 0, 0)).unwrap();
    let refused = budget.reserve(charge(1, 600, 0, 0, 0));
    assert!(matches!(refused, Err(BudgetError::LimitExceeded { .. })));
    budget.release(first).unwrap();
    assert!(budget.reserve(charge(1, 600, 0, 0, 0)).is_ok());
}

#[test]
fn settling_replaces_the_reservation_with_the_usage() {
    let budget = budget();
    let reservation = budget.reserve(charge(1, 100, 900, 800, 50)).unwrap();
    budget
        .settle(reservation, charge(1, 100, 300, 200, 10))
        .unwrap();
    assert_eq!(budget.charged().unwrap(), charge(1, 100, 300, 200, 10));
}

#[test]
fn a_dropped_reservation_stays_charged() {
    let budget = budget();
    {
        let _unsettled = budget.reserve(charge(1, 100, 900, 800, 50)).unwrap();
    }
    assert_eq!(budget.charged().unwrap(), charge(1, 100, 900, 800, 50));
}

#[test]
fn usage_above_the_worst_case_is_charged_and_closes_the_budget() {
    let budget = budget();
    let reservation = budget.reserve(charge(1, 100, 200, 100, 10)).unwrap();
    budget
        .settle_and_close(reservation, charge(1, 100, 200, 4_000, 70))
        .unwrap();
    assert_eq!(budget.charged().unwrap(), charge(1, 100, 200, 4_000, 70));
    assert!(budget.is_closed().unwrap());
    assert_eq!(
        budget.reserve(charge(1, 1, 1, 1, 1)).unwrap_err(),
        BudgetError::Closed
    );
}

#[test]
fn usage_past_a_ceiling_closes_the_budget_and_keeps_the_reservation() {
    let budget = budget();
    let reservation = budget.reserve(charge(1, 100, 200, 100, 10)).unwrap();
    let absurd = charge(1, 100, 200, u128::from(MAX_TASK_TOKENS) + 1, 10);
    assert_eq!(
        budget.settle_and_close(reservation, absurd).unwrap_err(),
        BudgetError::CeilingExceeded {
            quantity: "output tokens"
        }
    );
    assert!(budget.is_closed().unwrap());
    assert_eq!(budget.charged().unwrap(), charge(1, 100, 200, 100, 10));
}

#[test]
fn arithmetic_fails_loudly_instead_of_wrapping() {
    assert_eq!(
        Charge::for_usage(0, u128::MAX, 0, &prices()).unwrap_err(),
        BudgetError::CeilingExceeded { quantity: "cost" }
    );
    let total = charge(u128::MAX, 0, 0, 0, 0);
    assert_eq!(
        total.plus(&charge(1, 0, 0, 0, 0)).unwrap_err(),
        BudgetError::CeilingExceeded {
            quantity: "requests"
        }
    );
    for (quantity, over) in [
        (
            "requests",
            charge(u128::from(MAX_TASK_REQUESTS) + 1, 0, 0, 0, 0),
        ),
        (
            "bytes sent",
            charge(0, u128::from(MAX_TASK_BYTES_SENT) + 1, 0, 0, 0),
        ),
        (
            "input tokens",
            charge(0, 0, u128::from(MAX_TASK_TOKENS) + 1, 0, 0),
        ),
        (
            "output tokens",
            charge(0, 0, 0, u128::from(MAX_TASK_TOKENS) + 1, 0),
        ),
        (
            "cost",
            charge(0, 0, 0, 0, u128::from(MAX_TASK_COST_MICRO) + 1),
        ),
    ] {
        assert_eq!(
            Charge::default().plus(&over).unwrap_err(),
            BudgetError::CeilingExceeded { quantity },
        );
    }
}

#[test]
fn the_largest_configured_values_still_price_without_overflow() {
    let prices = Prices {
        currency: "USD".to_string(),
        input_micro_per_million_tokens: crate::config::ceilings::MAX_PRICE_MICRO_PER_MILLION_TOKENS,
        output_micro_per_million_tokens:
            crate::config::ceilings::MAX_PRICE_MICRO_PER_MILLION_TOKENS,
    };
    let worst = Charge::worst_case(
        crate::config::ceilings::MAX_BODY_BYTES_CAP,
        crate::config::ceilings::MAX_INPUT_TOKEN_ALLOWANCE,
        crate::config::ceilings::MAX_OUTPUT_TOKENS_PER_REQUEST,
        &prices,
    )
    .unwrap();
    assert!(worst.cost_micro > 0);
}

#[test]
fn task_ids_must_be_short_visible_ascii() {
    assert_eq!(
        TaskBudget::new("", limits()).unwrap_err(),
        BudgetError::InvalidTaskId
    );
    assert_eq!(
        TaskBudget::new("has space", limits()).unwrap_err(),
        BudgetError::InvalidTaskId
    );
    assert_eq!(
        TaskBudget::new(&"a".repeat(129), limits()).unwrap_err(),
        BudgetError::InvalidTaskId
    );
    assert!(TaskBudget::new(&"a".repeat(128), limits()).is_ok());
}
