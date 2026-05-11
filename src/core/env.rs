// File: src/core/env.rs
use std::collections::{HashMap, HashSet};
use chrono::Utc;
use uuid::Uuid;

use crate::core::types::{Value, Variable, VariableSource};
use crate::core::expr::{Expr, extract_variables};
use crate::core::propagation::{PropagationEngine, PropagationStrategy};
use crate::core::transaction::TransactionEngine;
use crate::core::types::SimpleType;

//use crate::core::transaction::TransactionPreview;

#[derive(Debug)]
pub struct Env {
    variables: HashMap<String, Variable>,
    expressions: HashMap<String, Expr>,
    dependents: HashMap<String, HashSet<String>>,
    dependencies: HashMap<String, HashSet<String>>,
    propagation_engine: PropagationEngine,
    use_new_engine: bool,
    transaction_engine: TransactionEngine,
}

impl Env {
    pub fn new() -> Self {
        Self {
            variables: HashMap::new(),
            expressions: HashMap::new(),
            dependents: HashMap::new(),
            dependencies: HashMap::new(),
            propagation_engine: PropagationEngine::new(),
            use_new_engine: false,
            transaction_engine: TransactionEngine::new(),
        }
    }
    
    // ==================== TRANSACTION METHODS ====================
    
    pub fn craft(&mut self, name: Option<&str>) -> Result<Uuid, String> {
        // Take snapshot manually
        let snapshot = self.list();
        self.transaction_engine.craft_with_snapshot(name, snapshot)
            .map_err(|e| format!("Transaction error: {:?}", e))
    }
    
    pub fn remove_variable(&mut self, name: &str) {
        self.variables.remove(name);
        self.expressions.remove(name);
        self.dependencies.remove(name);
        
        // Remove from dependents of other variables
        for dependents in self.dependents.values_mut() {
            dependents.remove(name);
        }
        self.dependents.remove(name);
    }
    
    pub fn forge(&mut self) -> Result<Vec<String>, String> {
        // Step 1: Take the transaction out (no double borrow)
        let mut transaction = self.transaction_engine.take_active_transaction()
            .map_err(|e| format!("Forging error: {}", e))?;

        if transaction.is_empty() {
            transaction.state = crate::core::transaction::TransactionState::Forged;
            self.transaction_engine.record_transaction(transaction);
            return Ok(Vec::new());
        }

        // Step 2: Build dependency order
        let (eval_order, circular_deps) = self.transaction_engine.build_evaluation_order(&transaction);
        if !circular_deps.is_empty() {
            transaction.state = crate::core::transaction::TransactionState::Smelted;
            self.transaction_engine.record_transaction(transaction);
            return Err(format!("Circular dependency: {:?}", circular_deps));
        }

        // Step 3: Apply direct values first
        let mut applied = Vec::new();
        let mut failures = Vec::new();
        for var_name in &eval_order {
            if let Some(change) = transaction.changes.get(var_name) {
                if change.expression.is_none() {
                    match self.update_value(var_name, change.new_value.clone()) {
                        Ok(()) => applied.push(var_name.to_string()),
                        Err(e) => failures.push(format!("{}: {}", var_name, e)),
                    }
                }
            }
        }

        // Step 4: Evaluate and apply expressions in order
        for var_name in &eval_order {
            if let Some(change) = transaction.changes.get(var_name) {
                if let Some(expr) = &change.expression {
                    match crate::core::expr::evaluate(expr, self) {
                        Ok(value) => {
                            match self.update_value(var_name, value.clone()) {
                                Ok(()) => applied.push(var_name.to_string()),
                                Err(e) => failures.push(format!("{}: {}", var_name, e)),
                            }
                        }
                        Err(e) => failures.push(format!("Cannot evaluate {}: {}", var_name, e)),
                    }
                }
            }
        }

        if !failures.is_empty() {
            // Rollback: restore all variables in the snapshot
            for (name, value) in &transaction.snapshot {
                let _ = self.update_value(name, value.clone());
            }
            // Remove any variables created during the transaction
            for var_name in transaction.changes.keys() {
                if !transaction.snapshot.contains_key(var_name) {
                    self.remove_variable(var_name);
                }
            }
            transaction.state = crate::core::transaction::TransactionState::Smelted;
            self.transaction_engine.record_transaction(transaction);
            return Err(format!("Forging failed: {}", failures.join(", ")));
        }

        transaction.state = crate::core::transaction::TransactionState::Forged;
        self.transaction_engine.record_transaction(transaction);
        Ok(applied)
    }
    pub fn get_variable_mut(&mut self, name: &str) -> Option<&mut Variable> {
        self.variables.get_mut(name)
    }
    
