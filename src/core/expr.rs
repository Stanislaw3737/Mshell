use std::collections::HashSet;
use crate::core::types::Value;
use crate::core::env::Env;
use crate::core::builtins;
use std::collections::HashMap;
use crate::core::types::SimpleType;
use crate::core::template::render_template;

#[derive(Debug, Clone)]
pub enum Expr {
    Literal(Value),
    Variable(String),

    List(Vec<Expr>),
    Dict(HashMap<String, Expr>),
    IndexAccess(Box<Expr>, Box<Expr>), 
    MethodCall(Box<Expr>, String, Vec<Expr>),

    Add(Box<Expr>, Box<Expr>),
    Subtract(Box<Expr>, Box<Expr>),
    Multiply(Box<Expr>, Box<Expr>),
    Divide(Box<Expr>, Box<Expr>),
    FunctionCall(String, Vec<Expr>),
    Conditional(Vec<ConditionalBranch>),
    // NEW: Comparison operators
    GreaterThan(Box<Expr>, Box<Expr>),
    GreaterThanOrEqual(Box<Expr>, Box<Expr>),
    LessThan(Box<Expr>, Box<Expr>),
    LessThanOrEqual(Box<Expr>, Box<Expr>),
    Equal(Box<Expr>, Box<Expr>),
    NotEqual(Box<Expr>, Box<Expr>),
    // NEW: Logical operators
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Not(Box<Expr>),
}

#[derive(Debug, Clone)]
pub struct ConditionalBranch {
    pub value: Box<Expr>,
    pub condition: Option<Box<Expr>>,
}

impl std::fmt::Display for Expr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Expr::Literal(Value::Str(s)) => write!(f, "\"{}\"", s),
            Expr::Literal(Value::Int(i)) => write!(f, "{}", i),
            Expr::Literal(Value::Float(fl)) => write!(f, "{}", fl),
            Expr::Literal(Value::Bool(b)) => write!(f, "{}", b),
            Expr::Literal(Value::List(items)) => {
                write!(f, "[")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{}", item)?;
                }
                write!(f, "]")
            },
            Expr::Literal(Value::Dict(map)) => {
                write!(f, "{{")?;
                for (i, (key, value)) in map.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "\"{}\": {}", key, value)?;
                }
                write!(f, "}}")
            },
            Expr::Variable(name) => write!(f, "{}", name),
            Expr::Add(left, right) => write!(f, "({} + {})", left, right),
            Expr::Subtract(left, right) => write!(f, "({} - {})", left, right),
            Expr::Multiply(left, right) => write!(f, "({} * {})", left, right),
            Expr::Divide(left, right) => write!(f, "({} / {})", left, right),
            Expr::GreaterThan(left, right) => write!(f, "({} > {})", left, right),
            Expr::GreaterThanOrEqual(left, right) => write!(f, "({} >= {})", left, right),
            Expr::LessThan(left, right) => write!(f, "({} < {})", left, right),
            Expr::LessThanOrEqual(left, right) => write!(f, "({} <= {})", left, right),
            Expr::Equal(left, right) => write!(f, "({} == {})", left, right),
            Expr::NotEqual(left, right) => write!(f, "({} != {})", left, right),
            Expr::And(left, right) => write!(f, "({} and {})", left, right),
            Expr::Or(left, right) => write!(f, "({} or {})", left, right),
            Expr::Not(expr) => write!(f, "(not {})", expr),
            Expr::FunctionCall(name, args) => {
                write!(f, "{}(", name)?;
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{}", arg)?;
                }
                write!(f, ")")
            }
            Expr::Conditional(branches) => {
                for (i, branch) in branches.iter().enumerate() {
                    if i > 0 {
                        write!(f, " | ")?;
                    }
                    write!(f, "{}", branch)?;
                }
                Ok(())
            }
            Expr::List(items) => {
                write!(f, "[")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{}", item)?;
                }
                write!(f, "]")
            }
            Expr::Dict(map) => {
                write!(f, "{{")?;
                for (i, (key, value)) in map.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "\"{}\": {}", key, value)?;
                }
                write!(f, "}}")
            }
            Expr::IndexAccess(container, index) => {
                write!(f, "{}[{}]", container, index)
            }
            Expr::MethodCall(obj, method, args) => {
                write!(f, "{}.{}(", obj, method)?;
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{}", arg)?;
                }
                write!(f, ")")
            }
            _ => write!(f, "<?>"),
        }
    }
}

impl std::fmt::Display for ConditionalBranch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.value)?;
        if let Some(cond) = &self.condition {
            write!(f, " when {}", cond)?;
        }
        Ok(())
    }
}

#[allow(dead_code)]
fn tokenize(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    
    for ch in input.chars() {
        if ch == '"' {
            in_quotes = !in_quotes;
            current.push(ch);
        } else if (ch == '+' || ch == '-' || ch == '*' || ch == '/' || ch == '(' || ch == ')' || ch == ',') && !in_quotes {
            if !current.trim().is_empty() {
                tokens.push(current.trim().to_string());
                current.clear();
            }
            tokens.push(ch.to_string());
        } else if ch.is_whitespace() && !in_quotes {
            if !current.is_empty() {
                tokens.push(current.clone());
                current.clear();
            }
        } else {
            current.push(ch);
        }
    }
    
    if !current.is_empty() {
        tokens.push(current);
    }
    
    tokens
}

fn parse_token(token: &str) -> Expr {
    let token = token.trim();

    
    if (token.starts_with('{') && token.ends_with('}')) || 
    (token.lines().count() > 1 && token.contains('{') && token.contains('}')) {
        
        // Clean multi-line formatting
        let cleaned_token = if token.lines().count() > 1 {
            // For multi-line, preserve structure but clean up
            token.lines()
                .map(|line| line.trim())
                .filter(|line| !line.is_empty() && !line.starts_with('#'))
                .collect::<Vec<&str>>()
                .join("")
        } else {
            token.to_string()
        };
        
        return parse_dict_literal(&cleaned_token);
    }

    if token.starts_with('"') && token.ends_with('"') {
        let content = token[1..token.len()-1].to_string();
        return Expr::Literal(Value::Str(content));
    }
    
    // Try integer first
    if let Ok(n) = token.parse::<i64>() {
        return Expr::Literal(Value::Int(n));
    }
    // Try float (must contain a dot and parse as f64, but not as int)
    if token.contains('.') {
        if let Ok(f) = token.parse::<f64>() {
            return Expr::Literal(Value::Float(f));
        }
    }
    
    if token == "true" {
        return Expr::Literal(Value::Bool(true));
    }
    if token == "false" {
        return Expr::Literal(Value::Bool(false));
    }
    
    // Check for method calls: obj.method() or obj.method(arg1, arg2)
    if token.contains('.') && token.contains('(') && token.ends_with(')') {
        return parse_method_call(token);
    }
    
    // Check for function calls
    if token.contains('(') && token.ends_with(')') {
        let name_end = token.find('(').unwrap();
        let func_name = &token[..name_end];
        let args_str = &token[name_end+1..token.len()-1];
        
        // Parse arguments
        let mut args = Vec::new();
        let mut current_arg = String::new();
        let mut paren_depth = 0;
        let mut in_quotes = false;
        let mut quote_char = '"';
        
        for ch in args_str.chars() {
            match ch {
                '"' => {
                    if !in_quotes {
                        in_quotes = true;
                        quote_char = ch;
                    } else if ch == quote_char {
                        in_quotes = false;
                    }
                    current_arg.push(ch);
                }
                '(' if !in_quotes => {
                    paren_depth += 1;
                    current_arg.push(ch);
                }
                ')' if !in_quotes => {
                    paren_depth -= 1;
                    current_arg.push(ch);
                }
                ',' if !in_quotes && paren_depth == 0 => {
                    if !current_arg.trim().is_empty() {
                        args.push(parse_token(current_arg.trim()));
                    }
                    current_arg.clear();
                }
                _ => {
                    current_arg.push(ch);
                }
            }
        }
        
        if !current_arg.trim().is_empty() {
            args.push(parse_token(current_arg.trim()));
        }
        
        return Expr::FunctionCall(func_name.to_string(), args);
    }

    // Handle list literals: [1, 2, 3]
    if token.starts_with('[') && token.ends_with(']') {
        let content = &token[1..token.len()-1].trim();
        if content.is_empty() {
            return Expr::List(Vec::new());
        }
        // Very simple parser for now - just split by comma
        let items: Vec<Expr> = content.split(',')
            .map(|item| parse_token(item.trim()))
            .collect();
        return Expr::List(items);
    }
    
    // Handle dict literals: {"key": "value"} - improved parser
    if token.starts_with('{') && token.ends_with('}') {
        let content = &token[1..token.len()-1].trim();
        if content.is_empty() {
            return Expr::Dict(HashMap::new());
        }
        
        // Simple parser that splits by commas and colons
        let mut map = HashMap::new();
        let parts: Vec<&str> = content.split(',').collect();
        
        for part in parts {
            let part = part.trim();
            if let Some(colon_pos) = part.find(':') {
                let key_part = part[..colon_pos].trim();
                let value_part = part[colon_pos + 1..].trim();
                
                // Extract key (remove quotes)
                let key = if key_part.starts_with('"') && key_part.ends_with('"') && key_part.len() > 1 {
                    &key_part[1..key_part.len()-1]
                } else {
                    key_part
                };
                
                // Parse value
                map.insert(key.to_string(), parse_token(value_part));
            }
        }
        
        return Expr::Dict(map);
    }
    
    Expr::Variable(token.to_string())
}

