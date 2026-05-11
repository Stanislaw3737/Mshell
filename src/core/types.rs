use std::collections::HashMap;
use serde::{Serialize, Deserialize};
use chrono::{DateTime, Utc};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Value {
    Str(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    List(Vec<Value>),
    Dict(HashMap<String, Value>),
    //Array(Vec<Value>),
    Json(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum VariableSource {
    Direct,     // set! command
    Computed,   // set x = y + z
    Propagated, // Changed because dependency changed
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SimpleType {
    String,
    Integer,
    Float,
    Boolean,
    List,
    Dictionary,
    Json,
    Any, // For flexible typing
}

impl SimpleType {
    pub fn name(&self) -> &'static str {
        match self {
            SimpleType::String => "string",
            SimpleType::Integer => "int",
            SimpleType::Float => "float", 
            SimpleType::Boolean => "bool",
            SimpleType::List => "list",
            SimpleType::Dictionary => "dict",
            SimpleType::Json => "json",
            SimpleType::Any => "any",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Variable {
    pub value: Value,
    pub is_constant: bool,
    pub expression: Option<String>,
    pub source: VariableSource,
    pub last_updated: DateTime<Utc>,
    pub update_count: u64, 
    #[serde(skip_serializing_if = "Option::is_none")]
    pub declared_type: Option<SimpleType>,
    // NEW: Propagation control fields
    #[serde(default)]
    pub propagation_delay: usize,     // ~-N: ignore first N propagations
    #[serde(default = "usize_max")]
    pub propagation_limit: usize,     // ~+N: become immune after N propagations
    #[serde(default)]
    pub delay_counter: usize,         // Tracks ignored propagations
    #[serde(default)]
    pub limit_counter: usize,         // Tracks successful propagations
}


fn usize_max() -> usize {
    usize::MAX
}

impl Variable {

    pub fn new_with_propagation(
        value: Value,
        is_constant: bool,
        expression: Option<String>,
        source: VariableSource,
        declared_type: Option<SimpleType>,
        delay: usize,    // ~-N: delay count
        limit: usize,    // ~+N: limit count (use usize::MAX for no limit)
    ) -> Self {
        Self {
            value,
            is_constant,
            expression,
            source,
            last_updated: Utc::now(),
            update_count: 0,
            declared_type,
            propagation_delay: delay,
            propagation_limit: limit,
            delay_counter: 0,         // Fix: use correct field name
            limit_counter: 0,         // Fix: use correct field name
        }
    }
    
    // Existing constructor with defaults
    pub fn new_with_type(
        value: Value,
        is_constant: bool,
        expression: Option<String>,
        source: VariableSource,
        declared_type: Option<SimpleType>,
    ) -> Self {
        // Default for `set` (non-reactive) should disable propagation by setting limit=0.
        // `ensure` uses `new_with_propagation` to enable propagation with explicit parameters.
        Self::new_with_propagation(value, is_constant, expression, source, declared_type, 0, 0)
    }
    
    // Keep existing constructor for backward compatibility
    pub fn new(
        value: Value, 
        is_constant: bool, 
        expression: Option<String>, 
        source: VariableSource
    ) -> Self {
        Self::new_with_type(value, is_constant, expression, source, None)
    }
    
    // NEW: Get effective type (declared type takes precedence)
    pub fn get_effective_type(&self) -> &str {
        if let Some(ref declared) = self.declared_type {
            declared.name()
        } else {
            self.value.type_name()
        }
    }

    pub fn should_propagate(&mut self) -> bool {
        // Constants never propagate
        if self.is_constant {
            return false;
        }
        
        // Handle delay (~-N: ignore first N changes)
        if self.delay_counter < self.propagation_delay {
            self.delay_counter += 1;
            return false;
        }
        
        // Handle limit (~+N: become immune after N changes)
        if self.propagation_limit != usize::MAX && self.limit_counter >= self.propagation_limit {
            return false;
        }
        
        // Increment propagation counter and allow propagation
        self.limit_counter += 1;
        true
    }
    
    // Reset counters (useful for manual control)
    pub fn reset_propagation_counters(&mut self) {
        self.delay_counter = 0;
        self.limit_counter = 0;
    }

    pub fn propagation_status(&self) -> String {
    if self.is_constant {
        "frozen".to_string()
    } else if self.delay_counter < self.propagation_delay {
        format!("delayed ({}/{})", self.delay_counter, self.propagation_delay)
    } else if self.propagation_limit != usize::MAX && self.limit_counter >= self.propagation_limit {
        format!("immune ({}/{})", self.limit_counter, self.propagation_limit)
    } else {
        format!("active (delay: {}/{}, limit: {}/{})", 
               self.delay_counter, self.propagation_delay,
               self.limit_counter, self.propagation_limit)
    }
}
}

impl Value {
    pub fn type_name(&self) -> &str {
        match self {
            Value::Str(_) => "string",
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::Bool(_) => "bool",
            Value::List(_) => "list",
            Value::Dict(_) => "dict",
            Value::Json(_) => "json",
        }
    }

    pub fn declared_type(&self) -> Option<&str> {
        // This will be populated when we parse type annotations
        None // Placeholder for now
    }

    pub fn to_string(&self) -> String {
        match self {
            Value::Str(s) => s.clone(),
            Value::Int(i) => i.to_string(),
            Value::Float(f) => {
                let s = format!("{}", f);
                if s.contains('.') {
                    s.trim_end_matches('0').trim_end_matches('.').to_string()
                } else {
                    s
                }
            },
            Value::Bool(b) => b.to_string(),
            Value::List(items) => {
                let item_strings: Vec<String> = items.iter().map(|item| item.display()).collect();
                format!("[{}]", item_strings.join(", "))
            },
            Value::Dict(map) => {
                let pairs: Vec<String> = map.iter()
                    .map(|(k, v)| format!("\"{}\": {}", k, v.display()))
                    .collect();
                format!("{{{}}}", pairs.join(", "))
            },
            /*Value::Array(items) => {
                let item_strings: Vec<String> = items.iter().map(|item| item.display()).collect();
                format!("[{}]", item_strings.join(", "))
            },*/
            Value::Json(json_str) => json_str.clone(),
        }
    }

    pub fn display(&self) -> String {
        match self {
            Value::Str(s) => format!("\"{}\"", s),
            Value::Int(i) => i.to_string(),
            Value::Float(f) => {
                let s = format!("{}", f);
                if s.contains('.') {
                    s.trim_end_matches('0').trim_end_matches('.').to_string()
                } else {
                    s
                }
            },
            Value::Bool(b) => b.to_string(),
            Value::List(items) => {
                let item_strings: Vec<String> = items.iter().map(|item| item.display()).collect();
                format!("[{}]", item_strings.join(", "))
            },
            Value::Dict(map) => {
                let pairs: Vec<String> = map.iter()
                    .map(|(k, v)| format!("\"{}\": {}", k, v.display()))
                    .collect();
                format!("{{{}}}", pairs.join(", "))
            },
            /*Value::Array(items) => {
                let item_strings: Vec<String> = items.iter().map(|item| item.display()).collect();
                format!("[{}]", item_strings.join(", "))
            },*/
            Value::Json(json_str) => format!("json!{}", json_str),
        }
    }
}


impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Str(s) => write!(f, "\"{}\"", s),
            Value::Int(i) => write!(f, "{}", i),
            Value::Float(fl) => {
                let s = format!("{}", fl);
                if s.contains('.') {
                    write!(f, "{}", s.trim_end_matches('0').trim_end_matches('.'))
                } else {
                    write!(f, "{}", s)
                }
            },
            Value::Bool(b) => write!(f, "{}", b),
            Value::List(items) => {
                write!(f, "[")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{}", item.display())?;
                }
                write!(f, "]")
            },
            Value::Dict(map) => {
                write!(f, "{{")?;
                for (i, (key, value)) in map.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "\"{}\": {}", key, value.display())?;
                }
                write!(f, "}}")
            },
            Value::Json(json_str) => write!(f, "json!{}", json_str),
        }
    }
}