    // In env.rs, update smelt method
    pub fn smelt(&mut self) -> Result<(), String> {
        let transaction = self.transaction_engine.take_active_transaction()
            .map_err(|e| format!("Smelting error: {:?}", e))?;
        
        // Remove all variables that were created during this transaction
        for var_name in transaction.changes.keys() {
            if !transaction.snapshot.contains_key(var_name) {
                // This variable was created during the transaction
                self.remove_variable(var_name);
            }
        }
        
        // Restore original values for existing variables
        for (var_name, original_value) in &transaction.snapshot {
            let _ = self.update_value(var_name, original_value.clone());
        }
        
        // Record smelted transaction
        let mut smelted_transaction = transaction;
        smelted_transaction.state = crate::core::transaction::TransactionState::Smelted;
        self.transaction_engine.record_transaction(smelted_transaction);
        
        Ok(())
    }
    
    fn restore_from_snapshot(&mut self, snapshot: &HashMap<String, Value>) {
        for (var_name, original_value) in snapshot {
            let _ = self.update_value(var_name, original_value.clone());
        }
    }
    
    pub fn temper(&mut self) -> Result<crate::core::transaction::TransactionPreview, String> {
        match self.transaction_engine.temper(self) {
            Ok(preview) => Ok(preview),
            Err(e) => Err(format!("Tempering error: {:?}", e)),
        }
    }
    
    // File: src/core/env.rs - Update inspect_transaction
    pub fn inspect_transaction(&self) -> Result<String, String> {
        match self.transaction_engine.inspect() {
            Ok(transaction) => {
                let mut output = String::new();
                output.push_str(&format!("[+] Craft: {}\n", transaction.id));
                
                if let Some(name) = &transaction.name {
                    output.push_str(&format!("  Name: {}\n", name));
                }
                
                output.push_str(&format!("  State: {:?}\n", transaction.state));
                output.push_str(&format!("  Created: {}\n", transaction.created_at.format("%H:%M:%S")));
                output.push_str(&format!("  Changes: {}\n", transaction.change_count()));
                
                if !transaction.changes.is_empty() {
                    output.push_str("  Shaped variables:\n");
                    for (i, (var_name, change)) in transaction.changes.iter().enumerate().take(10) {
                        if let Some(ref raw_expr) = change.raw_expression {
                            output.push_str(&format!("    {:2}. {} = {}\n", 
                                i + 1, var_name, raw_expr));
                        } else {
                            output.push_str(&format!("    {:2}. {} = {}\n", 
                                i + 1, var_name, change.new_value.display()));
                        }
                        
                        if !change.dependencies.is_empty() {
                            output.push_str(&format!("        depends on: {}\n", 
                                change.dependencies.join(", ")));
                        }
                    }
                    
                    if transaction.changes.len() > 10 {
                        output.push_str(&format!("    ... and {} more\n", transaction.changes.len() - 10));
                    }
                }
                
                Ok(output)
            }
            Err(e) => Err(format!("Inspection error: {:?}", e)),
        }
    }

     pub fn enable_new_engine(&mut self, strategy: PropagationStrategy) {
        self.use_new_engine = true;
        self.propagation_engine.set_strategy(strategy);
    }
    
    pub fn disable_new_engine(&mut self) {
        self.use_new_engine = false;
    }
    
    pub fn is_new_engine_enabled(&self) -> bool {
        self.use_new_engine
    }
    
    pub fn get_propagation_history(&self, limit: usize) -> Vec<String> {
        if self.use_new_engine {
            self.propagation_engine.get_history(limit)
                .iter()
                .map(|event| {
                    format!("{}: {} -> {} (affected: {})",
                        event.timestamp().format("%H:%M:%S"),
                        event.variable(),
                        event.new_value().display(),
                        event.affected_count()
                    )
                })
                .collect()
        } else {
            vec!["Legacy engine: No detailed history available".to_string()]
        }
    }
    
    pub fn visualize_dependencies(&self) -> String {
        if self.use_new_engine {
            self.propagation_engine.visualize()
        } else {
            self.visualize_legacy_dependencies()
        }
    }
    