fn parse_method_call(token: &str) -> Expr {
    // Find the first dot that's not inside parentheses or quotes
    let mut dot_pos = None;
    let mut paren_depth = 0;
    let mut in_quotes = false;
    let mut quote_char = '"';
    
    for (i, ch) in token.chars().enumerate() {
        match ch {
            '"' => {
                if !in_quotes {
                    in_quotes = true;
                    quote_char = ch;
                } else if ch == quote_char {
                    in_quotes = false;
                }
            }
            '(' if !in_quotes => {
                paren_depth += 1;
            }
            ')' if !in_quotes => {
                paren_depth -= 1;
            }
            '.' if !in_quotes && paren_depth == 0 => {
                dot_pos = Some(i);
                break;
            }
            _ => {}
        }
    }
    
    if let Some(dot_pos) = dot_pos {
        let obj_name = &token[..dot_pos];
        let mut remainder = &token[dot_pos + 1..];

        let mut current_expr = parse_token(obj_name);

        // Parse a chain of .method(args).method2(args)... from remainder
        loop {
            // remainder should start with methodName(...)
            // find the first '(' not in quotes
            let mut paren_pos = None;
            let mut in_quotes = false;
            let mut quote_char = '"';
            for (i, ch) in remainder.char_indices() {
                match ch {
                    '"' => {
                        if !in_quotes {
                            in_quotes = true;
                            quote_char = ch;
                        } else if ch == quote_char {
                            in_quotes = false;
                        }
                    }
                    '(' if !in_quotes => {
                        paren_pos = Some(i);
                        break;
                    }
                    _ => {}
                }
            }

            if paren_pos.is_none() {
                // Not a method call we can parse
                break;
            }

            let paren_pos = paren_pos.unwrap();
            let method_name = &remainder[..paren_pos];

            // Find matching closing parenthesis for this method call
            let mut depth = 0isize;
            let mut in_quotes = false;
            let mut quote_char = '"';
            let mut end_pos = None;
            for (i, ch) in remainder.char_indices().skip(paren_pos) {
                match ch {
                    '"' => {
                        if !in_quotes {
                            in_quotes = true;
                            quote_char = ch;
                        } else if ch == quote_char {
                            in_quotes = false;
                        }
                    }
                    '(' if !in_quotes => depth += 1,
                    ')' if !in_quotes => {
                        depth -= 1;
                        if depth == 0 {
                            end_pos = Some(i);
                            break;
                        }
                    }
                    _ => {}
                }
            }

            if end_pos.is_none() {
                break;
            }

            let end_pos = end_pos.unwrap();
            let args_str = &remainder[paren_pos + 1..end_pos];

            // Parse arguments
            let mut args = Vec::new();
            if !args_str.trim().is_empty() {
                let arg_parts = split_arguments_respecting_nesting(args_str);
                for arg_part in arg_parts {
                    args.push(parse_token(arg_part.trim()));
                }
            }

            // Build new MethodCall expr
            current_expr = Expr::MethodCall(Box::new(current_expr), method_name.to_string(), args);

            // Move remainder forward past this method call
            if end_pos + 1 >= remainder.len() {
                // no more text
                remainder = "";
                break;
            } else {
                remainder = &remainder[end_pos + 1..];
                // if next char is '.', skip it and continue; otherwise stop
                if remainder.starts_with('.') {
                    remainder = &remainder[1..];
                    continue;
                } else {
                    break;
                }
            }
        }

        return current_expr;
    }
    
    Expr::Variable(token.to_string())
}

fn split_arguments_respecting_nesting(s: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut quote_char = '"';
    let mut paren_depth = 0;
    let mut bracket_depth = 0;
    
    for ch in s.chars() {
        match ch {
            '"' => {
                if !in_quotes {
                    in_quotes = true;
                    quote_char = ch;
                } else if ch == quote_char {
                    in_quotes = false;
                }
                current.push(ch);
            }
            '(' if !in_quotes => {
                paren_depth += 1;
                current.push(ch);
            }
            ')' if !in_quotes => {
                paren_depth -= 1;
                current.push(ch);
            }
            '[' if !in_quotes => {
                bracket_depth += 1;
                current.push(ch);
            }
            ']' if !in_quotes => {
                bracket_depth -= 1;
                current.push(ch);
            }
            ',' if !in_quotes && paren_depth == 0 && bracket_depth == 0 => {
                parts.push(current.trim().to_string());
                current.clear();
            }
            _ => {
                current.push(ch);
            }
        }
    }
    
    if !current.trim().is_empty() {
        parts.push(current.trim().to_string());
    }
    
    parts
}

fn is_conditional_expression(s: &str) -> bool {
    let mut paren_depth = 0;
    let mut in_quotes = false;
    let mut in_braces = 0;
    
    for ch in s.chars() {
        match ch {
            '"' => in_quotes = !in_quotes,
            '(' if !in_quotes => paren_depth += 1,
            ')' if !in_quotes => paren_depth -= 1,
            '{' if !in_quotes => in_braces += 1,
            '}' if !in_quotes => in_braces -= 1,
            '|' if !in_quotes && paren_depth == 0 && in_braces == 0 => {
                return true;
            }
            _ => {}
        }
    }
    false
}

fn split_conditional_branches(s: &str) -> Result<Vec<&str>, String> {
    let mut branches = Vec::new();
    let mut start = 0;
    let mut paren_depth = 0;
    let mut in_quotes = false;
    let mut in_braces = 0;
    
    for (i, ch) in s.char_indices() {
        match ch {
            '"' => in_quotes = !in_quotes,
            '(' if !in_quotes => paren_depth += 1,
            ')' if !in_quotes => paren_depth -= 1,
            '{' if !in_quotes => in_braces += 1,
            '}' if !in_quotes => in_braces -= 1,
            '|' if !in_quotes && paren_depth == 0 && in_braces == 0 => {
                let branch = &s[start..i];
                if branch.trim().is_empty() {
                    return Err("Empty branch in conditional expression".to_string());
                }
                branches.push(branch);
                start = i + 1;
            }
            _ => {}
        }
    }
    
    let last_branch = &s[start..];
    if last_branch.trim().is_empty() {
        return Err("Empty branch in conditional expression".to_string());
    }
    branches.push(last_branch);
    
    Ok(branches)
}

fn find_when_keyword_advanced(s: &str) -> Option<usize> {
    let _in_quotes = false;
    let mut quote_char = '"';
    let mut paren_depth = 0;
    let mut bracket_depth = 0;
    let mut in_string = false;
    
    let chars: Vec<char> = s.chars().collect();
    
    for i in 0..chars.len() {
        match chars[i] {
            '"' | '\'' => {
                if !in_string {
                    in_string = true;
                    quote_char = chars[i];
                } else if chars[i] == quote_char {
                    // Check for escaped quotes
                    let mut escaped = false;
                    if i > 0 && chars[i-1] == '\\' {
                        let mut backslash_count = 1;
                        let mut j = i - 1;
                        while j > 0 && chars[j-1] == '\\' {
                            backslash_count += 1;
                            j -= 1;
                        }
                        escaped = backslash_count % 2 == 1;
                    }
                    if !escaped {
                        in_string = false;
                    }
                }
            }
            '(' if !in_string => paren_depth += 1,
            ')' if !in_string => paren_depth -= 1,
            '[' if !in_string => bracket_depth += 1,
            ']' if !in_string => bracket_depth -= 1,
            _ => {}
        }
        
        if !in_string && paren_depth == 0 && bracket_depth == 0 {
            if i + 4 <= chars.len() {
                let word: String = chars[i..i+4].iter().collect();
                if word.to_lowercase() == "when" {
                    let prev_char = if i > 0 { Some(chars[i-1]) } else { None };
                    let next_char = if i + 4 < chars.len() { Some(chars[i+4]) } else { None };
                    
                    let is_word_start = prev_char.map(|c| c.is_whitespace() || c == '|').unwrap_or(true);
                    let is_word_end = next_char.map(|c| c.is_whitespace() || c == '|').unwrap_or(true);
                    
                    if is_word_start && is_word_end {
                        return Some(i);
                    }
                }
            }
        }
    }
    
    None
}

fn parse_conditional_branch(s: &str) -> Result<ConditionalBranch, String> {
    let s = s.trim();
    
    // Enhanced pattern matching for "when" keyword
    if let Some(when_pos) = find_when_keyword_advanced(s) {
        let value_str = s[..when_pos].trim();
        let condition_str = s[when_pos + 4..].trim(); // "when" is 4 chars
        
        if value_str.is_empty() {
            return Err("Missing value before 'when'".to_string());
        }
        if condition_str.is_empty() {
            return Err("Missing condition after 'when'".to_string());
        }
        
        let value_expr = parse_operator_expression(value_str)?;
        let condition_expr = parse_enhanced_condition(condition_str)?; 
        
        Ok(ConditionalBranch {
            value: Box::new(value_expr),
            condition: Some(Box::new(condition_expr)),
        })
    } else if s == "otherwise" || s == "else" {
        // Handle "otherwise" branch
        let value_expr = parse_operator_expression("null")?; // Default fallback
        Ok(ConditionalBranch {
            value: Box::new(value_expr),
            condition: None, // No condition means fallback
        })
    } else {
        // Simple value (fallback case)
        let value_expr = parse_operator_expression(s)?;
        Ok(ConditionalBranch {
            value: Box::new(value_expr),
            condition: None,
        })
    }
}

fn parse_conditional_expression(s: &str) -> Result<Expr, String> {
    // Handle multi-line conditionals
    let clean_input = if s.contains('\n') {
        // Clean multi-line formatting
        s.lines()
            .map(|line| line.trim())
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .collect::<Vec<&str>>()
            .join(" ")
            .replace("|", " | ")  // Ensure proper spacing
    } else {
        s.to_string()
    };
    
    let branches_str = split_conditional_branches(&clean_input)?;
    let mut branches = Vec::new();
    
    for branch_str in branches_str {
        let branch = parse_conditional_branch(branch_str)?;
        branches.push(branch);
    }
    
    Ok(Expr::Conditional(branches))
}

