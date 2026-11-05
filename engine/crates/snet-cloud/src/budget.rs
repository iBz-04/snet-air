use std::sync::{Arc, Mutex, MutexGuard};

use crate::config::ceilings::{
    MAX_TASK_BYTES_SENT, MAX_TASK_COST_MICRO, MAX_TASK_REQUESTS, MAX_TASK_TOKENS,
};

const TOKENS_PER_PRICE_UNIT: u128 = 1_000_000;
const MAX_TASK_ID_LENGTH: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskLimits {
    pub requests: u64,
    pub bytes_sent: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_micro: u64,
}

impl TaskLimits {
    /// Whether every limit is at most the other's, so a caller can only
    /// narrow an endpoint's limits.
    pub fn within(&self, other: &TaskLimits) -> bool {
        self.requests <= other.requests
            && self.bytes_sent <= other.bytes_sent
            && self.input_tokens <= other.input_tokens
            && self.output_tokens <= other.output_tokens
            && self.cost_micro <= other.cost_micro
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prices {
    pub currency: String,
    pub input_micro_per_million_tokens: u64,
    pub output_micro_per_million_tokens: u64,
}

/// An amount charged to a task: a reservation, a request's reported usage or
/// a task's running total.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Charge {
    pub requests: u128,
    pub bytes_sent: u128,
    pub input_tokens: u128,
    pub output_tokens: u128,
    pub cost_micro: u128,
}

impl Charge {
    /// The worst case of one request: every byte of the body is a
    /// token, the provider adds at most the allowance, and the model writes up
    /// to the output cap. Public so a caller can price a request whose outcome
    /// was never recorded.
    pub fn worst_case(
        body_bytes: u64,
        input_token_allowance: u64,
        output_token_cap: u64,
        prices: &Prices,
    ) -> Result<Self, BudgetError> {
        let input_tokens = u128::from(body_bytes)
            .checked_add(u128::from(input_token_allowance))
            .ok_or(BudgetError::CeilingExceeded {
                quantity: "input tokens",
            })?;
        Self::for_usage(
            u128::from(body_bytes),
            input_tokens,
            u128::from(output_token_cap),
            prices,
        )
    }

    pub(crate) fn for_usage(
        bytes_sent: u128,
        input_tokens: u128,
        output_tokens: u128,
        prices: &Prices,
    ) -> Result<Self, BudgetError> {
        let cost_micro = checked_add(
            "cost",
            price_rounded_up(input_tokens, prices.input_micro_per_million_tokens)?,
            price_rounded_up(output_tokens, prices.output_micro_per_million_tokens)?,
        )?;
        Ok(Self {
            requests: 1,
            bytes_sent,
            input_tokens,
            output_tokens,
            cost_micro,
        })
    }

    pub(crate) fn exceeds(&self, other: &Charge) -> bool {
        self.input_tokens > other.input_tokens
            || self.output_tokens > other.output_tokens
            || self.cost_micro > other.cost_micro
            || self.bytes_sent > other.bytes_sent
            || self.requests > other.requests
    }

    fn plus(&self, other: &Charge) -> Result<Self, BudgetError> {
        let total = Self {
            requests: checked_add("requests", self.requests, other.requests)?,
            bytes_sent: checked_add("bytes sent", self.bytes_sent, other.bytes_sent)?,
            input_tokens: checked_add("input tokens", self.input_tokens, other.input_tokens)?,
            output_tokens: checked_add("output tokens", self.output_tokens, other.output_tokens)?,
            cost_micro: checked_add("cost", self.cost_micro, other.cost_micro)?,
        };
        total.check_ceilings()?;
        Ok(total)
    }

    fn minus(&self, other: &Charge) -> Result<Self, BudgetError> {
        let subtract = |left: u128, right: u128| {
            left.checked_sub(right)
                .ok_or(BudgetError::AccountingInconsistent)
        };
        Ok(Self {
            requests: subtract(self.requests, other.requests)?,
            bytes_sent: subtract(self.bytes_sent, other.bytes_sent)?,
            input_tokens: subtract(self.input_tokens, other.input_tokens)?,
            output_tokens: subtract(self.output_tokens, other.output_tokens)?,
            cost_micro: subtract(self.cost_micro, other.cost_micro)?,
        })
    }

    fn check_ceilings(&self) -> Result<(), BudgetError> {
        let quantities = [
            ("requests", self.requests, MAX_TASK_REQUESTS),
            ("bytes sent", self.bytes_sent, MAX_TASK_BYTES_SENT),
            ("input tokens", self.input_tokens, MAX_TASK_TOKENS),
            ("output tokens", self.output_tokens, MAX_TASK_TOKENS),
            ("cost", self.cost_micro, MAX_TASK_COST_MICRO),
        ];
        for (quantity, value, ceiling) in quantities {
            if value > u128::from(ceiling) {
                return Err(BudgetError::CeilingExceeded { quantity });
            }
        }
        Ok(())
    }