    fn visualize_legacy_dependencies(&self) -> String {
        let mut dot = String::from("digraph LegacyDependencies {\n");
        dot.push_str("  rankdir=LR;\n  node [shape=box];\n\n");
        
        for (name, var) in &self.variables {
            let color = if var.is_constant { "lightgray" } else { "white" };
            let style = if var.is_constant { "filled" } else { "solid" };
            
            dot.push_str(&format!(
                "  \"{}\" [label=\"{} = {}\", style={}, fillcolor={}];\n",
                name, name, var.value.display(), style, color
            ));
        }
        
        dot.push_str("\n");
        
        for (target, deps) in &self.dependencies {
            for source in deps {
                dot.push_str(&format!(
                    "  \"{}\" -> \"{}\";\n",
                    source, target
                ));
            }
        }
        
        dot.push_str("}\n");
        dot
    }
    
    pub fn migrate_to_new_engine(&mut self) -> Result<(), String> {
        self.use_new_engine = true;
        self.propagation_engine.clear();
        
        // First pass: register all direct variables
        for (name, var) in &self.variables {
            if let Some(_expr_str) = &var.expression {
                // Skip computed variables for now
                continue;
            } else {
                // Register as direct variable with actual value
                if let Err(e) = self.propagation_engine.register_direct_variable(
                    name, var.value.clone(), var.is_constant
                ) {
                    return Err(format!("Failed to migrate {}: {:?}", name, e));
                }
            }
        }
        
        // Second pass: register computed variables
        for (name, var) in &self.variables {
            if let Some(expr_str) = &var.expression {
                // Parse the expression
                match crate::core::expr::parse_expression(expr_str) {
                    Ok(expr) => {
                        // Try to evaluate with current environment
                        match crate::core::expr::evaluate(&expr, self) {
                            Ok(evaluated_value) => {
                                // Register with evaluated value
                                if let Err(e) = self.propagation_engine.register_computed_variable(
                                    name, evaluated_value, &expr
                                ) {
                                    return Err(format!("Failed to migrate {}: {:?}", name, e));
                                }
                            }
                            Err(_) => {
                                // Can't evaluate, register with current value
                                if let Err(e) = self.propagation_engine.register_computed_variable(
                                    name, var.value.clone(), &expr
                                ) {
                                    return Err(format!("Failed to migrate {}: {:?}", name, e));
                                }
                            }
                        }
                    }
                    Err(e) => {
                        return Err(format!("Failed to parse expression for {}: {}", name, e));
                    }
                }
            }
        }
        
        Ok(())
    }
    
    pub fn anneal(&mut self, steps: usize) -> Result<Vec<String>, String> {
        // Get the variable names first to avoid borrowing issues
        let var_names: Vec<String> = {
            let transaction = self.transaction_engine.get_active_transaction_mut()
                .map_err(|e| format!("Annealing error: {:?}", e))?;
            let mut names: Vec<String> = transaction.changes.keys().cloned().collect();
            names.sort();  // Sort alphabetically for now
            names
        };
        
        let mut applied = Vec::new();
        let step_limit = steps.min(var_names.len());
        
        for i in 0..step_limit {
            let var_name = &var_names[i];
            
            // Get the change value from transaction
            let change_value = {
                let transaction = self.transaction_engine.get_active_transaction_mut()
                    .map_err(|e| format!("Annealing error: {:?}", e))?;
                transaction.changes.get(var_name).map(|c| c.new_value.clone())
            };
            
            if let Some(value) = change_value {
                match self.update_value(var_name, value) {
                    Ok(()) => {
                        applied.push(var_name.clone());
                        
                        // Propagate
                        if self.use_new_engine {
                            let _ = self.propagate_from_enhanced(var_name);
                        }
                        
                        // Remove from transaction after applying
                        let transaction = self.transaction_engine.get_active_transaction_mut()
                            .map_err(|e| format!("Annealing error: {:?}", e))?;
                        transaction.changes.remove(var_name);
                    }
                    Err(e) => {
                        return Err(format!("Failed to anneal {}: {}", var_name, e));
                    }
                }
            }
        }
        
        Ok(applied)
    }
    