fn parse_condition_expression(s: &str) -> Result<Expr, String> {
    let s = s.trim();
    
    // Handle "not" operator
    if s.starts_with("not ") {
        let rest = s[4..].trim(); // Skip "not" and space
        if rest.is_empty() {
            return Err("Missing operand after 'not'".to_string());
        }
        let expr = parse_condition_expression(rest)?;
        return Ok(Expr::Not(Box::new(expr)));
    }
    
    // Handle parentheses at the beginning
    if s.starts_with('(') && s.ends_with(')') {
        // Check if it's properly matched outermost parentheses
        let mut depth = 0;
        let mut is_outermost = true;
        
        for (i, ch) in s.char_indices() {
            match ch {
                '(' => {
                    depth += 1;
                    if depth == 1 && i != 0 {
                        is_outermost = false;
                    }
                }
                ')' => {
                    depth -= 1;
                    if depth == 0 && i != s.len() - 1 {
                        is_outermost = false;
                    }
                }
                _ => {}
            }
        }
        
        if is_outermost {
            // Parse inside the parentheses
            return parse_condition_expression(&s[1..s.len()-1].trim());
        }
    }
    
    // First, try to split on "and" (lowest precedence)
    if let Some(pos) = find_logical_operator(s, "and") {
        let left = &s[..pos].trim();
        let right = &s[pos + 3..].trim(); // "and" is 3 chars
        
        if left.is_empty() || right.is_empty() {
            return Err("Incomplete 'and' expression".to_string());
        }
        
        let left_expr = parse_condition_expression(left)?;
        let right_expr = parse_condition_expression(right)?;
        
        return Ok(Expr::And(Box::new(left_expr), Box::new(right_expr)));
    }
    
    // Then try "or"
    if let Some(pos) = find_logical_operator(s, "or") {
        let left = &s[..pos].trim();
        let right = &s[pos + 2..].trim(); // "or" is 2 chars
        
        if left.is_empty() || right.is_empty() {
            return Err("Incomplete 'or' expression".to_string());
        }
        
        let left_expr = parse_condition_expression(left)?;
        let right_expr = parse_condition_expression(right)?;
        
        return Ok(Expr::Or(Box::new(left_expr), Box::new(right_expr)));
    }
    
    // Then try comparison operators
    let comparisons = [">=", "<=", "==", "!=", ">", "<"];
    
    for &op in &comparisons {
        // Try to find the operator, ignoring those inside parentheses
        let mut search_pos = 0;
        while let Some(pos) = s[search_pos..].find(op) {
            let actual_pos = search_pos + pos;
        
            // Check if this operator is at top level (not inside parentheses)
            let before = &s[..actual_pos];
            let after = &s[actual_pos + op.len()..];
        
            // Simple check: count parentheses
            let open_parens = before.chars().filter(|&c| c == '(').count();
            let close_parens = before.chars().filter(|&c| c == ')').count();
        
            if open_parens == close_parens {
                // At top level
                let left = before.trim();
                let right = after.trim();
            
                if !left.is_empty() && !right.is_empty() {
                    let left_expr = parse_operator_expression(left)?;
                    let right_expr = parse_operator_expression(right)?;
                
                    return match op {
                        ">" => Ok(Expr::GreaterThan(Box::new(left_expr), Box::new(right_expr))),
                        ">=" => Ok(Expr::GreaterThanOrEqual(Box::new(left_expr), Box::new(right_expr))),
                        "<" => Ok(Expr::LessThan(Box::new(left_expr), Box::new(right_expr))),
                        "<=" => Ok(Expr::LessThanOrEqual(Box::new(left_expr), Box::new(right_expr))),
                        "==" => Ok(Expr::Equal(Box::new(left_expr), Box::new(right_expr))),
                        "!=" => Ok(Expr::NotEqual(Box::new(left_expr), Box::new(right_expr))),
                        _ => unreachable!(),
                    };
                }
            }
        
            search_pos = actual_pos + 1;
        }
    }
    
    // If no comparison operator found, parse as arithmetic expression
    parse_operator_expression(s)
}

fn find_logical_operator(s: &str, op: &str) -> Option<usize> {
    // Find "and" or "or" as whole words (not inside other words)
    let mut in_quotes = false;
    let mut quote_char = ' ';
    let mut in_paren: i32 = 0;
    
    let chars: Vec<char> = s.chars().collect();
    
    for i in 0..chars.len() {
        // Handle quotes
        match chars[i] {
            '"' | '\'' => {
                if !in_quotes {
                    in_quotes = true;
                    quote_char = chars[i];
                } else if chars[i] == quote_char {
                    // Check for escaping
                    let mut is_escaped = false;
                    if i > 0 && chars[i-1] == '\\' {
                        let mut j = i - 1;
                        let mut backslash_count = 1;
                        while j > 0 && chars[j-1] == '\\' {
                            backslash_count += 1;
                            j -= 1;
                        }
                        is_escaped = backslash_count % 2 == 1;
                    }
                    if !is_escaped {
                        in_quotes = false;
                    }
                }
            }
            _ => {}
        }
        
        // Handle parentheses
        if !in_quotes {
            if chars[i] == '(' {
                in_paren += 1;
            } else if chars[i] == ')' {
                in_paren = in_paren.saturating_sub(1);
            }
        }
        
        // Check for operator
        if i + op.len() <= chars.len() && !in_quotes && in_paren == 0 {
            let substr: String = chars[i..i+op.len()].iter().collect();
            if substr == op {
                // Check if it's a whole word
                let prev_ok = i == 0 || !chars[i-1].is_alphanumeric();
                let next_ok = i + op.len() >= chars.len() || !chars[i+op.len()].is_alphanumeric();
                
                if prev_ok && next_ok {
                    return Some(i);
                }
            }
        }
    }
    
    None
}

fn parse_operator_expression(s: &str) -> Result<Expr, String> {
    let mut paren_depth = 0;
    let mut best_pos = None;
    let mut best_op = None;
    let mut best_precedence = 3;

    // Handle negative numbers
    if s.starts_with('-') {
        let rest = &s[1..].trim();
        if !rest.is_empty() {
            // Check if it's a number
            if let Ok(n) = rest.parse::<i64>() {
                return Ok(Expr::Literal(Value::Int(-n)));
            }
            // Or it could be a parenthesized expression
            // For simplicity, we'll parse it as 0 - expression
            let expr = parse_operator_expression(rest)?;
            return Ok(Expr::Subtract(Box::new(Expr::Literal(Value::Int(0))), Box::new(expr)));
        }
    }
    
    for (i, ch) in s.char_indices() {
        match ch {
            '(' => paren_depth += 1,
            ')' => paren_depth -= 1,
            '+' | '-' if paren_depth == 0 => {
                best_pos = Some(i);
                best_op = Some(ch);
                best_precedence = 1;
            }
            '*' | '/' if paren_depth == 0 && best_precedence > 1 => {
                best_pos = Some(i);
                best_op = Some(ch);
                best_precedence = 2;
            }
            _ => {}
        }
    }
    
    if let (Some(pos), Some(op)) = (best_pos, best_op) {
        let left = s[..pos].trim();
        let right = s[pos+1..].trim();
        
        if left.is_empty() || right.is_empty() {
            return Err(format!("Incomplete expression around '{}'", op));
        }
        
        let left_expr = parse_operator_expression(left)?;
        let right_expr = parse_operator_expression(right)?;
        
        match op {
            '+' => Ok(Expr::Add(Box::new(left_expr), Box::new(right_expr))),
            '-' => Ok(Expr::Subtract(Box::new(left_expr), Box::new(right_expr))),
            '*' => Ok(Expr::Multiply(Box::new(left_expr), Box::new(right_expr))),
            '/' => Ok(Expr::Divide(Box::new(left_expr), Box::new(right_expr))),
            _ => unreachable!(),
        }
    } else {
        if s.starts_with('(') && s.ends_with(')') {
            let mut depth = 0;
            let mut is_outermost = true;
            
            for (i, ch) in s.char_indices() {
                match ch {
                    '(' => {
                        depth += 1;
                        if depth == 1 && i != 0 {
                            is_outermost = false;
                        }
                    }
                    ')' => {
                        depth -= 1;
                        if depth == 0 && i != s.len() - 1 {
                            is_outermost = false;
                        }
                    }
                    _ => {}
                }
            }
            
            if is_outermost {
                parse_operator_expression(&s[1..s.len()-1].trim())
            } else {
                Ok(parse_token(s))
            }
        } else {
            Ok(parse_token(s))
        }
    }
}

fn parse_expr_with_precedence(s: &str) -> Result<Expr, String> {
    if is_conditional_expression(s) {
        parse_conditional_expression(s)
    } else {
        parse_operator_expression(s)
    }
}

pub fn parse_expression(input: &str) -> Result<Expr, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("Empty expression".to_string());
    }
    // If expression contains dot-chaining, validate its syntax so that
    // malformed chains (e.g. missing parentheses) produce an error
    // instead of being silently treated as a string.
    if trimmed.contains('.') && trimmed.contains('(') {
        validate_method_chain_syntax(trimmed)?;
    }

    parse_expr_with_precedence(trimmed)
}

fn validate_method_chain_syntax(s: &str) -> Result<(), String> {
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0usize;
    let mut paren_depth = 0i32;
    let mut in_quotes = false;

    while i < chars.len() {
        let ch = chars[i];
        if ch == '"' {
            in_quotes = !in_quotes;
            i += 1;
            continue;
        }

        if in_quotes {
            i += 1;
            continue;
        }

        if ch == '(' {
            paren_depth += 1;
            i += 1;
            continue;
        }
        if ch == ')' {
            if paren_depth > 0 { paren_depth -= 1; }
            i += 1;
            continue;
        }

        if ch == '.' && paren_depth == 0 {
            // Found a top-level dot; ensure a method name and '(' follow
            let mut j = i + 1;
            // skip whitespace
            while j < chars.len() && chars[j].is_whitespace() { j += 1; }
            if j >= chars.len() {
                return Err(format!("Syntax error: incomplete method chain after '.' at pos {}", i));
            }
            // method name: must start with letter or underscore
            if !(chars[j].is_alphabetic() || chars[j] == '_') {
                return Err(format!("Syntax error: invalid method name start '{}' at pos {}", chars[j], j));
            }
            // consume method name
            while j < chars.len() && (chars[j].is_alphanumeric() || chars[j] == '_') { j += 1; }
            // skip whitespace
            while j < chars.len() && chars[j].is_whitespace() { j += 1; }
            if j >= chars.len() || chars[j] != '(' {
                return Err(format!("Syntax error: expected '(' after method name at pos {}", j));
            }
            // find matching closing paren
            let mut depth = 0i32;
            let mut k = j;
            let mut in_q = false;
            while k < chars.len() {
                let c = chars[k];
                if c == '"' { in_q = !in_q; k += 1; continue; }
                if in_q { k += 1; continue; }
                if c == '(' { depth += 1; }
                if c == ')' {
                    depth -= 1;
                    if depth == 0 { break; }
                }
                k += 1;
            }
            if k >= chars.len() || chars[k] != ')' {
                return Err(format!("Syntax error: unmatched '(' for method starting at pos {}", j));
            }
            // move i past this method call
            i = k + 1;
            continue;
        }

        i += 1;
    }

    Ok(())
}