    fn check_limits(&self, limits: &TaskLimits) -> Result<(), BudgetError> {
        let quantities = [
            ("requests", self.requests, limits.requests),
            ("bytes sent", self.bytes_sent, limits.bytes_sent),
            ("input tokens", self.input_tokens, limits.input_tokens),
            ("output tokens", self.output_tokens, limits.output_tokens),
            ("cost", self.cost_micro, limits.cost_micro),
        ];
        for (quantity, would_be, limit) in quantities {
            if would_be > u128::from(limit) {
                return Err(BudgetError::LimitExceeded {
                    quantity,
                    would_be,
                    limit,
                });
            }
        }
        Ok(())
    }
}

fn checked_add(quantity: &'static str, left: u128, right: u128) -> Result<u128, BudgetError> {
    left.checked_add(right)
        .ok_or(BudgetError::CeilingExceeded { quantity })
}

/// Integer micro-units, rounded up so cost is never under-estimated.
fn price_rounded_up(tokens: u128, micro_per_million_tokens: u64) -> Result<u128, BudgetError> {
    let product = tokens
        .checked_mul(u128::from(micro_per_million_tokens))
        .ok_or(BudgetError::CeilingExceeded { quantity: "cost" })?;
    let rounded = checked_add("cost", product, TOKENS_PER_PRICE_UNIT - 1)?;
    Ok(rounded / TOKENS_PER_PRICE_UNIT)
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BudgetError {
    #[error("the task ID must be 1 to 128 visible ASCII characters")]
    InvalidTaskId,
    #[error("the task budget is closed because reported usage exceeded a request's worst case")]
    Closed,
    #[error("the task's {quantity} would reach {would_be}, above its limit of {limit}")]
    LimitExceeded {
        quantity: &'static str,
        would_be: u128,
        limit: u64,
    },
    #[error("a {quantity} computation crossed its ceiling")]
    CeilingExceeded { quantity: &'static str },
    #[error("a settlement removed more than the task had charged")]
    AccountingInconsistent,
    #[error("the task budget's lock was poisoned by a panic")]
    Poisoned,
}

/// A worst case taken from a task's budget before the request is recorded
///. Dropping it leaves it charged in full.
#[derive(Debug)]
pub(crate) struct Reservation {
    amount: Charge,
}

impl Reservation {
    pub(crate) fn amount(&self) -> Charge {
        self.amount
    }
}

#[derive(Debug)]
struct BudgetState {
    limits: TaskLimits,
    charged: Charge,
    closed: bool,
}

/// One task's running totals against its limits. Cloning gives another
/// handle to the same totals, so an exchange task that outlives its caller
/// still settles the task's budget.
#[derive(Debug, Clone)]
pub struct TaskBudget {
    task_id: Arc<str>,
    state: Arc<Mutex<BudgetState>>,
}

impl TaskBudget {
    pub fn new(task_id: &str, limits: TaskLimits) -> Result<Self, BudgetError> {
        let valid = !task_id.is_empty()
            && task_id.len() <= MAX_TASK_ID_LENGTH
            && task_id.bytes().all(|byte| byte.is_ascii_graphic());
        if !valid {
            return Err(BudgetError::InvalidTaskId);
        }
        Ok(Self {
            task_id: Arc::from(task_id),
            state: Arc::new(Mutex::new(BudgetState {
                limits,
                charged: Charge::default(),
                closed: false,
            })),
        })
    }

    pub fn task_id(&self) -> &str {
        &self.task_id
    }

    pub fn charged(&self) -> Result<Charge, BudgetError> {
        Ok(self.lock()?.charged)
    }

    pub fn is_closed(&self) -> Result<bool, BudgetError> {
        Ok(self.lock()?.closed)
    }

    pub(crate) fn reserve(&self, worst_case: Charge) -> Result<Reservation, BudgetError> {
        let mut state = self.lock()?;
        if state.closed {
            return Err(BudgetError::Closed);
        }
        let would_be = state.charged.plus(&worst_case)?;
        would_be.check_limits(&state.limits)?;
        state.charged = would_be;
        Ok(Reservation { amount: worst_case })
    }

    /// Gives a reservation back when nothing was recorded.
    pub(crate) fn release(&self, reservation: Reservation) -> Result<(), BudgetError> {
        let mut state = self.lock()?;
        state.charged = state.charged.minus(&reservation.amount)?;
        Ok(())
    }

    /// Replaces a reservation with the reported usage of an accepted reply
    ///.
    pub(crate) fn settle(
        &self,
        reservation: Reservation,
        usage: Charge,
    ) -> Result<(), BudgetError> {
        let mut state = self.lock()?;
        state.charged = state.charged.minus(&reservation.amount)?.plus(&usage)?;
        Ok(())
    }

    /// Charges usage above the worst case and closes the budget, because its
    /// bounds can no longer be trusted. The budget closes even when
    /// the usage cannot be charged; the reservation then stays charged.
    pub(crate) fn settle_and_close(
        &self,
        reservation: Reservation,
        usage: Charge,
    ) -> Result<(), BudgetError> {
        let mut state = self.lock()?;
        state.closed = true;
        state.charged = state.charged.minus(&reservation.amount)?.plus(&usage)?;
        Ok(())
    }

    /// Closes the budget when reported usage is too large even to price; the
    /// reservation stays charged.
    pub(crate) fn close(&self) -> Result<(), BudgetError> {
        self.lock()?.closed = true;
        Ok(())
    }

    fn lock(&self) -> Result<MutexGuard<'_, BudgetState>, BudgetError> {
        self.state.lock().map_err(|_| BudgetError::Poisoned)
    }
}

#[cfg(test)]
mod tests;