    pub fn quench(&mut self) -> Result<Vec<String>, String> {
        let transaction = self.transaction_engine.take_active_transaction()
            .map_err(|e| format!("Quenching error: {:?}", e))?;
        
        // Fast commit - apply changes immediately without full propagation
        let mut applied = Vec::new();
        
        for (var_name, change) in &transaction.changes {
            if let Ok(()) = self.update_value(var_name, change.new_value.clone()) {
                applied.push(var_name.clone());
            }
        }
        
        // Record quenched transaction
        let mut quenched_transaction = transaction;
        quenched_transaction.state = crate::core::transaction::TransactionState::Quenched;
        self.transaction_engine.record_transaction(quenched_transaction);
        
        Ok(applied)
    }
    
    pub fn transaction_status(&self) -> String {
        if let Some((id, state, count)) = self.transaction_engine.active_transaction_info() {
            let state_str = match state {
                crate::core::transaction::TransactionState::Crafting => "🛠 Crafting".to_string(),
                crate::core::transaction::TransactionState::Tempered => "🧪 Tempered".to_string(),
                crate::core::transaction::TransactionState::Annealing(step) => format!("🔥 Annealing (step {})", step),
                crate::core::transaction::TransactionState::Forged => "✅ Forged".to_string(),
                crate::core::transaction::TransactionState::Smelted => "❌ Smelted".to_string(),
                crate::core::transaction::TransactionState::Quenched => "⚡ Quenched".to_string(),
                crate::core::transaction::TransactionState::Polishing => "✨ Polishing".to_string(),
            };
            
            format!("[{}] Transaction active: {} ({} changes)", 
                state_str, id, count)
        } else {
            "[ ] No active transaction".to_string()
        }
    }
    
    pub fn has_active_transaction(&self) -> bool {
        self.transaction_engine.has_active_transaction()
    }
    
    pub fn get_transaction_history(&self, limit: usize) -> Vec<String> {
        let history = self.transaction_engine.get_transaction_history(limit);
        let mut output = Vec::new();
        
        for transaction in history {
            let state_str = match transaction.state {
                crate::core::transaction::TransactionState::Forged => "✅",
                crate::core::transaction::TransactionState::Smelted => "❌",
                crate::core::transaction::TransactionState::Quenched => "⚡",
                _ => "📝",
            };
            
            let name = transaction.name.as_deref().unwrap_or("unnamed");
            let time = transaction.created_at.format("%H:%M:%S");
            
            output.push(format!("{} {} @ {} ({} changes)", 
                state_str, name, time, transaction.change_count()));
        }
        
        output
    }
    
    // ==================== VARIABLE METHODS ====================
    
   pub fn set_computed_with_type(&mut self, name: &str, value: Value, expr: &Expr, declared_type: Option<SimpleType>) {
        // If we have an active transaction, record the change
        if self.has_active_transaction() {
            let old_value = self.get_value(name)
                .cloned()
                .unwrap_or(Value::Str("<?>".to_string()));
            
            // Try to record in transaction
            if let Ok(transaction) = self.transaction_engine.get_active_transaction_mut() {
                let raw_expr = Some(expr.to_string());  
                let dependencies = extract_variables(expr);
                
                transaction.add_change_with_raw_expr(
                    name.to_string(),
                    old_value,
                    Value::Str("<?>".to_string()),  // Placeholder
                    Some(expr.clone()),
                    raw_expr.clone(),  // Store the raw expression
                    dependencies.clone()
                );
                
                // CRITICAL: Actually store the variable in the environment!
                self.variables.insert(name.to_string(), Variable::new_with_type(
                    Value::Str("<?>".to_string()),  // Placeholder during transaction
                    false,
                    raw_expr.clone(),
                    VariableSource::Computed,
                    declared_type  // Store declared type
                ));
                
                // Store the actual expression for retrieval
                self.expressions.insert(name.to_string(), expr.clone());
                
                // Track dependencies
                self.dependencies.insert(name.to_string(), dependencies.clone().into_iter().collect());
                
                for dep in dependencies {
                    self.dependents.entry(dep)
                        .or_insert_with(HashSet::new)
                        .insert(name.to_string());
                }
                
                return;
            }
        }
        
        // Non-transaction case - make sure this actually stores the variable
        self.remove_dependencies(name);

        self.variables.insert(name.to_string(), Variable::new_with_type(
            value.clone(),
            false,
            Some(expr.to_string()),
            VariableSource::Computed,
            declared_type
        ));

        self.expressions.insert(name.to_string(), expr.clone());

        let mut deps: HashSet<String> = extract_variables(expr).into_iter().collect();

        if deps.contains(name) {
            deps.remove(name);
        }

        self.dependencies.insert(name.to_string(), deps.clone());

        for dep in deps {
            self.dependents.entry(dep)
                .or_insert_with(HashSet::new)
                .insert(name.to_string());
        }
        
        // Propagation should be triggered by the caller to avoid double-invocation.
    }