pub fn evaluate(expr: &Expr, env: &Env) -> Result<Value, String> {
    match expr {
        Expr::Literal(Value::Str(s)) => {
            // Check if this string needs interpolation
            if s.contains('$') || s.contains('{') {
                match render_template(s, env) {
                    Ok(interpolated) => Ok(Value::Str(interpolated)),
                    Err(e) => Err(e),
                }
            } else {
                Ok(Value::Str(s.clone()))
            }
        }
        Expr::Literal(value) => Ok(value.clone()),
        Expr::Variable(name) => {
            env.get_value(name)
                .cloned()
                .ok_or_else(|| format!("Variable not found: {}", name))
        }
        Expr::Add(left, right) => {
            let left_val = evaluate(left, env)?;
            let right_val = evaluate(right, env)?;
            match (&left_val, &right_val) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a + b)),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a + b)),
                (Value::Int(a), Value::Float(b)) => Ok(Value::Float((*a as f64) + b)),
                (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a + (*b as f64))),
                (Value::Str(a), Value::Str(b)) => Ok(Value::Str(format!("{}{}", a, b))),
                (Value::Str(a), Value::Int(b)) => Ok(Value::Str(format!("{}{}", a, b))),
                (Value::Int(a), Value::Str(b)) => Ok(Value::Str(format!("{}{}", a, b))),
                (Value::Str(a), Value::Float(b)) => Ok(Value::Str(format!("{}{}", a, b))),
                (Value::Float(a), Value::Str(b)) => Ok(Value::Str(format!("{}{}", a, b))),
                _ => Err(format!("Cannot add {} and {} - use explicit types for math", left_val.type_name(), right_val.type_name())),
            }
        }
        Expr::Subtract(left, right) => {
            let left_val = evaluate(left, env)?;
            let right_val = evaluate(right, env)?;
            match (&left_val, &right_val) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a - b)),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a - b)),
                (Value::Int(a), Value::Float(b)) => Ok(Value::Float((*a as f64) - b)),
                (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a - (*b as f64))),
                _ => Err(format!("Cannot subtract {} from {} - must be int or float", right_val.type_name(), left_val.type_name())),
            }
        }
        Expr::Multiply(left, right) => {
            let left_val = evaluate(left, env)?;
            let right_val = evaluate(right, env)?;
            match (&left_val, &right_val) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a * b)),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a * b)),
                (Value::Int(a), Value::Float(b)) => Ok(Value::Float((*a as f64) * b)),
                (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a * (*b as f64))),
                _ => Err(format!("Cannot multiply {} and {} - must be int or float", left_val.type_name(), right_val.type_name())),
            }
        }
        Expr::Divide(left, right) => {
            let left_val = evaluate(left, env)?;
            let right_val = evaluate(right, env)?;
            match (&left_val, &right_val) {
                (Value::Int(a), Value::Int(b)) => {
                    if *b == 0 {
                        Err("Division by zero".to_string())
                    } else {
                        Ok(Value::Float((*a as f64) / (*b as f64)))
                    }
                }
                (Value::Float(a), Value::Float(b)) => {
                    if *b == 0.0 {
                        Err("Division by zero".to_string())
                    } else {
                        Ok(Value::Float(a / b))
                    }
                }
                (Value::Int(a), Value::Float(b)) => {
                    if *b == 0.0 {
                        Err("Division by zero".to_string())
                    } else {
                        Ok(Value::Float((*a as f64) / *b))
                    }
                }
                (Value::Float(a), Value::Int(b)) => {
                    if *b == 0 {
                        Err("Division by zero".to_string())
                    } else {
                        Ok(Value::Float(*a / (*b as f64)))
                    }
                }
                _ => Err(format!("Cannot divide {} by {} - must be int or float", left_val.type_name(), right_val.type_name())),
            }
        }
        Expr::FunctionCall(name, args) => {
            let evaluated_args: Result<Vec<Value>, String> = 
                args.iter().map(|arg| evaluate(arg, env)).collect();
            let args_values = evaluated_args?;
            
            match name.as_str() {
                "count" if args_values.len() == 2 => {
                    let value = &args_values[0];
                    let pattern = match &args_values[1] {
                        Value::Str(s) => s.as_str(),
                        _ => return Err("Pattern must be a string".to_string()),
                    };
                    builtins::count(value, pattern)
                }
                "now" if args_values.is_empty() => Ok(builtins::now()),
                "len" if args_values.len() == 1 => {
                    builtins::len(&args_values[0])
                }
                "upper" if args_values.len() == 1 => {
                    builtins::upper(&args_values[0])
                }
                "lower" if args_values.len() == 1 => {
                    builtins::lower(&args_values[0])
                }
                "trim" if args_values.len() == 1 => {
                    builtins::trim(&args_values[0])
                }

                // NEW: List operations
                "push" if args_values.len() == 2 => {
                    builtins::push(&args_values[0], &args_values[1])
                }
                "pop" if args_values.len() == 1 => {
                    builtins::pop(&args_values[0])
                }
                "contains" if args_values.len() == 2 => {
                    builtins::contains(&args_values[0], &args_values[1])
                }
                "sort" if args_values.len() == 1 => {
                    builtins::sort(&args_values[0])
                }
                // NEW: String operations
                "split" if args_values.len() == 2 => {
                    builtins::split(&args_values[0], &args_values[1])
                }
                "join" if args_values.len() == 2 => {
                    builtins::join(&args_values[0], &args_values[1])
                }
                "replace" if args_values.len() == 3 => {
                    builtins::replace(&args_values[0], &args_values[1], &args_values[2])
                }
                "substring" if args_values.len() == 3 => {
                    builtins::substring(&args_values[0], &args_values[1], &args_values[2])
                }
                "starts_with" if args_values.len() == 2 => {
                    builtins::starts_with(&args_values[0], &args_values[1])
                }
                "ends_with" if args_values.len() == 2 => {
                    builtins::ends_with(&args_values[0], &args_values[1])
                }
                "char_at" if args_values.len() == 2 => {
                    builtins::char_at(&args_values[0], &args_values[1])
                }
                "substring_index" if args_values.len() == 3 => {
                    builtins::substring_index(&args_values[0], &args_values[1], &args_values[2])
                }
                "find_index" if args_values.len() == 2 => {
                    builtins::find_index(&args_values[0], &args_values[1])
                }
                "replace_at" if args_values.len() == 4 => {
                    builtins::replace_at(&args_values[0], &args_values[1], &args_values[2], &args_values[3])
                }

                "keys" if args_values.len() == 1 => {
                    builtins::keys(&args_values[0])
                }
                "values" if args_values.len() == 1 => {
                    builtins::values(&args_values[0])
                }
                "get" if args_values.len() == 2 => {
                    builtins::get(&args_values[0], &args_values[1])
                }
                "put" if args_values.len() == 3 => {
                    builtins::put(&args_values[0], &args_values[1], &args_values[2])
                }
                "has_key" if args_values.len() == 2 => {
                    builtins::has_key(&args_values[0], &args_values[1])
                }
                "remove" if args_values.len() == 2 => {
                    builtins::remove(&args_values[0], &args_values[1])
                }
                "merge" if args_values.len() == 2 => {
                    builtins::merge(&args_values[0], &args_values[1])
                }
                "get_index" if args_values.len() == 2 => {
                    builtins::get_index(&args_values[0], &args_values[1])
                }
                "put_index" if args_values.len() == 3 => {
                    builtins::put_index(&args_values[0], &args_values[1], &args_values[2])
                }
                "insert" if args_values.len() == 3 => {
                    builtins::insert(&args_values[0], &args_values[1], &args_values[2])
                }
                "remove_index" if args_values.len() == 2 => {
                    builtins::remove_index(&args_values[0], &args_values[1])
                }
                "sort" if args_values.len() == 2 => {
                    builtins::sort_with_direction(&args_values[0], &args_values[1])
                }
                
                // Index-based string functions
                "char_at" if args_values.len() == 2 => {
                    builtins::char_at(&args_values[0], &args_values[1])
                }
                "substring_index" if args_values.len() == 3 => {
                    builtins::substring_index(&args_values[0], &args_values[1], &args_values[2])
                }
                "find_index" if args_values.len() == 2 => {
                    builtins::find_index(&args_values[0], &args_values[1])
                }
                "replace_at" if args_values.len() == 4 => {
                    builtins::replace_at(&args_values[0], &args_values[1], &args_values[2], &args_values[3])
                }
                _ => Err(format!("Unknown function or wrong arity: {}/{}", name, args.len())),
            }
        }
        Expr::Conditional(branches) => {
            for branch in branches {
                match &branch.condition {
                    Some(condition) => {
                        match evaluate(condition, env)? {
                            Value::Bool(true) => {
                                return evaluate(&branch.value, env);
                            }
                            Value::Bool(false) => {
                                continue;
                            }
                            _ => return Err("Condition must evaluate to boolean".to_string()),
                        }
                    }
                    None => {
                        return evaluate(&branch.value, env);
                    }
                }
            }
            Err("No matching condition in conditional expression".to_string())
        }
        Expr::GreaterThan(left, right) => {
            let left_val = evaluate(left, env)?;
            let right_val = evaluate(right, env)?;
            match (&left_val, &right_val) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a > b)),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a > b)),
                (Value::Int(a), Value::Float(b)) => Ok(Value::Bool((*a as f64) > *b)),
                (Value::Float(a), Value::Int(b)) => Ok(Value::Bool(*a > (*b as f64))),
                _ => Err(format!("Cannot compare {} and {}", left_val.type_name(), right_val.type_name())),
            }
        }
        Expr::GreaterThanOrEqual(left, right) => {
            let left_val = evaluate(left, env)?;
            let right_val = evaluate(right, env)?;
            match (&left_val, &right_val) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a >= b)),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a >= b)),
                (Value::Int(a), Value::Float(b)) => Ok(Value::Bool((*a as f64) >= *b)),
                (Value::Float(a), Value::Int(b)) => Ok(Value::Bool(*a >= (*b as f64))),
                _ => Err(format!("Cannot compare {} and {}", left_val.type_name(), right_val.type_name())),
            }
        }
        Expr::LessThan(left, right) => {
            let left_val = evaluate(left, env)?;
            let right_val = evaluate(right, env)?;
            match (&left_val, &right_val) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a < b)),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a < b)),
                (Value::Int(a), Value::Float(b)) => Ok(Value::Bool((*a as f64) < *b)),
                (Value::Float(a), Value::Int(b)) => Ok(Value::Bool(*a < (*b as f64))),
                _ => Err(format!("Cannot compare {} and {}", left_val.type_name(), right_val.type_name())),
            }
        }
        Expr::LessThanOrEqual(left, right) => {
            let left_val = evaluate(left, env)?;
            let right_val = evaluate(right, env)?;
            match (&left_val, &right_val) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a <= b)),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a <= b)),
                (Value::Int(a), Value::Float(b)) => Ok(Value::Bool((*a as f64) <= *b)),
                (Value::Float(a), Value::Int(b)) => Ok(Value::Bool(*a <= (*b as f64))),
                _ => Err(format!("Cannot compare {} and {}", left_val.type_name(), right_val.type_name())),
            }
        }
        Expr::Equal(left, right) => {
            let left_val = evaluate(left, env)?;
            let right_val = evaluate(right, env)?;
            Ok(Value::Bool(left_val == right_val))
        }
        Expr::NotEqual(left, right) => {
            let left_val = evaluate(left, env)?;
            let right_val = evaluate(right, env)?;
            Ok(Value::Bool(left_val != right_val))
        }
        Expr::And(left, right) => {
            let left_val = evaluate(left, env)?;
            let right_val = evaluate(right, env)?;
            
            match (&left_val, &right_val) {
                (Value::Bool(a), Value::Bool(b)) => Ok(Value::Bool(*a && *b)),
                _ => Err("Logical 'and' requires boolean operands".to_string()),
            }
        }
        Expr::Or(left, right) => {
            let left_val = evaluate(left, env)?;
            let right_val = evaluate(right, env)?;
            
            match (&left_val, &right_val) {
                (Value::Bool(a), Value::Bool(b)) => Ok(Value::Bool(*a || *b)),
                _ => Err("Logical 'or' requires boolean operands".to_string()),
            }
        }
        Expr::Not(expr) => {
            let val = evaluate(expr, env)?;
            match val {
                Value::Bool(b) => Ok(Value::Bool(!b)),
                _ => Err("Logical 'not' requires boolean operand".to_string()),
            }
        }
        Expr::List(items) => {
            let mut evaluated_items = Vec::new();
            for item in items {
                evaluated_items.push(evaluate(item, env)?);
            }
            Ok(Value::List(evaluated_items))
        }
        Expr::Dict(map) => {
            let mut evaluated_map = HashMap::new();
            for (key, value_expr) in map {
                let value = evaluate(value_expr, env)?;
                evaluated_map.insert(key.clone(), value);
            }
            Ok(Value::Dict(evaluated_map))
        }
        Expr::IndexAccess(container_expr, index_expr) => {
            let container = evaluate(container_expr, env)?;
            let index = evaluate(index_expr, env)?;
            
            match (&container, &index) {
                (Value::List(items), Value::Int(i)) => {
                    let idx = *i as usize;
                    if idx < items.len() {
                        Ok(items[idx].clone())
                    } else {
                        Err(format!("Index {} out of bounds for list of length {}", idx, items.len()))
                    }
                }
                (Value::Dict(map), Value::Str(key)) => {
                    if let Some(value) = map.get(key) {
                        Ok(value.clone())
                    } else {
                        Err(format!("Key '{}' not found in dictionary", key))
                    }
                }
                _ => Err(format!("Cannot index {} with {}", container.type_name(), index.type_name())),
            }
        }
        Expr::MethodCall(obj_expr, method_name, args) => {
            // Evaluate arguments for this method (expect's arg is typically a message)
            let mut evaluated_args = Vec::new();
            for arg in args {
                evaluated_args.push(evaluate(arg, env)?);
            }

            // Special-case `.expect(...)`: it should catch errors from the inner expression
            if method_name == "expect" {
                // Evaluate the inner expression but DO NOT propagate its error immediately;
                // instead return a controlled error message if it failed.
                match evaluate(obj_expr, env) {
                    Ok(v) => return Ok(v),
                    Err(_inner_err) => {
                        // If a message string was provided, use it directly
                        if let Some(Value::Str(msg)) = evaluated_args.get(0) {
                            return Err(msg.clone());
                        }

                        // Otherwise compute a default message indicating which dot-function position failed.
                        fn count_methods(expr: &Expr) -> usize {
                            match expr {
                                Expr::MethodCall(inner, _m, _args) => 1 + count_methods(inner),
                                _ => 0,
                            }
                        }

                        let method_pos = count_methods(&*obj_expr);
                        return Err(format!("Failed at function {}", method_pos));
                    }
                }
            }

            // For all other methods, evaluate the object and proceed as before
            let obj = evaluate(obj_expr, env)?;

            // Clone obj once for use in match to avoid borrowing issues
            match (obj.clone(), method_name.as_str()) {  // Clone here
                // String methods
                (Value::Str(s), "len") => Ok(Value::Int(s.len() as i64)),
                (Value::Str(s), "upper") => Ok(Value::Str(s.to_uppercase())),
                (Value::Str(s), "lower") => Ok(Value::Str(s.to_lowercase())),
                (Value::Str(s), "trim") => Ok(Value::Str(s.trim().to_string())),
                (Value::Str(json_str), "parse_json") => {
                    crate::core::builtins::parse_json(&json_str)
                },
                (value, "to_json") => {
                    crate::core::builtins::to_json(&value).map(|s| Value::Str(s))
                },
                
                // JSON Path methods
                (Value::Dict(_) | Value::Json(_), "get") if !evaluated_args.is_empty() => {
                    if let Value::Str(path) = &evaluated_args[0] {
                        let json_path = crate::core::builtins::JsonPath::parse(path)
                            .map_err(|e| format!("Invalid JSON path '{}': {}", path, e))?;
                        // Use the cloned obj from the match:
                        json_path.get(&obj).map_err(|e| format!("JSON path error: {}", e))
                    } else {
                        Err("JSON get() requires string path".to_string())
                    }
                },
                (Value::Str(json_str), "get") if !evaluated_args.is_empty() => {
                    if let Value::Str(path) = &evaluated_args[0] {
                        match crate::core::builtins::parse_json(&json_str) {
                            Ok(parsed_json) => {
                                let json_path = crate::core::builtins::JsonPath::parse(path)
                                    .map_err(|e| format!("Invalid JSON path '{}': {}", path, e))?;
                                json_path.get(&parsed_json).map_err(|e| format!("JSON path error: {}", e))
                            }
                            Err(e) => Err(format!("Cannot parse JSON: {}", e)),
                        }
                    } else {
                        Err("JSON get() requires string path".to_string())
                    }
                },
                
                // Advanced JSON methods
                (Value::Dict(_) | Value::Json(_), "keys") => {
                    match &obj {  // Use reference to the cloned obj
                        Value::Dict(map) => {
                            let keys: Vec<Value> = map.keys()
                                .map(|k| Value::Str(k.clone()))
                                .collect();
                            Ok(Value::List(keys))
                        }
                        Value::Json(json_str) => {
                            match crate::core::builtins::parse_json(json_str) {
                                Ok(Value::Dict(map)) => {
                                    let keys: Vec<Value> = map.keys()
                                        .map(|k| Value::Str(k.clone()))
                                        .collect();
                                    Ok(Value::List(keys))
                                }
                                Ok(_) => Err("JSON is not an object".to_string()),
                                Err(e) => Err(format!("Cannot parse JSON: {}", e)),
                            }
                        }
                        _ => Err("keys() requires dictionary or JSON object".to_string()),
                    }
                },
                
                (Value::Dict(_) | Value::Json(_), "values") => {
                    match &obj {  // Use reference to the cloned obj
                        Value::Dict(map) => {
                            let values: Vec<Value> = map.values().cloned().collect();
                            Ok(Value::List(values))
                        }
                        Value::Json(json_str) => {
                            match crate::core::builtins::parse_json(json_str) {
                                Ok(Value::Dict(map)) => {
                                    let values: Vec<Value> = map.values().cloned().collect();
                                    Ok(Value::List(values))
                                }
                                Ok(_) => Err("JSON is not an object".to_string()),
                                Err(e) => Err(format!("Cannot parse JSON: {}", e)),
                            }
                        }
                        _ => Err("values() requires dictionary or JSON object".to_string()),
                    }
                },
                
                // List methods
                (Value::List(items), "len") => Ok(Value::Int(items.len() as i64)),
                (Value::List(mut items), "push") if !evaluated_args.is_empty() => {
                    items.push(evaluated_args[0].clone());
                    Ok(Value::List(items))
                },
                (Value::List(items), "pop") => {
                    let mut items = items;
                    if let Some(_last) = items.pop() {
                        Ok(Value::List(items))
                    } else {
                        Err("Cannot pop from empty list".to_string())
                    }
                },
                (Value::List(items), "get") if !evaluated_args.is_empty() => {
                    if let Value::Int(index) = evaluated_args[0] {
                        let idx = index as usize;
                        if idx < items.len() {
                            Ok(items[idx].clone())
                        } else {
                            Err(format!("Index {} out of bounds", idx))
                        }
                    } else {
                        Err("List get() requires integer index".to_string())
                    }
                },
                (Value::List(items), "contains") if !evaluated_args.is_empty() => {
                    Ok(Value::Bool(items.contains(&evaluated_args[0].clone())))
                },
                (Value::List(items), "sort") => {
                    crate::core::builtins::sort(&Value::List(items.clone()))
                },
                (Value::List(items), "sort_with_direction") if !evaluated_args.is_empty() => {
                    crate::core::builtins::sort_with_direction(&Value::List(items.clone()), &evaluated_args[0])
                },
                (Value::List(items), "filter") if !evaluated_args.is_empty() => {
                    // filter expects a condition expression string
                    if let Value::Str(cond) = &evaluated_args[0] {
                        crate::core::builtins::filter(&Value::List(items.clone()), cond, env)
                    } else {
                        Err("filter() requires string condition".to_string())
                    }
                },
                
                // Dict methods
                (Value::Dict(map), "get") if !evaluated_args.is_empty() => {
                    if let Value::Str(key) = &evaluated_args[0] {
                        if let Some(val) = map.get(key) {
                            Ok(val.clone())
                        } else {
                            Err(format!("Key '{}' not found", key))
                        }
                    } else {
                        Err("Dict get() requires string key".to_string())
                    }
                },
                (Value::Dict(mut map), "set") if evaluated_args.len() >= 2 => {
                    if let Value::Str(key) = &evaluated_args[0] {
                        map.insert(key.clone(), evaluated_args[1].clone());
                        Ok(Value::Dict(map))
                    } else {
                        Err("Dict set() requires string key".to_string())
                    }
                },
                (Value::Dict(map), "has_key") if !evaluated_args.is_empty() => {
                    if let Value::Str(key) = &evaluated_args[0] {
                        Ok(Value::Bool(map.contains_key(key)))
                    } else {
                        Err("has_key() requires string key".to_string())
                    }
                },
                (Value::Dict(mut map), "remove") if !evaluated_args.is_empty() => {
                    if let Value::Str(key) = &evaluated_args[0] {
                        map.remove(key);
                        Ok(Value::Dict(map))
                    } else {
                        Err("remove() requires string key".to_string())
                    }
                },
                (Value::Dict(map1), "merge") if !evaluated_args.is_empty() => {
                    if let Value::Dict(map2) = &evaluated_args[0] {
                        let mut merged = map1.clone();
                        for (k, v) in map2 {
                            merged.insert(k.clone(), v.clone());
                        }
                        Ok(Value::Dict(merged))
                    } else {
                        Err("merge() requires a dictionary argument".to_string())
                    }
                },
                
                // String methods continued
                (Value::Str(s), "split") if !evaluated_args.is_empty() => {
                    crate::core::builtins::split(&Value::Str(s.clone()), &evaluated_args[0])
                },
                (Value::Str(s), "find_index") if !evaluated_args.is_empty() => {
                    crate::core::builtins::find_index(&Value::Str(s.clone()), &evaluated_args[0])
                },
                (Value::Str(s), "starts_with") if !evaluated_args.is_empty() => {
                    crate::core::builtins::starts_with(&Value::Str(s.clone()), &evaluated_args[0])
                },
                (Value::Str(s), "ends_with") if !evaluated_args.is_empty() => {
                    crate::core::builtins::ends_with(&Value::Str(s.clone()), &evaluated_args[0])
                },
                (Value::Str(s), "char_at") if !evaluated_args.is_empty() => {
                    crate::core::builtins::char_at(&Value::Str(s.clone()), &evaluated_args[0])
                },
                (Value::Str(s), "substring") if evaluated_args.len() >= 2 => {
                    crate::core::builtins::substring(&Value::Str(s.clone()), &evaluated_args[0], &evaluated_args[1])
                },
                (Value::Str(s), "substring_index") if evaluated_args.len() >= 2 => {
                    crate::core::builtins::substring_index(&Value::Str(s.clone()), &evaluated_args[0], &evaluated_args[1])
                },
                (Value::Str(s), "replace_at") if evaluated_args.len() >= 3 => {
                    crate::core::builtins::replace_at(&Value::Str(s.clone()), &evaluated_args[0], &evaluated_args[1], &evaluated_args[2])
                },
                (Value::Str(s), "replace") if evaluated_args.len() >= 2 => {
                    crate::core::builtins::replace(&Value::Str(s.clone()), &evaluated_args[0], &evaluated_args[1])
                },
                (Value::List(items), "join") if !evaluated_args.is_empty() => {
                    crate::core::builtins::join(&Value::List(items.clone()), &evaluated_args[0])
                },
                
                // Add more method implementations as needed
                (obj_val, method) => Err(format!("Method '{}' not implemented for {}", method, obj_val.type_name())),
                
            }
        }
    }
}


