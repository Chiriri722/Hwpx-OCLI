//! Overflow-safe resource accounting shared by Hancom parsers and emitters.

use crate::error::{PluginError, Result};

/// A monotonic byte/item budget that commits usage only after validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceBudget {
    label: &'static str,
    limit: u64,
    used: u64,
}

impl ResourceBudget {
    pub const fn new(label: &'static str, limit: u64) -> Self {
        Self {
            label,
            limit,
            used: 0,
        }
    }

    pub const fn used(&self) -> u64 {
        self.used
    }

    pub const fn limit(&self) -> u64 {
        self.limit
    }

    pub const fn remaining(&self) -> u64 {
        self.limit - self.used
    }

    pub fn consume(&mut self, amount: u64) -> Result<()> {
        let next = self.used.checked_add(amount).ok_or_else(|| {
            PluginError::corrupt(format!("resource limit exceeded: {} overflow", self.label))
        })?;
        if next > self.limit {
            return Err(PluginError::corrupt(format!(
                "resource limit exceeded: {} {next} exceeds maximum {}",
                self.label, self.limit
            )));
        }
        self.used = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic_overflow_is_rejected_without_mutation() {
        let mut budget = ResourceBudget::new("test bytes", u64::MAX);
        budget.consume(u64::MAX).expect("exact maximum");
        assert!(budget.consume(1).is_err());
        assert_eq!(budget.used(), u64::MAX);
    }
}