    // Similarly fix set_direct_with_type:
    pub fn set_direct_with_type(&mut self, name: &str, value: Value, declared_type: Option<SimpleType>) {
        // If we have an active transaction, record the change
        if self.has_active_transaction() {
            let old_value = self.get_value(name)
                .cloned()
                .unwrap_or(Value::Str("<?>".to_string()));
            
            // Try to record in transaction
            if let Ok(transaction) = self.transaction_engine.get_active_transaction_mut() {
                let dependencies = if let Some(expr) = self.expressions.get(name) {
                    extract_variables(expr)
                } else {
                    Vec::new()
                };
                
                transaction.add_change(  // CORRECT METHOD - 5 arguments
                    name.to_string(),
                    old_value,
                    value.clone(),
                    None,  // No expression for direct values
                    dependencies  // Use the dependencies variable
                );  

                // CRITICAL: Actually store the variable!
                self.variables.insert(name.to_string(), Variable::new_with_type(
                    Value::Str("<?>".to_string()),  // Placeholder during transaction
                    false,
                    None,
                    VariableSource::Direct,
                    declared_type  // Store declared type
                ));
                
                return;
            }
        }
        
        // Non-transaction case - make sure this actually stores the variable
        self.remove_dependencies(name);
        
        self.variables.insert(name.to_string(), Variable::new_with_type(
            value.clone(),
            false,
            None,
            VariableSource::Direct,
            declared_type
        ));
        
        self.expressions.remove(name);
        
        // Intentionally do not call into `propagation_engine` here for `set`.
    }

    // Convenience methods for backward compatibility
    pub fn set_computed(&mut self, name: &str, value: Value, expr: &Expr) {
        self.set_computed_with_type(name, value, expr, None);
    }
    
    pub fn set_direct(&mut self, name: &str, value: Value) {
        self.set_direct_with_type(name, value, None);
    }
    
    fn remove_dependencies(&mut self, name: &str) {
        if let Some(old_deps) = self.dependencies.get(name) {
            for dep in old_deps {
                if let Some(dependents) = self.dependents.get_mut(dep) {
                    dependents.remove(name);
                }
            }
        }
        
        self.dependencies.remove(name);
        self.expressions.remove(name);
    }

    
    pub fn freeze(&mut self, name: &str) -> Result<(), String> {
        if let Some(var) = self.variables.get_mut(name) {
            var.is_constant = true;
            
            if self.use_new_engine {
                if let Err(e) = self.propagation_engine.freeze_variable(name) {
                    return Err(format!("Failed to freeze in propagation engine: {:?}", e));
                }
            }
            
            Ok(())
        } else {
            Err(format!("Variable '{}' not found", name))
        }
    }
    
    pub fn get_value(&self, name: &str) -> Option<&Value> {
        self.variables.get(name).map(|v| &v.value)
    }
    
    pub fn get_variable(&self, name: &str) -> Option<&Variable> {
        self.variables.get(name)
    }
    
    pub fn list(&self) -> Vec<(String, Value)> {
        self.variables
            .iter()
            .map(|(k, v)| (k.clone(), v.value.clone()))
            .collect()
    }
    
    pub fn get_dependents(&self, name: &str) -> Vec<String> {
        if self.use_new_engine {
            self.propagation_engine.graph().get_direct_dependents(name)
        } else {
            self.dependents.get(name)
                .map(|set| set.iter().cloned().collect())
                .unwrap_or_default()
        }
    }
    
    pub fn get_dependencies(&self, name: &str) -> Vec<String> {
        if self.use_new_engine {
            self.propagation_engine.graph().get_direct_dependencies(name)
        } else {
            self.dependencies.get(name)
                .map(|set| set.iter().cloned().collect())
                .unwrap_or_default()
        }
    }
    
    pub fn get_expression(&self, name: &str) -> Option<&Expr> {
        self.expressions.get(name)
    }
    