pub fn extract_variables(expr: &Expr) -> Vec<String> {
    let mut vars = HashSet::new();
    extract_variables_recursive(expr, &mut vars);
    vars.into_iter().collect()
}

fn extract_variables_recursive(expr: &Expr, vars: &mut HashSet<String>) {
    match expr {
        Expr::Variable(name) => {
            vars.insert(name.clone());
        }
        Expr::Add(left, right)
        | Expr::Subtract(left, right)
        | Expr::Multiply(left, right)
        | Expr::Divide(left, right) => {
            extract_variables_recursive(left, vars);
            extract_variables_recursive(right, vars);
        }
        Expr::GreaterThan(left, right)
        | Expr::GreaterThanOrEqual(left, right)
        | Expr::LessThan(left, right)
        | Expr::LessThanOrEqual(left, right)
        | Expr::Equal(left, right)
        | Expr::NotEqual(left, right)
        | Expr::And(left, right)
        | Expr::Or(left, right) => {
            extract_variables_recursive(left, vars);
            extract_variables_recursive(right, vars);
        }
        Expr::Not(expr) => {
            extract_variables_recursive(expr, vars);
        }
        Expr::FunctionCall(_, args) => {
            for arg in args {
                extract_variables_recursive(arg, vars);
            }
        }
        Expr::Conditional(branches) => {
            for branch in branches {
                extract_variables_recursive(&branch.value, vars);
                if let Some(condition) = &branch.condition {
                    extract_variables_recursive(condition, vars);
                }
            }
        }
        
        Expr::List(items) => {
            for item in items {
                extract_variables_recursive(item, vars);
            }
        }
        Expr::Dict(map) => {
            for value in map.values() {
                extract_variables_recursive(value, vars);
            }
        }
        Expr::IndexAccess(container, index) => {
            extract_variables_recursive(container, vars);
            extract_variables_recursive(index, vars);
        }
        Expr::MethodCall(obj, _, args) => {
            extract_variables_recursive(obj, vars);
            for arg in args {
                extract_variables_recursive(arg, vars);
            }
        }
        Expr::Literal(_) => {} // This handles all literal types including Int, Float, Bool, etc.
    }
}

