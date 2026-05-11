use morris::core::env::Env;
use morris::core::expr::{parse_expression, evaluate};
use morris::core::types::{Value};

#[test]
fn test_dict_keys_len_chain() {
    let mut env = Env::new();
    let mut map = std::collections::HashMap::new();
    map.insert("a".to_string(), Value::Int(1));
    map.insert("b".to_string(), Value::Int(2));
    env.set_direct("d", Value::Dict(map));

    let expr = parse_expression("d.keys().len()").expect("parse");
    let result = evaluate(&expr, &env).expect("eval");
    assert_eq!(result, Value::Int(2));
}