    pub fn propagate_from_enhanced(&mut self, changed_var: &str) -> Result<Vec<String>, String> {
        if !self.use_new_engine {
            return self.propagate_from_legacy(changed_var);
        }
        
        // Check if we're in a transaction
        if self.has_active_transaction() {
            // During transaction, just record the propagation path
            if let Ok(transaction) = self.transaction_engine.get_active_transaction_mut() {
                transaction.propagation_paths.push(vec![changed_var.to_string()]);
            }
            return Ok(vec![changed_var.to_string()]);
        }
        
        // Original propagation logic (outside transaction)
        let current_value = self.get_value(changed_var)
            .cloned()
            .ok_or_else(|| format!("Variable '{}' not found", changed_var))?;
        
        match self.propagation_engine.set_variable(changed_var, current_value) {
            Ok(result) => {
                let mut actually_updated = Vec::new();
                
                for var_name in &result.changed_variables {
                    // Check propagation control before updating
                    if let Some(var) = self.variables.get_mut(var_name) {
                        if var.should_propagate() {
                            if let Some(new_value) = self.propagation_engine.get_value(var_name) {
                                var.value = new_value.clone();
                                var.source = VariableSource::Propagated;
                                var.last_updated = Utc::now();
                                var.update_count += 1;
                                actually_updated.push(var_name.to_string());
                            }
                        }
                    }
                }
                
                Ok(actually_updated)
            }
            Err(e) => {
                eprintln!("New propagation engine failed: {:?}, falling back to legacy", e);
                self.propagate_from_legacy(changed_var)
            }
        }
    }
    
    fn propagate_from_legacy(&mut self, changed_var: &str) -> Result<Vec<String>, String> {
        use crate::core::propagate::propagate_from;
        propagate_from(self, changed_var)
    }
    
    /*pub fn visualize_dependencies(&self) -> String {
        if self.use_new_engine {
            self.propagation_engine.visualize()
        } else {
            self.visualize_legacy_dependencies()
        }
    }*/
    
    /*fn visualize_legacy_dependencies(&self) -> String {
        let mut dot = String::from("digraph LegacyDependencies {\n");
        dot.push_str("  rankdir=LR;\n  node [shape=box];\n\n");
        
        for (name, var) in &self.variables {
            let color = if var.is_constant { "lightgray" } else { "white" };
            let style = if var.is_constant { "filled" } else { "solid" };
            
            dot.push_str(&format!(
                "  \"{}\" [label=\"{} = {}\", style={}, fillcolor={}];\n",
                name, name, var.value.display(), style, color
            ));
        }
        
        dot.push_str("\n");
        
        for (target, deps) in &self.dependencies {
            for source in deps {
                dot.push_str(&format!(
                    "  \"{}\" -> \"{}\";\n",
                    source, target
                ));
            }
        }
        
        dot.push_str("}\n");
        dot
    }*/
    
    /*pub fn get_propagation_history(&self, limit: usize) -> Vec<String> {
        if self.use_new_engine {
            self.propagation_engine.get_history(limit)
                .iter()
                .map(|event| {
                    format!("{}: {} -> {} (affected: {})",
                        event.timestamp().format("%H:%M:%S"),
                        event.variable(),
                        event.new_value().display(),
                        event.affected_count()
                    )
                })
                .collect()
        } else {
            vec!["Legacy engine: No detailed history available".to_string()]
        }
    }*/

    /*pub fn migrate_to_new_engine(&mut self) -> Result<(), String> {
        self.use_new_engine = true;
        self.propagation_engine.clear();
        
        // First pass: register all direct variables
        for (name, var) in &self.variables {
            if let Some(_expr_str) = &var.expression {
                // Skip computed variables for now
                continue;
            } else {
                // Register as direct variable with actual value
                if let Err(e) = self.propagation_engine.register_direct_variable(
                    name, var.value.clone(), var.is_constant
                ) {
                    return Err(format!("Failed to migrate {}: {:?}", name, e));
                }
            }
        }
        
        // Second pass: register computed variables
        for (name, var) in &self.variables {
            if let Some(expr_str) = &var.expression {
                // Parse the expression
                match crate::core::expr::parse_expression(expr_str) {
                    Ok(expr) => {
                        // Try to evaluate with current environment
                        match crate::core::expr::evaluate(&expr, self) {
                            Ok(evaluated_value) => {
                                // Register with evaluated value
                                if let Err(e) = self.propagation_engine.register_computed_variable(
                                    name, evaluated_value, &expr
                                ) {
                                    return Err(format!("Failed to migrate {}: {:?}", name, e));
                                }
                            }
                            Err(_) => {
                                // Can't evaluate, register with current value
                                if let Err(e) = self.propagation_engine.register_computed_variable(
                                    name, var.value.clone(), &expr
                                ) {
                                    return Err(format!("Failed to migrate {}: {:?}", name, e));
                                }
                            }
                        }
                    }
                    Err(e) => {
                        return Err(format!("Failed to parse expression for {}: {}", name, e));
                    }
                }
            }
        }
        
        Ok(())
    }*/ 
    