/*fn parse_index_access(expr_str: &str) -> Result<Expr, String> {
    // Find opening bracket
    if let Some(open_pos) = expr_str.find('[') {
        let var_name = &expr_str[..open_pos];
        let rest = &expr_str[open_pos..];
        
        if rest.ends_with(']') && rest.len() > 2 {
            let index_str = &rest[1..rest.len()-1];
            let var_expr = parse_token(var_name);
            let index_expr = parse_token(index_str);
            return Ok(Expr::IndexAccess(Box::new(var_expr), Box::new(index_expr)));
        }
    }
    Err("Invalid index access".to_string())
}*/

fn parse_dict_from_cleaned(token: &str) -> Expr {
    if !(token.starts_with('{') && token.ends_with('}')) {
        return Expr::Variable(token.to_string());
    }
    
    let content = &token[1..token.len()-1].trim();
    if content.is_empty() {
        return Expr::Dict(HashMap::new());
    }
    
    let mut map = HashMap::new();
    let pairs = split_json_pairs(content);
    
    for (key, value) in pairs {
        map.insert(key, parse_token(&value));
    }
    
    Expr::Dict(map)
}

fn split_json_pairs(content: &str) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    let mut current_key = String::new();
    let mut current_value = String::new();
    let mut in_key = true;
    let mut in_quotes = false;
    let mut quote_char = '"';
    let mut bracket_depth = 0;
    let mut brace_depth = 0;
    let mut chars = content.chars().peekable();
    
    while let Some(ch) = chars.next() {
        match ch {
            '"' | '\'' => {
                if !in_quotes {
                    in_quotes = true;
                    quote_char = ch;
                } else if ch == quote_char && 
                         (!chars.peek().map_or(false, |&next| next == '\\')) {
                    in_quotes = false;
                }
                if in_key {
                    current_key.push(ch);
                } else {
                    current_value.push(ch);
                }
            }
            ':' if !in_quotes && brace_depth == 0 && bracket_depth == 0 => {
                if in_key {
                    in_key = false;
                } else {
                    current_value.push(ch);
                }
            }
            ',' if !in_quotes && brace_depth == 0 && bracket_depth == 0 => {
                if !in_key {
                    pairs.push((strip_quotes(&current_key), current_value.trim().to_string()));
                    current_key.clear();
                    current_value.clear();
                    in_key = true;
                }
            }
            '{' if !in_quotes => {
                brace_depth += 1;
                if !in_key {
                    current_value.push(ch);
                }
            }
            '}' if !in_quotes => {
                brace_depth -= 1;
                if !in_key {
                    current_value.push(ch);
                }
            }
            '[' if !in_quotes => {
                bracket_depth += 1;
                if !in_key {
                    current_value.push(ch);
                }
            }
            ']' if !in_quotes => {
                bracket_depth -= 1;
                if !in_key {
                    current_value.push(ch);
                }
            }
            _ => {
                if in_key {
                    current_key.push(ch);
                } else {
                    current_value.push(ch);
                }
            }
        }
    }
    
    // Don't forget the last pair
    if !current_key.is_empty() && !current_value.is_empty() {
        pairs.push((strip_quotes(&current_key), current_value.trim().to_string()));
    }
    
    pairs
}

