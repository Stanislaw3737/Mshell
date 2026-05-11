use super::types::*;
use crate::core::types::Value;
use std::collections::HashMap;
use uuid::Uuid;

// Lightweight stubbed TransactionEngine to remove transactional behavior
// while keeping the public API used across the codebase. Methods are
// intentionally no-ops or return "no active transaction" errors so the
// rest of the runtime continues to function without transactional semantics.

/*use crate::core::transaction::types::{
    TypeIssue, ConstraintViolation, 
    PropagationAnalysis, PropagationPath, 
    PerformanceEstimate, DetailedChange
};*/

#[derive(Debug, Default)]
pub struct TransactionEngine {
    active_transaction: Option<Transaction>,
    transaction_log: Vec<Transaction>,
    max_log_size: usize,
}

impl TransactionEngine {
    pub fn new() -> Self {
        Self { active_transaction: None, transaction_log: Vec::new(), max_log_size: 100 }
    }

    pub fn craft_with_snapshot(&mut self, _name: Option<&str>, _snapshot: Vec<(String, Value)>) -> Result<Uuid, TransactionError> {
        // Transactions are disabled: return a synthetic id but do not activate a transaction.
        Ok(Uuid::new_v4())
    }

    pub fn take_active_transaction(&mut self) -> Result<Transaction, TransactionError> {
        Err(TransactionError::NoActiveTransaction)
    }

    pub fn get_active_transaction_mut(&mut self) -> Result<&mut Transaction, TransactionError> {
        Err(TransactionError::NoActiveTransaction)
    }

    pub fn inspect(&self) -> Result<&Transaction, TransactionError> {
        Err(TransactionError::NoActiveTransaction)
    }

    pub fn temper(&self, _env: &crate::core::env::Env) -> Result<super::types::TransactionPreview, super::types::TransactionError> {
        Err(super::types::TransactionError::NoActiveTransaction)
    }

    pub fn record_transaction(&mut self, _transaction: Transaction) {
        // no-op: keep an extremely short log for compatibility
        if self.transaction_log.len() >= self.max_log_size {
            self.transaction_log.remove(0);
        }
    }

    pub fn has_active_transaction(&self) -> bool {
        false
    }

    pub fn active_transaction_info(&self) -> Option<(Uuid, TransactionState, usize)> {
        None
    }

    pub fn get_transaction_history(&self, _limit: usize) -> Vec<Transaction> {
        Vec::new()
    }

    pub fn forge(&mut self, _env: &mut crate::core::env::Env) -> Result<Vec<String>, TransactionError> {
        Err(TransactionError::NoActiveTransaction)
    }

    pub fn build_evaluation_order(&self, _transaction: &Transaction) -> (Vec<String>, Vec<String>) {
        (Vec::new(), Vec::new())
    }

    pub fn rollback_transaction(&self, _env: &mut crate::core::env::Env, _transaction: &Transaction) -> Result<(), TransactionError> {
        Ok(())
    }

    pub fn what_if(&self, _scenario: &HashMap<String, crate::core::types::Value>, _env: &crate::core::env::Env) -> Result<super::types::ScenarioOutcome, super::types::TransactionError> {
        Err(super::types::TransactionError::NoActiveTransaction)
    }
}