    pub fn batch_update(&mut self, updates: Vec<(&str, Value)>) -> Result<Vec<String>, String> {
        if !self.use_new_engine {
            return Err("Batch updates require the new propagation engine".to_string());
        }
        
        let mut all_changed = Vec::new();
        let updates_clone = updates.clone();
        
        for (name, value) in updates {
            if let Some(var) = self.variables.get_mut(name) {
                if var.is_constant {
                    return Err(format!("Variable '{}' is frozen", name));
                }
                var.value = value.clone();
                var.source = VariableSource::Direct;
                var.last_updated = Utc::now();
                var.update_count += 1;
            } else {
                return Err(format!("Variable '{}' not found", name));
            }
        }
        
        for (name, _) in updates_clone {
            match self.propagate_from_enhanced(name) {
                Ok(changed) => {
                    all_changed.extend(changed);
                }
                Err(e) => {
                    return Err(format!("Failed to propagate from {}: {}", name, e));
                }
            }
        }
        
        all_changed.sort();
        all_changed.dedup();
        Ok(all_changed)
    }

    pub fn what_if(&mut self, scenario: HashMap<String, crate::core::types::Value>) -> Result<crate::core::transaction::ScenarioOutcome, String> {
        match self.transaction_engine.what_if(&scenario, self) {
            Ok(outcome) => Ok(outcome),
            Err(e) => Err(format!("What-if analysis failed: {:?}", e)),
        }
    }

    pub fn set_computed_with_propagation(
        &mut self, 
        name: &str, 
        value: Value, 
        expr: &Expr, 
        declared_type: Option<SimpleType>,
        delay: usize,
        limit: usize,
    ) {
        // If we have an active transaction, record the change
        
        if self.has_active_transaction() {
            let old_value = self.get_value(name)
                .cloned()
                .unwrap_or(Value::Str("<?>".to_string()));
            
            // Try to record in transaction
            if let Ok(transaction) = self.transaction_engine.get_active_transaction_mut() {
                let raw_expr = Some(expr.to_string());  
                let dependencies = extract_variables(expr);
                
                transaction.add_change_with_raw_expr(
                    name.to_string(),
                    old_value,
                    Value::Str("<?>".to_string()),  // Placeholder
                    Some(expr.clone()),
                    raw_expr.clone(),  // Store the raw expression
                    dependencies.clone()
                );
                
                // Store in main environment with propagation controls
                self.variables.insert(name.to_string(), Variable::new_with_propagation(
                    Value::Str("<?>".to_string()),  // Placeholder during transaction
                    false,
                    raw_expr.clone(),
                    VariableSource::Computed,
                    declared_type,
                    delay,    // Propagation delay
                    limit,    // Propagation limit
                ));
                
                // Store the actual expression for retrieval
                self.expressions.insert(name.to_string(), expr.clone());
                
                // Also track dependencies in main environment
                self.dependencies.insert(name.to_string(), dependencies.clone().into_iter().collect());
                
                for dep in dependencies {
                    self.dependents.entry(dep)
                        .or_insert_with(HashSet::new)
                        .insert(name.to_string());
                }
                
                return;
            }
        }
        
        // Non-transaction case - make sure this actually stores the variable
        self.remove_dependencies(name);

        self.variables.insert(name.to_string(), Variable::new_with_propagation(
            value.clone(),
            false,
            Some(expr.to_string()),
            VariableSource::Computed,
            declared_type,
            delay,    // Propagation delay
            limit,    // Propagation limit
        ));

        self.expressions.insert(name.to_string(), expr.clone());

        let mut deps: HashSet<String> = extract_variables(expr).into_iter().collect();

        if deps.contains(name) {
            deps.remove(name);
        }

        self.dependencies.insert(name.to_string(), deps.clone());

        for dep in deps {
            self.dependents.entry(dep)
                .or_insert_with(HashSet::new)
                .insert(name.to_string());
        }
        
        if self.use_new_engine {
            if let Err(e) = self.propagation_engine.register_computed_variable(name, value.clone(), expr) {
                eprintln!("Warning: Failed to register with new propagation engine: {:?}", e);
            }
        }
    }
    