fn parse_dict_literal(token: &str) -> Expr {
    if !(token.starts_with('{') && token.ends_with('}')) {
        return Expr::Variable(token.to_string());
    }
    
    let content = &token[1..token.len()-1].trim();
    if content.is_empty() {
        return Expr::Dict(HashMap::new());
    }
    
    // More robust parsing for complex nested structures
    let pairs = parse_json_style_pairs(content);
    
    let mut expr_map = HashMap::new();
    for (key, value_str) in pairs {
        expr_map.insert(key, parse_token(&value_str));
    }
    
    Expr::Dict(expr_map)
}

fn parse_json_style_pairs(content: &str) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    let mut current_key = String::new();
    let mut current_value = String::new();
    let mut in_key = true;
    let mut in_string = false;
    let mut string_char = '"';
    let mut depth = 0; // Track nesting level
    let mut i = 0;
    let chars: Vec<char> = content.chars().collect();
    
    while i < chars.len() {
        let ch = chars[i];
        
        match ch {
            '"' | '\'' => {
                if !in_string {
                    in_string = true;
                    string_char = ch;
                } else if ch == string_char {
                    // Check if it's escaped
                    let mut escaped = false;
                    let mut backslash_count = 0;
                    let mut j = i;
                    while j > 0 && chars[j-1] == '\\' {
                        backslash_count += 1;
                        j -= 1;
                    }
                    escaped = backslash_count % 2 == 1;
                    
                    if !escaped {
                        in_string = false;
                    }
                }
                
                if in_key {
                    current_key.push(ch);
                } else {
                    current_value.push(ch);
                }
            }
            ':' if !in_string && depth == 0 => {
                in_key = false;
            }
            ',' if !in_string && depth == 0 => {
                if !in_key && !current_key.is_empty() {
                    pairs.push((
                        strip_quotes(&current_key.trim()).to_string(),
                        current_value.trim().to_string()
                    ));
                    current_key.clear();
                    current_value.clear();
                    in_key = true;
                }
            }
            '{' if !in_string => {
                depth += 1;
                if !in_key {
                    current_value.push(ch);
                }
            }
            '}' if !in_string => {
                depth -= 1;
                if !in_key {
                    current_value.push(ch);
                }
            }
            '[' if !in_string => {
                depth += 1;
                if !in_key {
                    current_value.push(ch);
                }
            }
            ']' if !in_string => {
                depth -= 1;
                if !in_key {
                    current_value.push(ch);
                }
            }
            _ => {
                if in_key {
                    current_key.push(ch);
                } else {
                    current_value.push(ch);
                }
            }
        }
        i += 1;
    }
    
    // Don't forget the last pair
    if !current_key.is_empty() {
        pairs.push((
            strip_quotes(&current_key.trim()).to_string(),
            current_value.trim().to_string()
        ));
    }
    
    pairs
}

fn strip_quotes(s: &str) -> String {
    let s = s.trim();
    if s.len() >= 2 {
        if (s.starts_with('"') && s.ends_with('"')) ||
           (s.starts_with('\'') && s.ends_with('\'')) {
            return s[1..s.len()-1].to_string();
        }
    }
    s.to_string()
}

fn parse_enhanced_condition(s: &str) -> Result<Expr, String> {
    parse_logical_or(s)
}

/*fn parse_and_condition(s: &str) -> Result<Expr, String> {
    let and_parts = split_by_operator_respecting_nesting(s, "and");
    if and_parts.len() > 1 {
        let mut exprs = Vec::new();
        for part in and_parts {
            exprs.push(parse_comparison_condition(part)?);
        }
        
        let mut result = exprs[0].clone();
        for expr in exprs.into_iter().skip(1) {
            result = Expr::And(Box::new(result), Box::new(expr));
        }
        return Ok(result);
    }
    
    return parse_comparison_condition(s);
}

fn parse_comparison_condition(s: &str) -> Result<Expr, String> {
    let s = s.trim();
    
    // Handle parentheses at the beginning
    if s.starts_with('(') && s.ends_with(')') {
        // Check if it's properly matched outermost parentheses
        let mut depth = 0;
        let mut is_outermost = true;
        
        for (i, ch) in s.char_indices() {
            match ch {
                '(' => {
                    depth += 1;
                    if depth == 1 && i != 0 {
                        is_outermost = false;
                    }
                }
                ')' => {
                    depth -= 1;
                    if depth == 0 && i != s.len() - 1 {
                        is_outermost = false;
                    }
                }
                _ => {}
            }
        }
        
        if is_outermost {
            return parse_comparison_condition(&s[1..s.len()-1].trim());
        }
    }
    
    // Handle comparison operators
    let comparisons = [">=", "<=", "==", "!=", ">", "<"];
    
    for &op in &comparisons {
        let parts = split_by_simple_operator(s, op);
        if parts.len() == 2 {
            let left = parse_operator_expression(parts[0].trim())?;
            let right = parse_operator_expression(parts[1].trim())?;
            
            return match op {
                ">" => Ok(Expr::GreaterThan(Box::new(left), Box::new(right))),
                ">=" => Ok(Expr::GreaterThanOrEqual(Box::new(left), Box::new(right))),
                "<" => Ok(Expr::LessThan(Box::new(left), Box::new(right))),
                "<=" => Ok(Expr::LessThanOrEqual(Box::new(left), Box::new(right))),
                "==" => Ok(Expr::Equal(Box::new(left), Box::new(right))),
                "!=" => Ok(Expr::NotEqual(Box::new(left), Box::new(right))),
                _ => unreachable!(),
            };
        }
    }
    
    // If no comparison operator found, parse as arithmetic expression
    parse_operator_expression(s)
}*/

/*fn split_by_operator_respecting_nesting<'a>(s: &'a str, op: &str) -> Vec<&'a str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut paren_depth = 0;
    let mut in_quotes = false;
    let mut quote_char = ' ';
    
    let chars: Vec<char> = s.chars().collect();
    let op_chars: Vec<char> = op.chars().collect();
    
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '"' | '\'' => {
                if !in_quotes {
                    in_quotes = true;
                    quote_char = chars[i];
                } else if chars[i] == quote_char {
                    // Check for escaping
                    let mut is_escaped = false;
                    let mut j = i;
                    let mut backslash_count = 0;
                    while j > 0 && chars[j-1] == '\\' {
                        backslash_count += 1;
                        j -= 1;
                    }
                    is_escaped = backslash_count % 2 == 1;
                    
                    if !is_escaped {
                        in_quotes = false;
                    }
                }
            }
            '(' if !in_quotes => paren_depth += 1,
            ')' if !in_quotes => paren_depth -= 1,
            _ => {}
        }
        
        // Check for operator
        if !in_quotes && paren_depth == 0 && i + op_chars.len() <= chars.len() {
            let substr: String = chars[i..i+op_chars.len()].iter().collect();
            if substr == op {
                // Check word boundaries
                let prev_ok = i == 0 || chars[i-1].is_whitespace();
                let next_ok = i + op_chars.len() >= chars.len() || 
                             chars[i + op_chars.len()].is_whitespace();
                
                if prev_ok && next_ok {
                    parts.push(&s[start..i]);
                    start = i + op_chars.len();
                    i += op_chars.len() - 1; // Adjust for loop increment
                }
            }
        }
        
        i += 1;
    }
    
    parts.push(&s[start..]);
    parts
}*/

/*fn split_by_simple_operator<'a>(s: &'a str, op: &str) -> Vec<&'a str> {
    let parts: Vec<&str> = s.split(op).collect();
    if parts.len() == 2 {
        vec![parts[0], parts[1]]
    } else {
        vec![s]
    }
}*/


fn parse_logical_or(s: &str) -> Result<Expr, String> {
    let parts = split_by_logical_operator(s, "or");
    if parts.len() > 1 {
        let mut exprs = Vec::new();
        for part in parts {
            exprs.push(parse_logical_and(&part)?);
        }
        
        let mut result = exprs[0].clone();
        for expr in exprs.into_iter().skip(1) {
            result = Expr::Or(Box::new(result), Box::new(expr));
        }
        Ok(result)
    } else {
        parse_logical_and(s)
    }
}

fn parse_logical_and(s: &str) -> Result<Expr, String> {
    let parts = split_by_logical_operator(s, "and");
    if parts.len() > 1 {
        let mut exprs = Vec::new();
        for part in parts {
            exprs.push(parse_comparison(&part)?);
        }
        
        let mut result = exprs[0].clone();
        for expr in exprs.into_iter().skip(1) {
            result = Expr::And(Box::new(result), Box::new(expr));
        }
        Ok(result)
    } else {
        parse_comparison(s)
    }
}

fn parse_comparison(s: &str) -> Result<Expr, String> {
    let s = s.trim();
    
    // Handle "not" operator
    if s.starts_with("not ") {
        let rest = s[4..].trim();
        if rest.is_empty() {
            return Err("Missing operand after 'not'".to_string());
        }
        let expr = parse_comparison(rest)?;
        return Ok(Expr::Not(Box::new(expr)));
    }
    
    // Handle parentheses
    if s.starts_with('(') && s.ends_with(')') {
        // Check if it's properly matched outermost parentheses
        let mut depth = 0;
        let mut is_outermost = true;
        
        for (i, ch) in s.char_indices() {
            match ch {
                '(' => {
                    depth += 1;
                    if depth == 1 && i != 0 {
                        is_outermost = false;
                    }
                }
                ')' => {
                    depth -= 1;
                    if depth == 0 && i != s.len() - 1 {
                        is_outermost = false;
                    }
                }
                _ => {}
            }
        }
        
        if is_outermost && depth == 0 {
            return parse_comparison(&s[1..s.len()-1].trim());
        }
    }
    
    // Handle comparison operators
    let comparisons = [">=", "<=", "==", "!=", ">", "<", "in"];
    
    for &op in &comparisons {
        let parts = split_by_comparison_operator(s, op);
        if parts.len() == 2 {
            let left = parse_arithmetic(&parts[0])?;
            let right = parse_arithmetic(&parts[1])?;
            
            return match op {
                ">" => Ok(Expr::GreaterThan(Box::new(left), Box::new(right))),
                ">=" => Ok(Expr::GreaterThanOrEqual(Box::new(left), Box::new(right))),
                "<" => Ok(Expr::LessThan(Box::new(left), Box::new(right))),
                "<=" => Ok(Expr::LessThanOrEqual(Box::new(left), Box::new(right))),
                "==" => Ok(Expr::Equal(Box::new(left), Box::new(right))),
                "!=" => Ok(Expr::NotEqual(Box::new(left), Box::new(right))),
                "in" => Ok(Expr::FunctionCall("contains".to_string(), vec![right, left])), // reversed for contains
                _ => unreachable!(),
            };
        }
    }
    
    // If no comparison operator found, parse as arithmetic expression
    parse_arithmetic(s)
}

fn parse_arithmetic(s: &str) -> Result<Expr, String> {
    // Your existing arithmetic parsing logic
    parse_operator_expression(s)
}

fn split_by_logical_operator(s: &str, op: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut quote_char = '"';
    let mut paren_depth = 0i32;
    let mut bracket_depth = 0i32;
    
    let chars: Vec<char> = s.chars().collect();
    
    // Find the positions where the operator occurs
    let op_chars: Vec<char> = op.chars().collect();
    let mut i = 0;
    
    while i < chars.len() {
        if !in_quotes && paren_depth == 0 && bracket_depth == 0 {
            // Check if we found the operator at position i
            if i + op_chars.len() <= chars.len() {
                let substr: String = chars[i..i+op_chars.len()].iter().collect();
                if substr == op {
                    // Check word boundaries
                    let prev_ok = i == 0 || chars[i-1].is_whitespace();
                    let next_ok = i + op_chars.len() >= chars.len() || chars[i + op_chars.len()].is_whitespace();
                    
                    if prev_ok && next_ok {
                        parts.push(current.trim().to_string());
                        current.clear();
                        i += op_chars.len();
                        continue;
                    }
                }
            }
        }
        
        match chars[i] {
            '"' | '\'' => {
                if !in_quotes {
                    in_quotes = true;
                    quote_char = chars[i];
                } else if chars[i] == quote_char {
                    // Check for escaping
                    let mut escaped = false;
                    if i > 0 && chars[i-1] == '\\' {
                        let mut backslash_count = 1;
                        let mut j = i - 1;
                        while j > 0 && chars[j-1] == '\\' {
                            backslash_count += 1;
                            j -= 1;
                        }
                        escaped = backslash_count % 2 == 1;
                    }
                    if !escaped {
                        in_quotes = false;
                    }
                }
                current.push(chars[i]);
            }
            '(' if !in_quotes => {
                paren_depth += 1;
                current.push(chars[i]);
            }
            ')' if !in_quotes => {
                paren_depth -= 1;
                current.push(chars[i]);
            }
            '[' if !in_quotes => {
                bracket_depth += 1;
                current.push(chars[i]);
            }
            ']' if !in_quotes => {
                bracket_depth -= 1;
                current.push(chars[i]);
            }
            _ => {
                current.push(chars[i]);
            }
        }
        i += 1;
    }
    
    if !current.is_empty() {
        parts.push(current.trim().to_string());
    }
    
    parts
}

fn split_by_comparison_operator(s: &str, op: &str) -> Vec<String> {
    let mut in_quotes = false;
    let mut quote_char = '"';
    let mut paren_depth = 0i32;
    let mut bracket_depth = 0i32;
    
    let chars: Vec<char> = s.chars().collect();
    let op_chars: Vec<char> = op.chars().collect();
    
    for i in 0..chars.len() {
        if !in_quotes && paren_depth == 0 && bracket_depth == 0 {
            // Check if we found the operator
            if i + op_chars.len() <= chars.len() {
                let substr: String = chars[i..i+op_chars.len()].iter().collect();
                if substr == op {
                    // Check word boundaries
                    let prev_ok = i == 0 || chars[i-1].is_whitespace();
                    let next_ok = i + op_chars.len() >= chars.len() || chars[i + op_chars.len()].is_whitespace();
                    
                    if prev_ok && next_ok {
                        return vec![s[..i].to_string(), s[i + op_chars.len()..].to_string()];
                    }
                }
            }
        }
        
        match chars[i] {
            '"' | '\'' => {
                if !in_quotes {
                    in_quotes = true;
                    quote_char = chars[i];
                } else if chars[i] == quote_char {
                    // Check for escaping
                    let mut escaped = false;
                    if i > 0 && chars[i-1] == '\\' {
                        let mut backslash_count = 1;
                        let mut j = i - 1;
                        while j > 0 && chars[j-1] == '\\' {
                            backslash_count += 1;
                            j -= 1;
                        }
                        escaped = backslash_count % 2 == 1;
                    }
                    if !escaped {
                        in_quotes = false;
                    }
                }
            }
            '(' if !in_quotes => paren_depth += 1,
            ')' if !in_quotes => paren_depth -= 1,
            '[' if !in_quotes => bracket_depth += 1,
            ']' if !in_quotes => bracket_depth -= 1,
            _ => {}
        }
    }
    
    vec![s.to_string()] // Return original if no operator found
}

fn is_multiline_match_expression(lines: &[&str]) -> bool {
    if lines.is_empty() {
        return false;
    }
    
    let first_line = lines[0].trim();
    if !first_line.starts_with("match ") {
        return false;
    }
    
    // Look for continuation patterns
    for line in lines.iter().skip(1) {
        let trimmed = line.trim();
        if trimmed.starts_with('}') || trimmed == ";;" {
            return true;
        }
        if trimmed.contains("=>") || trimmed.contains("otherwise") || trimmed.contains("|") {
            continue;
        }
    }
    
    false
}

// Enhanced conditional detection
fn is_multiline_conditional(input: &str) -> bool {
    let lines: Vec<&str> = input.lines().collect();
    if lines.len() < 2 {
        return false;
    }
    
    // Check if it contains pipe operators for conditionals
    let mut has_pipe = false;
    let mut has_when = false;
    
    for line in &lines {
        if line.contains('|') {
            has_pipe = true;
        }
        if line.contains("when") || line.contains("otherwise") || line.contains("else") {
            has_when = true;
        }
    }
    
    has_pipe && has_when
}

// In expr.rs, enhance your parsing functions
fn parse_multiline_match_expression(lines: &[&str]) -> Result<Expr, String> {
    if lines.is_empty() {
        return Err("Empty match expression".to_string());
    }
    
    let mut full_expression = String::new();
    for line in lines {
        full_expression.push_str(line.trim());
        full_expression.push(' ');
    }
    
    // Now parse as regular match expression
    parse_match_expression(&full_expression)
}

fn parse_match_expression(input: &str) -> Result<Expr, String> {
    let input = input.trim();
    if !input.starts_with("match ") {
        return Err("Not a match expression".to_string());
    }
    
    // Parse: match value { pattern1 => result1 | pattern2 => result2 | otherwise => default }
    // This is a simplified example - you'll need to implement full match parsing
    
    // For now, treat as complex conditional
    parse_conditional_expression(input)
}

pub fn parse_variable_with_type(var_str: &str) -> Result<(String, Option<SimpleType>), String> {
    let var_str = var_str.trim();
    
    // Parse patterns like: "name:type" or just "name"
    if let Some(colon_pos) = var_str.find(':') {
        let var_name = var_str[..colon_pos].trim();
        let type_str = var_str[colon_pos+1..].trim();
        
        if var_name.is_empty() {
            return Err("Variable name cannot be empty".to_string());
        }
        
        let var_type = match type_str.to_lowercase().as_str() {
            "string" | "str" => Some(SimpleType::String),
            "int" | "integer" => Some(SimpleType::Integer),
            "float" | "double" => Some(SimpleType::Float),
            "bool" | "boolean" => Some(SimpleType::Boolean),
            "list" => Some(SimpleType::List),
            "dict" | "dictionary" => Some(SimpleType::Dictionary),
            "json" => Some(SimpleType::Json),
            "any" => Some(SimpleType::Any),
            _ => return Err(format!("Unknown type: {}", type_str)),
        };
        
        Ok((var_name.to_string(), var_type))
    } else {
        // No type annotation
        Ok((var_str.to_string(), None))
    }
}

pub fn parse_propagation_suffix(input: &str) -> Result<(String, usize, usize), String> {
    let input = input.trim();
    
    // Look for ~ syntax at the end
    if let Some(tilde_pos) = input.rfind('~') {
         
        let (expr_part, control_part) = input.split_at(tilde_pos);
        let control_str = control_part[1..].trim(); // Skip the '~'

        
        
        if control_str.starts_with('+') {
            // ~+N syntax: become immune after N propagations
            let limit_str = &control_str[1..]; // Skip the '+'
            if let Ok(limit) = limit_str.parse::<usize>() {
                if limit == 0 {
                    return Err("~+0 is not valid - use a positive number".to_string());
                }
                let result = (expr_part.trim().to_string(), 0, limit);
                 
                return Ok(result); // delay=0, limit=N
            } else {
                return Err(format!("Invalid number after ~+: {}", limit_str));
            }
        } else if control_str.starts_with('-') {
            // ~-N syntax: delay propagation for N changes
            let delay_str = &control_str[1..]; // Skip the '-'
            if let Ok(delay) = delay_str.parse::<usize>() {
                let result = (expr_part.trim().to_string(), delay, usize::MAX);
                 
                return Ok(result); // delay=N, limit=infinite
            } else {
                return Err(format!("Invalid number after ~-: {}", delay_str));
            }
        } else {
            return Err(format!("Invalid propagation control syntax: {}", control_part));
        }
    }
    
    // No propagation control syntax found
    let result = (input.to_string(), 0, usize::MAX);
     
    Ok(result) // No delay, no limit
}

#[cfg(test)]
mod method_chain_tests {
    use super::*;
    use crate::core::env::Env;
    use crate::core::types::Value;
    use std::collections::HashMap;

    #[test]
    fn dict_keys_len_chain() {
        let mut env = Env::new();
        let mut map = HashMap::new();
        map.insert("a".to_string(), Value::Int(1));
        map.insert("b".to_string(), Value::Int(2));
        env.set_direct("d", Value::Dict(map));

        let expr = parse_expression("d.keys().len()").expect("parse expression");
        let val = evaluate(&expr, &env).expect("evaluate");
        assert_eq!(val, Value::Int(2));
    }
}