    // Enhanced set_direct with propagation control
    pub fn set_direct_with_propagation(
        &mut self, 
        name: &str, 
        value: Value, 
        declared_type: Option<SimpleType>,
        delay: usize,
        limit: usize,
    ) {
        // If we have an active transaction, record the change
        if self.has_active_transaction() {
            let old_value = self.get_value(name)
                .cloned()
                .unwrap_or(Value::Str("<?>".to_string()));
            
            // Try to record in transaction
            if let Ok(transaction) = self.transaction_engine.get_active_transaction_mut() {
                let dependencies = if let Some(expr) = self.expressions.get(name) {
                    extract_variables(expr)
                } else {
                    Vec::new()
                };
                
                transaction.add_change(  // CORRECT METHOD - 5 arguments
                    name.to_string(),
                    old_value,
                    value.clone(),
                    None,  // No expression for direct values
                    dependencies  // Use the dependencies variable
                );  

                // Store with propagation controls
                self.variables.insert(name.to_string(), Variable::new_with_propagation(
                    Value::Str("<?>".to_string()),  // Placeholder during transaction
                    false,
                    None,
                    VariableSource::Direct,
                    declared_type,
                    delay,    // Propagation delay
                    limit,    // Propagation limit
                ));
                
                return;
            }
        }
        
        // Non-transaction case
        self.remove_dependencies(name);
        
        self.variables.insert(name.to_string(), Variable::new_with_propagation(
            value.clone(),
            false,
            None,
            VariableSource::Direct,
            declared_type,
            delay,    // Propagation delay
            limit,    // Propagation limit
        ));
        
        self.expressions.remove(name);
        
        // Propagation should be triggered by the caller to avoid double-invocation.
    }

    // Public helper: propagate from a changed variable using the active engine.
    pub fn propagate_from_env(&mut self, changed_var: &str) -> Result<Vec<String>, String> {
        if self.use_new_engine {
            self.propagate_from_enhanced(changed_var)
        } else {
            self.propagate_from_legacy(changed_var)
        }
    }
    
    // Enhanced update_value to respect propagation control
    pub fn update_value(&mut self, name: &str, value: Value) -> Result<(), String> {
        // If in transaction, defer actual update (just update local copy)
        if self.has_active_transaction() {
            if let Some(var) = self.variables.get_mut(name) {
                if var.is_constant {
                    return Err(format!("Variable '{}' is frozen", name));
                }
                var.value = value.clone();
                var.source = VariableSource::Propagated;
                var.last_updated = Utc::now();
                var.update_count += 1;
                return Ok(());
            } else {
                return Err(format!("Variable '{}' not found", name));
            }
        }
        
        // Original behavior (outside transaction)
        if let Some(var) = self.variables.get_mut(name) {
            if var.is_constant {
                return Err(format!("Variable '{}' is frozen", name));
            }
            
            // Don't check propagation control here - it should be checked by the propagation logic
            // The propagation logic has already determined that this update should happen
            var.value = value.clone();
            var.source = VariableSource::Propagated;
            var.last_updated = Utc::now();
            var.update_count += 1;
            Ok(())
        } else {
            Err(format!("Variable '{}' not found", name))
        }
    }

    pub fn update_value_without_propagation_check(&mut self, name: &str, value: Value) -> Result<(), String> {
        if let Some(var) = self.variables.get_mut(name) {
            if var.is_constant {
                return Err(format!("Variable '{}' is frozen", name));
            }
            
            // Update without propagation check (for use by propagation logic)
            var.value = value.clone();
            var.source = VariableSource::Propagated;
            var.last_updated = Utc::now();
            var.update_count += 1;
            Ok(())
        } else {
            Err(format!("Variable '{}' not found", name))
        }
    }

}

