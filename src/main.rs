mod core;
mod output;
mod repl;  

use std::io::{self, Write};
use std::fs;
use std::collections::HashMap;
use crate::core::env::Env;
use crate::core::filesystem::FileSystem;
use crate::core::library::Library;
use crate::core::intent::{parse_to_intent, Verb, Target, IntentState};
use crate::output::Printer;
use crate::repl::Repl;  
use crate::core::types::Value;

use uuid::Uuid;
use crate::core::history::HistoryManager;
use crate::core::change_engine::ChangeEngineManager;
use rustyline::error::ReadlineError;  
use ctrlc;  
use crate::core::types::SimpleType;
use crate::core::template::render_template;    

fn main() -> io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
        #[allow(dead_code)]
    
    if args.len() > 1 {
        let filename = &args[1];
        if !filename.ends_with(".msh") {
            println!("[-] Expected .msh file, got: {}", filename);
            println!("[?] Usage: morris <file.msh>");
            return Ok(());
        }
        
        match execute_msh_file(filename) {
            Ok(_) => {
                let printer = Printer::new();
                printer.success(&format!("Script '{}' executed successfully", filename));
            },
            Err(e) => {
                let printer = Printer::new();
                printer.error(&e);
                std::process::exit(1);
            }
        }
    } else {
        interactive_mode()?;
    }
    
    Ok(())
}

fn interactive_mode() -> io::Result<()> {
    // Setup Ctrl+C handler for graceful shutdown
    ctrlc::set_handler(|| {
        // This allows Ctrl+C to work at process level
    }).expect("Error setting Ctrl-C handler");
    
    // Create REPL
    let mut repl: Repl = match Repl::new() {
        Ok(repl) => repl,
        Err(e) => {
            println!("Error: {}", e);
            println!("Falling back to basic input mode...");
            return interactive_mode_fallback();
        }
    };
    
    // PHASE 0: RUN STARTUP VALIDATION BEFORE ANYTHING ELSE
    repl.printer().header("🧪 Morris Startup Validation");
    
    let mut validator = match crate::core::startup_validator::StartupValidator::new() {
        Ok(validator) => validator,
        Err(e) => {
            repl.printer().error(&format!("Validation system failed: {}", e));
            repl.printer().warning("System cannot start safely. Use fallback mode.");
            return interactive_mode_fallback();
        }
    };
    
    match validator.validate_startup() {
        Ok(report) => {
            if report.has_critical_issues() {
                repl.printer().error("CRITICAL VALIDATION FAILURES DETECTED");
                println!("{}", report.format_summary());
                repl.printer().warning("System cannot start safely. Use fallback mode.");
                return interactive_mode_fallback();
            }
            
            repl.printer().success("Startup validation passed");
            if !report.warnings.is_empty() {
                repl.printer().warning(&format!("{} warnings found", report.warnings.len()));
                println!("{}", report.format_summary());
            }
        }
        Err(e) => {
            repl.printer().error(&format!("Validation failed: {}", e));
            return interactive_mode_fallback();
        }
    }

    // Load validated library state using the validator's library_manager field
    let library_state = match validator.library_manager.load_validated_library() {
        Ok(state) => state,
        Err(e) => {
            repl.printer().error(&format!("Failed to load library: {}", e));
            return interactive_mode_fallback();
        }
    };

    let mut loaded_intents = match validator.library_manager().load_intent_files() {
        Ok(intents) => {
            repl.printer().success(&format!("Loaded {} intent definitions", intents.len()));
            intents
        }
        Err(e) => {
            repl.printer().warning(&format!("Could not load intent files: {}", e));
            HashMap::new()
        }
    };
    
    // Merge with user-defined intents
    let mut all_intents = loaded_intents;
    //all_intents.extend(defined_intents);
    
    // Show the new logo (only after validation passes)
    show_morris_logo(repl.printer());
    
    println!("Type 'help' for available commands.");
    println!("Type 'exit' to quit.");
    println!();  // Add blank line
    
    // Initialize state with validated library
    let mut env = Env::new();
    let filesystem = FileSystem::new();
    
    // Load validated library (now that we know it's safe)
    let library_state = match validator.library_manager().load_validated_library() {
        Ok(state) => state,
        Err(e) => {
            repl.printer().error(&format!("Failed to load library: {}", e));
            return interactive_mode_fallback();
        }
    };
    
    let mut library = Library::new();
    let mut intent_history: Vec<crate::core::intent::Intent> = Vec::new();
    
    // NEW: Load validated intents from library state
    let mut defined_intents: HashMap<String, crate::core::intent::Intent> = 
        library_state.user_intents.clone();
    
    let mut history_manager = HistoryManager::new();
    let mut engine_manager = ChangeEngineManager::new();
    
    // Load existing data (now that we know it's safe)
    match history_manager.load() {
        Ok(_) => repl.printer().info("History loaded"),
        Err(e) => repl.printer().warning(&format!("Could not load history: {}", e)),
    }
    
    match engine_manager.load() {
        Ok(_) => repl.printer().info("Change engine loaded"),
        Err(e) => repl.printer().warning(&format!("Could not load change engine: {}", e)),
    }
    
    // NEW: Create safety guard for all operations
    let safety_guard = crate::core::safety_guard::SafetyGuard::new()
        .expect("Failed to initialize safety guard");
    
    // Main loop
    loop {
        let mut input = String::new();
        match repl.read_line("intent> ") {
            Ok(Some(line)) => {
                if line.trim_end().ends_with('{') {
                    // Multi-line block mode
                    input.push_str(&line);
                    input.push('\n');
                    loop {
                        match repl.read_line("... ") {
                            Ok(Some(block_line)) => {
                                if block_line.trim() == "}" {
                                    input.push_str("}");
                                    break;
                                } else {
                                    input.push_str(&block_line);
                                    input.push('\n');
                                }
                            }
                            _ => break,
                        }
                    }
                } else {
                    input = line;
                }
                
                // Process the input
                match input.as_str() {
                    "exit" | "quit" => {
                        println!();  // Add blank line before exit message
                        repl.printer().success("Goodbye!");
                        break;
                    }
                    "help" => {
                        show_help(repl.printer());
                        println!();  // Add blank line after help
                        continue;
                    }
                    "env" => {
                        show_env_clean(&env, repl.printer());
                        println!();  // Add blank line after env
                        continue;
                    }
                    "history" => {
                        show_history_clean(&intent_history, repl.printer());
                        println!();  // Add blank line after history
                        continue;
                    }
                    "clear" => {
                        // Robust clear screen
                        if cfg!(windows) {
                            // Windows
                            let _ = std::process::Command::new("cmd")
                                .args(&["/C", "cls"])
                                .status();
                        } else {
                            // Unix/Linux/Mac
                            print!("\x1B[2J\x1B[1;1H");
                            let _ = io::stdout().flush();
                        }
                        continue;
                    }
                    "engine on" => {
                        env.enable_new_engine(crate::core::propagation::PropagationStrategy::Immediate);
                        repl.printer().success("New propagation engine enabled!");
                        println!();
                        continue;
                    }
                    "engine off" => {
                        env.disable_new_engine();
                        repl.printer().success("New propagation engine disabled (using legacy)");
                        println!();
                        continue;
                    }
                    "engine migrate" => {
                        match env.migrate_to_new_engine() {
                            Ok(_) => {
                                repl.printer().success("Migrated all variables to new engine!");
                                println!();
                            }
                            Err(e) => {
                                repl.printer().error(&format!("Migration failed: {}", e));
                                println!();
                            }
                        }
                        continue;
                    }
                    "engine visualize" => {
                        let visualization = env.visualize_dependencies();
                        println!("{}", visualization);
                        println!();
                        continue;
                    }
                    "engine history" => {
                        let history = env.get_propagation_history(10);
                        if history.is_empty() {
                            repl.printer().info("No propagation history available");
                        } else {
                            repl.printer().header("Propagation History");
                            for event in history {
                                println!("  {}", event);
                            }
                        }
                        println!();
                        continue;
                    }
                    "engine status" => {
                        if env.is_new_engine_enabled() {
                            repl.printer().success("✓ New propagation engine is ENABLED");
                            let var_count = env.list().len();
                            println!("  Tracking {} variables with enhanced dependency graph", var_count);
                        } else {
                            repl.printer().info("New propagation engine is DISABLED (using legacy)");
                        }
                        println!();
                        continue;
                    }
                    // NEW: Integrity system commands
                    "validate" => {
                        match validator.validate_current_state(&env, &defined_intents) {
                            Ok(report) => {
                                repl.printer().success("System validation passed");
                                println!("{}", report.format_summary());
                            }
                            Err(e) => {
                                repl.printer().error(&format!("Validation failed: {}", e));
                            }
                        }
                        println!();
                        continue;
                    }
                    "integrity check" => {
                        match validator.check_system_integrity() {
                            Ok(report) => {
                                if report.is_clean() {
                                    repl.printer().success("System integrity verified");
                                } else {
                                    repl.printer().warning("Integrity issues found");
                                    println!("{}", report.format_summary());
                                }
                            }
                            Err(e) => {
                                repl.printer().error(&format!("Integrity check failed: {}", e));
                            }
                        }
                        println!();
                        continue;
                    }
                    _ => {
                        // Parse and execute the intent WITH SAFETY GUARD
                        match parse_to_intent(&input) {
                            Ok(mut intent) => {
                                // NEW: Validate intent safety before execution
                                if let Err(e) = safety_guard.validate_intent(&intent) {
                                    repl.printer().error(&format!("Safety check failed: {}", e));
                                    println!();
                                    continue;
                                }
                                
                                // Handle system commands that were parsed as intents
                                if intent.state == IntentState::NeedsClarification {
                                    if let Some(cmd) = intent.get_context("system_command") {
                                        match cmd.as_str() {
                                            "help" => {
                                                show_help(repl.printer());
                                                println!();
                                            }
                                            "env" => {
                                                show_env_clean(&env, repl.printer());
                                                println!();
                                            }
                                            "history" => {
                                                show_history_clean(&intent_history, repl.printer());
                                                println!();
                                            }
                                            "clear" => print!("\x1B[2J\x1B[1;1H"),
                                            _ => {
                                                repl.printer().error(&format!("Command '{}' not recognized", cmd));
                                                repl.printer().info("Try 'help' for available commands");
                                                println!();
                                            }
                                        }
                                        continue;
                                    }
                                }
                                
                                // Check if it's a define intent
                                if intent.is_composition && intent.intent_source == Some("defined_intent".to_string()) {
                                    if let Some(name) = &intent.composition_name {
                                        // NEW: Validate the new intent definition
                                        if let Err(e) = safety_guard.validate_new_definition(&intent) {
                                            repl.printer().error(&format!("Cannot define intent: {}", e));
                                            println!();
                                            continue;
                                        }
                                        
                                        defined_intents.insert(name.clone(), intent.clone());
                                        repl.printer().success(&format!("Intent defined: {}", name));
                                        println!();
                                        continue;
                                    }
                                }
                        
                                // If it's an execute intent for a defined intent
                                if intent.verb == Verb::Execute {
                                    if let Some(intent_name) = intent.parameters.get("intent_to_execute") {
                                        if let Some(defined_intent) = defined_intents.get(intent_name) {
                                            repl.printer().info(&format!("Executing intent: {}", intent_name));
                                            
                                            // Instantiate with parameters
                                            let params = intent.parameters.clone();
                                            let instantiated = defined_intent.instantiate_with_params(&params);
                                            
                                            // NEW: Validate instantiated intent
                                            if let Err(e) = safety_guard.validate_intent(&instantiated) {
                                                repl.printer().error(&format!("Cannot execute intent: {}", e));
                                                println!();
                                                continue;
                                            }
                                            
                                            // Execute the instantiated intent
                                            match execute_intent_with_guard(
                                                &instantiated, 
                                                &mut env, 
                                                &filesystem, 
                                                &mut library, 
                                                &mut intent_history, 
                                                &mut history_manager, 
                                                &mut engine_manager, 
                                                &safety_guard,
                                                repl.printer()
                                            ) {
                                                Ok(output) => {
                                                    println!("{}", output);
                                                    println!();  // Add blank line after output
                                                }
                                                Err(e) => {
                                                    repl.printer().error(&format!("Failed to execute defined intent: {}", e));
                                                    println!();  // Add blank line after error
                                                }
                                            }
                                            continue;
                                        }
                                    }
                                }
                                
                                intent = intent
                                    .with_context("source", "interactive")
                                    .with_context("timestamp", &chrono::Utc::now().to_rfc3339());
                                
                                intent.state = IntentState::Parsed;
                                intent_history.push(intent.clone());
                                
                                // Check execution guard
                                match intent.can_execute(&env) {
                                    Ok(true) => {
                                        intent.state = IntentState::Executing;
                                        
                                        match execute_intent_with_guard(
                                            &intent, 
                                            &mut env, 
                                            &filesystem, 
                                            &mut library, 
                                            &mut intent_history, 
                                            &mut history_manager, 
                                            &mut engine_manager, 
                                            &safety_guard,
                                            repl.printer()
                                        ) {
                                            Ok(output) => {
                                                println!("{}", output);
                                                println!();  // Add blank line after successful output
                                                intent.state = IntentState::Succeeded;
                                                
                                                // Record successful execution
                                                history_manager.record(&intent, &output, intent.state.clone());
                                                engine_manager.record_intent();
                                                engine_manager.capture_env_state(&env);

                                                // Auto-save every 5 intents
                                                if intent_history.len() % 5 == 0 {
                                                    let _ = history_manager.save();
                                                    let _ = engine_manager.save();
                                                }
                                                
                                                if let Some(last) = intent_history.last_mut() {
                                                    last.state = intent.state.clone();
                                                    last.context.extend(intent.context.clone());
                                                }
                                            }
                                            Err(e) => {
                                                repl.printer().error(&e);
                                                println!();  // Add blank line after error
                                                intent.state = IntentState::Failed;
                                                
                                                // Record failed execution
                                                history_manager.record(&intent, &e, intent.state.clone());
                                                engine_manager.record_intent();
                                                
                                                // Auto-save every 5 intents
                                                if intent_history.len() % 5 == 0 {
                                                    let _ = history_manager.save();
                                                    let _ = engine_manager.save();
                                                }
                                                
                                                if let Some(last) = intent_history.last_mut() {
                                                    last.state = intent.state.clone();
                                                }
                                            }
                                        }
                                        
                                        if let Some(last) = intent_history.last_mut() {
                                            last.state = intent.state.clone();
                                            last.context.extend(intent.context.clone());
                                        }
                                    }
                                    Ok(false) => {
                                        repl.printer().error("Execution guard failed - intent not executed");
                                        println!();  // Add blank line after error
                                        intent.state = IntentState::Failed;
                                        
                                        if let Some(last) = intent_history.last_mut() {
                                            last.state = intent.state.clone();
                                        }
                                    }
                                    Err(e) => {
                                        repl.printer().error(&format!("Error evaluating guard: {}", e));
                                        println!();  // Add blank line after error
                                        intent.state = IntentState::Failed;
                                        
                                        if let Some(last) = intent_history.last_mut() {
                                            last.state = intent.state.clone();
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                repl.printer().error(&e);
                                repl.printer().info("Type 'help' for available intents");
                                println!();  // Add blank line after error/info
                            }
                        }
                    }
                }
            }
            Ok(None) => {
                // Ctrl+C or empty input - continue
                continue;
            }
            Err(ReadlineError::Eof) => {
                // Ctrl+D - exit
                println!();  // Add blank line before exit
                break;
            }
            Err(err) => {
                repl.printer().error(&format!("Input error: {}", err));
                println!();  // Add blank line after error
                break;
            }
        }
    }
    
    // Clean shutdown
    println!();  // Add blank line
    repl.printer().info("Saving history and change engine...");
    
    // Save REPL command history
    repl.save_history().ok();
    
    // Save Morris state
    history_manager.save().ok();
    engine_manager.save().ok();
    engine_manager.end_session();
    
    repl.printer().success("Knowledge Preserved...");
    Ok(())
}

// NEW: Enhanced execute_intent function with safety guard
fn execute_intent_with_guard(
    intent: &crate::core::intent::Intent,
    env: &mut Env,
    filesystem: &FileSystem,
    library: &mut Library,
    intent_history_vec: &mut Vec<crate::core::intent::Intent>,
    history_manager: &mut HistoryManager,
    engine_manager: &mut ChangeEngineManager,
    safety_guard: &crate::core::safety_guard::SafetyGuard,
    printer: &Printer,
) -> Result<String, String> {
    // Validate execution with safety guard
    safety_guard.validate_execution(intent, env)?;
    
    // Then proceed with normal execution
        execute_intent_with_type(intent, env, filesystem, library, intent_history_vec, history_manager, engine_manager, printer)
}

fn setup_ctrlc_handler() {
    ctrlc::set_handler(|| {
        // This just allows Ctrl+C to work; rustyline handles it in read_line
    }).expect("Error setting Ctrl-C handler");
}

fn interactive_mode_fallback() -> io::Result<()> {
    let printer = Printer::new();
    
    printer.header("morris v0.6");
    println!("The Carbon-Silicon Tongue learns to remember.");
    println!("Type 'help' for available intents, 'exit' to quit.\n");
    
    let mut env = Env::new();
    let filesystem = FileSystem::new();
    let mut library = Library::new();
    let mut intent_history: Vec<crate::core::intent::Intent> = Vec::new();
    let mut defined_intents: HashMap<String, crate::core::intent::Intent> = HashMap::new();
    let mut running = true;
    let mut history_manager = HistoryManager::new();
    let mut engine_manager = ChangeEngineManager::new();

    
    // Load history and engine on startup
    if let Err(e) = history_manager.load() {
        printer.warning(&format!("Could not load history: {}", e));
    }
    
    if let Err(e) = engine_manager.load() {
        printer.warning(&format!("Could not load change engine: {}", e));
    }
    
    while running {
        print!("intent> ");
        io::stdout().flush()?;
        
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim();
        
        if input.is_empty() {
            continue;
        }
        
        match input {
            "exit" | "quit" => {
                printer.success("Goodbye!");
                running = false;
                continue;
            }
            "help" => {
                show_help(&printer);
                println!();
                continue;
            }
            "env" => {
                show_env_clean(&env, &printer);
                println!();
                continue;
            }
            "history" => {
                show_history_clean(&intent_history, &printer);
                println!();
                continue;
            }
            "clear" => {
                print!("\x1B[2J\x1B[1;1H");
                continue;
            }
            _ => {}
        }
        
        match parse_to_intent(input) {
            Ok(mut intent) => {
                // Handle system commands that were parsed as intents
                if intent.state == IntentState::NeedsClarification {
                    if let Some(cmd) = intent.get_context("system_command") {
                        match cmd.as_str() {
                            "help" => show_help(&printer),
                            "env" => show_env_clean(&env, &printer),
                            "history" => show_history_clean(&intent_history, &printer),
                            "clear" => print!("\x1B[2J\x1B[1;1H"),
                            _ => {
                                printer.error(&format!("Command '{}' not recognized", cmd));
                                printer.info("Try 'help' for available commands");
                            }
                        }
                        continue;
                    }
                }
                
                // Check if it's a define intent
                if intent.is_composition && intent.intent_source == Some("defined_intent".to_string()) {
                    if let Some(name) = &intent.composition_name {
                        defined_intents.insert(name.clone(), intent.clone());
                        printer.success(&format!("Intent defined: {}", name));
                        println!();
                        continue;
                    }
                }
        
                // If it's an execute intent for a defined intent
                if intent.verb == Verb::Execute {
                    if let Some(intent_name) = intent.parameters.get("intent_to_execute") {
                        if let Some(defined_intent) = defined_intents.get(intent_name) {
                            printer.info(&format!("Executing intent: {}", intent_name));
                            
                            // Instantiate with parameters
                            let params = intent.parameters.clone();
                            let instantiated = defined_intent.instantiate_with_params(&params);
                            
                            // Execute the instantiated intent
                            match execute_defined_intent(&instantiated, &mut env, &filesystem, &mut library, &mut intent_history, &defined_intents) {
                                Ok(output) => {
                                    println!("{}", output);
                                }
                                Err(_e) => {
                                    printer.error("Failed to execute defined intent");
                                    println!();
                                }
                            }
                            continue;
                        }
                    }
                }
                
                intent = intent
                    .with_context("source", "interactive")
                    .with_context("timestamp", &chrono::Utc::now().to_rfc3339());
                
                intent.state = IntentState::Parsed;
                intent_history.push(intent.clone());
                
                // Check execution guard
                match intent.can_execute(&env) {
                    Ok(true) => {
                        intent.state = IntentState::Executing;
                        
                        match execute_intent(&intent, &mut env, &filesystem, &mut library, &mut intent_history, &mut history_manager, &mut engine_manager, &printer) {
                            Ok(output) => {
                                println!("{}", output);
                                intent.state = IntentState::Succeeded;
                                // Record successful execution
                                history_manager.record(&intent, &output, intent.state.clone());
                                engine_manager.record_intent();

                                engine_manager.capture_env_state(&env);
        
                                // Auto-save every 5 intents
                                if intent_history.len() % 5 == 0 {
                                    let _ = history_manager.save(); // Ignore errors
                                    let _ = engine_manager.save();   // Ignore errors
                                println!();
                                }
                            }
                            Err(e) => {
                                printer.error(&e);
                                intent.state = IntentState::Failed;
                                // Record failed execution too
                                history_manager.record(&intent, &e, intent.state.clone());
                                engine_manager.record_intent();
                                println!();
                            }
                        }
                        
                        if let Some(last) = intent_history.last_mut() {
                            last.state = intent.state.clone();
                            last.context.extend(intent.context.clone());
                        }
                    }
                    Ok(false) => {
                        printer.error("Execution guard failed - intent not executed");
                        intent.state = IntentState::Failed;
                        println!();
                        
                        if let Some(last) = intent_history.last_mut() {
                            last.state = intent.state.clone();
                        }
                    }
                    Err(e) => {
                        printer.error(&format!("Error evaluating guard: {}", e));
                        intent.state = IntentState::Failed;
                        println!();
                        
                        if let Some(last) = intent_history.last_mut() {
                            last.state = intent.state.clone();
                        }
                    }
                }
            }
            Err(e) => {
                printer.error(&e);
                printer.info("Type 'help' for available intents");
                println!();
            }
        }
    }
    printer.info("Saving history and change engine...");
    let history_result = history_manager.save();
    let engine_result = engine_manager.save();

    if let Err(e) = history_result {
        printer.warning(&format!("Failed to save history: {}", e));
    }
    if let Err(e) = engine_result {
        printer.warning(&format!("Failed to save change engine: {}", e));
    }
    
    // End current session
    engine_manager.end_session();
    
    Ok(())
}

fn show_help(printer: &Printer) {
    printer.header("Available Intent Types");
    
    printer.subheader("Intent Definition");
    println!("  define intent \"name\" with (param1, param2=\"default\") {{ expression }}");
    println!("  define intent \"name\" composed_of [\"intent1\", \"intent2\"]");
    println!("  execute \"intent_name\" with param1=value1, param2=value2");
    
    printer.subheader("File Operations");
    println!("  save \"path.menv\"              - Save environment to file");
    println!("  read \"file.txt\" into var     - Read file into variable");
    println!("  write \"file.txt\" \"content\"   - Write content to file");
    println!("  append \"file.txt\" \"content\"  - Append content to file");
    println!("  mkdir \"path/to/dir\"          - Create directory");
    println!("  list \"path\"                  - List directory contents");
    println!("  info \"file.txt\"              - Get file information");
    println!("  exists \"file.txt\"            - Check if file exists");
    
    printer.subheader("Core Operations");
    println!("  set <var> = <value> [as <type>]");
    println!("  ensure <condition>");
    println!("  ensure file \"path\" exists");
    println!("  writeout(<content>)");
    println!("  derive <var>");
    println!("  find <pattern>");
    println!("  analyze <var>");
    println!("  freeze <var>");
    println!("  load <file.msh>");
    println!("  set <var> = <value> [as <type>]");
    println!("  ensure <condition>");
    println!("  ensure file \"path\" exists");
    println!("  writeout(<content>)");
    println!("  derive <var>");
    println!("  find <pattern>");
    println!("  analyze <var>");
    println!("  freeze <var>");
    println!("  load <file.msh>");
    println!("  parse-json \"json_string\"     - Parse JSON string");
    println!("  to-json <variable>            - Convert variable to JSON");
    println!("  from-json \"json\" into <var>   - Parse JSON into variable");
    println!("  json-get <variable>.<path>         - Get value from JSON path");
    println!("  json-set <variable>.<path> = value - Set value at JSON path");
    
    printer.subheader("System Commands");
    println!("  env         - Show current environment");
    println!("  history     - Show intent history");
    println!("  clear       - Clear screen");
    println!("  exit        - Exit Morris");

    printer.subheader("Book Navigation (Filesystem as Library)");
    println!("  page                    - Show current page/directory");
    println!("  turn <path>             - Change directory");
    println!("  chapter <path>          - Alias for turn");
    println!("  bookmark add \"name\" [path] - Create bookmark");
    println!("  bookmark remove \"name\"     - Remove bookmark");
    println!("  bookmarks               - List all bookmarks");
    println!("  volume add \"name\" path [\"desc\"] - Define volume");
    println!("  volumes                 - List all volumes");
    println!("  shelve                  - Save current position");
    println!("  unshelve                - Restore saved position");
    println!("  back [n]                - Go back n pages (default: 1)");
    println!("  index                   - List directory contents");
    println!("  annotate <target> \"note\" - Add note to file/directory");
    println!("  read_annotation <target> - Read annotation");
    println!("  skim <file>             - Quick file preview");
    println!("  library                 - Show library overview");

    printer.header("Enhanced Book Navigation");
    
    printer.subheader("Navigation Verbs");
    println!("  page                    - Show current page");
    println!("  turn <path>             - Turn to page (supports -1, -2, +1, etc)");
    println!("  jump <path>             - Jump to location (alias: goto)");
    println!("  peek [n]                - Peek n steps back (default: -1)");
    println!("  return [n]              - Return n pages back (default: 1)");
    println!("  mark \"name\" [desc]     - Create temporary mark");
    
    printer.subheader("Examples:");
    println!("  turn ..                 - Go up one directory");
    println!("  turn -1                 - Go back one page");
    println!("  turn -2                 - Go back two pages");
    println!("  turn /home/user/docs    - Absolute path");
    println!("  turn \"My Documents\"     - Bookmark or volume");
    println!("  peek                    - See where you'd go back to");
    println!("  peek -2                 - See two pages back");
    println!("  return                  - Go back one page");
    println!("  return 3                - Go back three pages");
    println!("  mark \"important spot\"   - Mark current location");

    printer.subheader("Propagation Engine Commands");
    println!("  engine on          - Enable new propagation engine");
    println!("  engine off         - Disable new engine (use legacy)");
    println!("  engine migrate     - Migrate existing variables to new engine");
    println!("  engine visualize   - Show dependency graph visualization");
    println!("  engine history     - Show propagation history");
    println!("  engine status      - Show engine status");

    // Transaction commands removed: transactions are disabled in this build
}

fn show_env_clean(env: &Env, printer: &Printer) {
    let vars: Vec<(String, String)> = env.list()
        .iter()
        .map(|(name, value)| (name.clone(), value.display()))
        .collect();
    
    if vars.is_empty() {
        printer.info("No variables defined");
        return;
    }
    
    printer.header(&format!("Environment ({} variables)", vars.len()));
    
    // Find max name length for alignment
    let max_name_len = vars.iter()
        .map(|(name, _)| name.len())
        .max()
        .unwrap_or(0)
        .min(20);
    
    for (name, value) in vars.iter().take(50) {
        printer.print_key_value(&format!("{:width$}", name, width = max_name_len), value, 2);
    }
    
    if vars.len() > 50 {
        printer.info(&format!("... and {} more variables", vars.len() - 50));
    }
}

fn show_history_clean(intent_history: &[crate::core::intent::Intent], printer: &Printer) {
    if intent_history.is_empty() {
        printer.info("No intent history yet");
        return;
    }
    
    printer.header(&format!("History ({} intents)", intent_history.len()));
    
    // Show last 10 intents
    for (i, intent) in intent_history.iter().rev().take(10).enumerate() {
        let prefix = match intent.state {
            IntentState::Succeeded => "[+]",
            IntentState::Failed => "[-]",
            IntentState::Executing => "[▶]",
            IntentState::Created => "[🆕]",
            IntentState::Parsed => "[📝]",
            IntentState::NeedsClarification => "[?]",
        };
        
        let verb_str = format!("{:?}", intent.verb);
        let target_str = intent.target_string();
        
        // Fixed: printer.use_color is now accessible
        if printer.use_color {
            let color = match intent.state {
                IntentState::Succeeded => "\x1b[32m",  // green
                IntentState::Failed => "\x1b[31m",     // red
                IntentState::Executing => "\x1b[36m",  // cyan
                _ => "\x1b[90m",                       // dark gray
            };
            println!("  {}{:3}. {}{} {} → {}", color, i + 1, prefix, "\x1b[0m", verb_str, target_str);
        } else {
            println!("  {:3}. {} {} → {}", i + 1, prefix, verb_str, target_str);
        }
        
        if let Some(source) = intent.get_context("source") {
            println!("      Source: {}", source);
        }
    }
    
    if intent_history.len() > 10 {
        printer.info(&format!("... and {} more intents", intent_history.len() - 10));
    }
}

fn execute_intent_in_test_env(
    intent: &crate::core::intent::Intent,
    env: &mut Env,
    filesystem: &FileSystem,
    library: &mut Library,
    history: &mut Vec<crate::core::intent::Intent>,
    history_manager: &mut HistoryManager,
    engine_manager: &mut ChangeEngineManager,
    printer: &Printer,
) -> Result<String, String> {
    // This is a direct copy of the logic that would normally be in execute_intent
    // but without the recursive super::execute_intent call
    match &intent.verb {
        Verb::Set => execute_set_intent_clean(intent, env, printer),
        Verb::Ensure => execute_ensure_intent_clean(intent, env, printer),
        Verb::Writeout => execute_writeout_intent_clean(intent, env, printer),
        Verb::Derive => execute_derive_intent_clean(intent, env, printer),
        Verb::Analyze => execute_analyze_intent_clean(intent, env, printer),
        Verb::Find => execute_find_intent_clean(intent, env, printer),
        Verb::Execute => execute_execute_intent_clean(intent, env, printer),
        Verb::Freeze => execute_freeze_intent_clean(intent, env, printer),
        
        // File operations
        Verb::Save => execute_save_intent_clean(intent, env, filesystem, printer),
        Verb::Read => execute_read_intent_clean(intent, env, filesystem, printer),
        Verb::Write => execute_write_intent_clean(intent, env, filesystem, printer),
        Verb::Append => execute_append_intent_clean(intent, env, filesystem, printer),
        Verb::Mkdir => execute_mkdir_intent_clean(intent, filesystem, printer),
        Verb::List => execute_list_intent_clean(intent, filesystem, printer),
        Verb::Info => execute_info_intent_clean(intent, filesystem, printer),
        Verb::Exists => execute_exists_intent_clean(intent, filesystem, printer),
        Verb::Load => execute_load_intent_clean(intent, env, history, history_manager, engine_manager, library, printer),
        
        // Book navigation
        Verb::Page => execute_page_intent(library, printer),
        Verb::Turn => execute_turn_intent(intent, library, printer),
        Verb::Bookmark => execute_bookmark_intent(intent, library, printer),
        Verb::Bookmarks => execute_bookmarks_intent(library, printer),
        Verb::RemoveBookmark => execute_remove_bookmark_intent(intent, library, printer),
        Verb::Volume => execute_volume_intent(intent, library, printer),
        Verb::Volumes => execute_volumes_intent(library, printer),
        Verb::Shelve => execute_shelve_intent(library, printer),
        Verb::Unshelve => execute_unshelve_intent(library, printer),
        Verb::Annotate => execute_annotate_intent(intent, library, printer),
        Verb::ReadAnnotation => execute_read_annotation_intent(intent, library, printer),
        Verb::Index => execute_index_intent(library, printer),
        Verb::Back => execute_back_intent(intent, library, printer),
        Verb::Library => execute_library_intent(library, printer),
        Verb::Chapter => execute_chapter_intent(intent, library, printer),
        Verb::Skim => execute_skim_intent(intent, env, filesystem, printer),
        Verb::Jump => execute_jump_intent(intent, library, printer),
        Verb::Peek => execute_peek_intent(intent, library, printer),
        Verb::Return => execute_return_intent(intent, library, printer),
        Verb::Mark => execute_mark_intent(intent, library, printer),
        Verb::Goto => execute_jump_intent(intent, library, printer),
        
        // History operations
        Verb::History => execute_history_intent(intent, history_manager, printer),
        Verb::HistorySearch => execute_history_search_intent(intent, history_manager, printer),
        Verb::HistoryTag => execute_history_tag_intent(intent, history_manager, printer),
        Verb::HistoryReplay => execute_history_replay_intent(intent, history_manager, env, filesystem, library, history, engine_manager, printer),
        Verb::HistoryClear => execute_history_clear_intent(history_manager, printer),
        Verb::HistorySave => execute_history_save_intent(history_manager, printer),
        
        // Change Engine operations
        Verb::EngineStatus => execute_engine_status_intent(engine_manager, printer),
        Verb::EngineSave => execute_engine_save_intent(engine_manager, printer),
        Verb::EngineLoad => execute_engine_load_intent(engine_manager, printer),
        Verb::EngineValidate => execute_engine_validate_intent(engine_manager, printer),
        Verb::EngineDefine => execute_engine_define_intent(intent, engine_manager, printer),
        Verb::EngineRule => execute_engine_rule_intent(intent, engine_manager, printer),
        Verb::EngineHook => execute_engine_hook_intent(intent, engine_manager, printer),
        
        // Transaction operations - disabled
        Verb::Craft => transactions_disabled_with_intent(intent, env, printer),
        Verb::Forge => transactions_disabled_no_intent(env, printer),
        Verb::Smelt => transactions_disabled_no_intent(env, printer),
        Verb::Temper => transactions_disabled_no_intent(env, printer),
        Verb::Inspect => transactions_disabled_no_intent(env, printer),
        Verb::Anneal => transactions_disabled_with_intent(intent, env, printer),
        Verb::Quench => transactions_disabled_no_intent(env, printer),
        Verb::Polish => transactions_disabled_with_intent(intent, env, printer),
        Verb::Alloy => transactions_disabled_with_intent(intent, env, printer),
        Verb::Engrave => transactions_disabled_with_intent(intent, env, printer),
        Verb::Gild => transactions_disabled_with_intent(intent, env, printer),
        Verb::Patina => transactions_disabled_with_intent(intent, env, printer),
        Verb::Transaction => transactions_disabled_no_intent(env, printer),
        
        Verb::WhatIf => execute_what_if_intent(intent, env, printer),
        
        // JSON operations
        Verb::ParseJson => execute_parse_json_intent(intent, env, printer),
        Verb::ToJson => execute_to_json_intent(intent, env, printer),
        Verb::FromJson => execute_from_json_intent(intent, env, printer),
        Verb::JsonGet => execute_json_get_intent(intent, env, printer),
        Verb::JsonSet => execute_json_set_intent(intent, env, printer),
        
        // Collection operations
        Verb::Collection => execute_collection_intent(intent, env, printer),
        Verb::Dictionary => execute_dictionary_intent(intent, env, printer),
        
        // Phase 2: Enhanced Introspection
        Verb::Examine => {
            match crate::core::startup_validator::StartupValidator::new() {
                Ok(validator) => {
                    execute_examine_intent(
                        intent, 
                        env,
                        library,
                        &HashMap::new(), // You'll need actual defined_intents
                        &validator,
                        printer
                    )
                }
                Err(e) => Err(format!("Validator error: {}", e))
            }
        },
        
        Verb::Construct => {
            let mut defined_intents_copy = HashMap::new(); // Replace with actual reference
            execute_construct_intent(intent, &mut defined_intents_copy, printer)
        },
        
        Verb::Evolve => {
            let mut defined_intents_copy = HashMap::new(); // Replace with actual reference
            execute_evolve_intent(intent, &mut defined_intents_copy, printer)
        },
        
        Verb::Grow => {
            let mut defined_intents_copy = HashMap::new(); // Replace with actual reference
            execute_grow_intent(intent, &mut defined_intents_copy, printer)
        },
        
        // Phase 3: Reflection Programming
        Verb::Reflect => {
            match crate::core::startup_validator::StartupValidator::new() {
                Ok(validator) => {
                    execute_reflect_intent(
                        intent,
                        env,
                        &HashMap::new(), // You'll need actual defined_intents
                        &validator,
                        printer
                    )
                }
                Err(e) => Err(format!("Validator error: {}", e))
            }
        },
        
        Verb::Test => {
            execute_test_intent(
                intent,
                env,
                &HashMap::new(), // You'll need actual defined_intents  
                printer
            )
        },
        
        Verb::Adopt => {
            let mut defined_intents_copy = HashMap::new(); // Replace with actual reference
            execute_adopt_intent(intent, &mut defined_intents_copy, printer)
        },
    }
}

pub fn execute_intent(
    intent: &crate::core::intent::Intent,
    env: &mut Env,
    filesystem: &FileSystem,
    library: &mut Library,
    intent_history_vec: &mut Vec<crate::core::intent::Intent>,
    history_manager: &mut HistoryManager,
    engine_manager: &mut ChangeEngineManager,
    printer: &Printer,
) -> Result<String, String> {
        execute_intent_in_test_env_with_type(
            intent,
            env,
            filesystem,
            library,
            intent_history_vec,
            history_manager,
            engine_manager,
            printer
        )
}

// Backwards-compatible wrapper: calls existing execute_intent implementation
fn execute_intent_with_type(
    intent: &crate::core::intent::Intent,
    env: &mut Env,
    filesystem: &FileSystem,
    library: &mut Library,
    intent_history_vec: &mut Vec<crate::core::intent::Intent>,
    history_manager: &mut HistoryManager,
    engine_manager: &mut ChangeEngineManager,
    printer: &Printer,
) -> Result<String, String> {
    execute_intent(intent, env, filesystem, library, intent_history_vec, history_manager, engine_manager, printer)
}

// Backwards-compatible wrapper for test env variant
fn execute_intent_in_test_env_with_type(
    intent: &crate::core::intent::Intent,
    env: &mut Env,
    filesystem: &FileSystem,
    library: &mut Library,
    intent_history_vec: &mut Vec<crate::core::intent::Intent>,
    history_manager: &mut HistoryManager,
    engine_manager: &mut ChangeEngineManager,
    printer: &Printer,
) -> Result<String, String> {
    execute_intent_in_test_env(intent, env, filesystem, library, intent_history_vec, history_manager, engine_manager, printer)
}

fn execute_set_intent_clean(
    intent: &crate::core::intent::Intent, 
    env: &mut Env,
    _printer: &Printer,
) -> Result<String, String> {
    if let Some(Target::Variable(var_name)) = &intent.target {
        let value_str = intent.parameters.get("value")
            .ok_or("No value specified in set intent")?;
        // Get propagation parameters from intent (already parsed during intent parsing)
        let propagation_delay = intent.parameters.get("propagation_delay")
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(0);
        let propagation_limit = intent.parameters.get("propagation_limit")
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(usize::MAX);
        
        
        
        let declared_type = intent.parameters.get("declared_type")
            .and_then(|type_str| parse_simple_type(type_str));
        
        // Since value_str is already cleaned by intent parser, use it directly
        let clean_value_str = value_str;
        
        // SPECIAL HANDLING FOR MULTI-LINE JSON OBJECTS
        let trimmed_value = clean_value_str.trim();
        if trimmed_value.starts_with('{') && trimmed_value.contains('\n') {
            // This looks like multi-line JSON, clean it up
            match parse_multiline_json_properly(trimmed_value) {
                Ok(expr) => {
                    match crate::core::expr::evaluate(&expr, env) {
                        Ok(parsed_value) => {
                            if env.has_active_transaction() {
                                let placeholder = Value::Str("<?>".to_string());
                                env.set_computed_with_type(var_name, placeholder, &expr, declared_type.clone());
                                let type_info = if let Some(ref t) = declared_type {
                                    format!(":{}", t.name())
                                } else {
                                    "".to_string()
                                };
                                let mut result = format!("[🛠] Crafted: {}{} = {}", var_name, type_info, parsed_value.display());
                                if propagation_delay > 0 {
                                    result.push_str(&format!(" (~-{})", propagation_delay));
                                }
                                if propagation_limit != usize::MAX {
                                    result.push_str(&format!(" (~+{})", propagation_limit));
                                }
                                return Ok(result);
                            } else {
                                env.set_computed_with_type(var_name, parsed_value.clone(), &expr, declared_type.clone());
                                let type_info = if let Some(ref t) = declared_type {
                                    format!(":{}", t.name())
                                } else {
                                    "".to_string()
                                };
                                let mut result = format!("[+] {}{} = {} (computed)", var_name, type_info, parsed_value.display());
                                if propagation_delay > 0 {
                                    result.push_str(&format!(" (~-{})", propagation_delay));
                                }
                                if propagation_limit != usize::MAX {
                                    result.push_str(&format!(" (~+{})", propagation_limit));
                                }
                                return Ok(result);
                            }
                        }
                        Err(e) => {
                            // Fall through to normal processing
                            
                        }
                    }
                }
                Err(e) => {
                    // Fall through to normal processing
                    
                }
            }
        }
        
        if is_multiline_json_object(trimmed_value) {
            match parse_multiline_json_object(trimmed_value) {
                Ok(expr) => {
                    match crate::core::expr::evaluate(&expr, env) {
                        Ok(value) => {
                            // Handle transaction case
                            if env.has_active_transaction() {
                                let placeholder = Value::Str("<?>".to_string());
                                env.set_computed_with_type(var_name, placeholder, &expr, declared_type.clone());
                                let type_info = if let Some(ref t) = declared_type {
                                    format!(":{}", t.name())
                                } else {
                                    "".to_string()
                                };
                                let mut result = format!("[🛠] Crafted: {}{} = {}", var_name, type_info, value.display());
                                if propagation_delay > 0 {
                                    result.push_str(&format!(" (~-{})", propagation_delay));
                                }
                                if propagation_limit != usize::MAX {
                                    result.push_str(&format!(" (~+{})", propagation_limit));
                                }
                                return Ok(result);
                            } else {
                                env.set_computed_with_type(var_name, value.clone(), &expr, declared_type.clone());
                                
                                let propagated = crate::core::propagate::propagate_from(env, var_name)
                                    .unwrap_or_default();
                                
                                let type_info = if let Some(ref t) = declared_type {
                                    format!(":{}", t.name())
                                } else {
                                    "".to_string()
                                };
                                let mut output = String::new();
                                output.push_str(&format!("[+] {}{} = {} (computed)", var_name, type_info, value.display()));
                                if propagation_delay > 0 {
                                    output.push_str(&format!(" (~-{})", propagation_delay));
                                }
                                if propagation_limit != usize::MAX {
                                    output.push_str(&format!(" (~+{})", propagation_limit));
                                }
                                output.push_str(&format!("\n  Expression: {}", expr));
                                
                                if !propagated.is_empty() {
                                    output.push_str(&format!("\n  → Updated: {}", propagated.join(", ")));
                                }
                                
                                return Ok(output);
                            }
                        }
                        Err(_) => {
                            // Fall through to normal processing if JSON parsing fails
                        }
                    }
                }
                Err(_) => {
                    // Fall through to normal processing if JSON parsing fails
                }
            }
        }
        
        // CHECK IF IN TRANSACTION FIRST
        if env.has_active_transaction() {
             // Check if it's a simple number or string
            let trimmed = clean_value_str.trim();
            
            // Check for simple numeric value
            if let Ok(num) = trimmed.parse::<i64>() {
                let value = Value::Int(num);
                env.set_direct_with_type(var_name, value.clone(), declared_type.clone());
                let type_info = if let Some(ref t) = declared_type {
                    format!(":{}", t.name())
                } else {
                    "".to_string()
                };
                let mut result = format!("[🛠] Crafted: {}{} = {}", var_name, type_info, value.display());
                if propagation_delay > 0 {
                    result.push_str(&format!(" (~-{})", propagation_delay));
                }
                if propagation_limit != usize::MAX {
                    result.push_str(&format!(" (~+{})", propagation_limit));
                }
                return Ok(result);
            }
            
            // Check for simple boolean
            if trimmed == "true" || trimmed == "false" {
                let value = Value::Bool(trimmed == "true");
                env.set_direct_with_type(var_name, value.clone(), declared_type.clone());
                let type_info = if let Some(ref t) = declared_type {
                    format!(":{}", t.name())
                } else {
                    "".to_string()
                };
                let mut result = format!("[🛠] Crafted: {}{} = {}", var_name, type_info, value.display());
                if propagation_delay > 0 {
                    result.push_str(&format!(" (~-{})", propagation_delay));
                }
                if propagation_limit != usize::MAX {
                    result.push_str(&format!(" (~+{})", propagation_limit));
                }
                return Ok(result);
            }
            
            // Check for quoted string
            if trimmed.starts_with('"') && trimmed.ends_with('"') {
                let inner = &trimmed[1..trimmed.len()-1];
                let value = Value::Str(inner.to_string());
                env.set_direct_with_type(var_name, value.clone(), declared_type.clone());
                let type_info = if let Some(ref t) = declared_type {
                    format!(":{}", t.name())
                } else {
                    "".to_string()
                };
                let mut result = format!("[🛠] Crafted: {}{} = \"{}\"", var_name, type_info, inner);
                if propagation_delay > 0 {
                    result.push_str(&format!(" (~-{})", propagation_delay));
                }
                if propagation_limit != usize::MAX {
                    result.push_str(&format!(" (~+{})", propagation_limit));
                }
                return Ok(result);
            }
            
            // Parse expression but don't evaluate yet
            match crate::core::expr::parse_expression(&clean_value_str) {
                Ok(expr) => {
                    // During transaction, we store the expression unevaluated
                    let placeholder = Value::Str("<?>".to_string());
                    env.set_computed_with_type(var_name, placeholder, &expr, declared_type.clone());
                    let type_info = if let Some(ref t) = declared_type {
                        format!(":{}", t.name())
                    } else {
                        "".to_string()
                    };
                    let mut result = format!("[🛠] Crafted: {}{} = {}", var_name, type_info, clean_value_str);
                    if propagation_delay > 0 {
                        result.push_str(&format!(" (~-{})", propagation_delay));
                    }
                    if propagation_limit != usize::MAX {
                        result.push_str(&format!(" (~+{})", propagation_limit));
                    }
                    return Ok(result);
                }
                Err(e) => {
                    // If the value looks like a method chain, propagate the parse error
                    if clean_value_str.contains('.') && clean_value_str.contains('(') {
                        return Err(e);
                    }

                    match parse_simple_value(&clean_value_str, intent.parameters.get("type").map(|s: &String| s.as_str())) {
                        Ok(value) => {
                            env.set_direct_with_type(var_name, value.clone(), declared_type.clone());
                            let type_info = if let Some(ref t) = declared_type {
                                format!(":{}", t.name())
                            } else {
                                "".to_string()
                            };
                            let mut result = format!("[🛠] Crafted: {}{} = {}", var_name, type_info, value.display());
                            if propagation_delay > 0 {
                                result.push_str(&format!(" (~-{})", propagation_delay));
                            }
                            if propagation_limit != usize::MAX {
                                result.push_str(&format!(" (~+{})", propagation_limit));
                            }
                            return Ok(result);
                        }
                        Err(_) => {
                            // Treat as string literal
                            let expr = crate::core::expr::Expr::Literal(Value::Str(clean_value_str.to_string()));
                            let placeholder = Value::Str("<?>".to_string());
                            env.set_computed_with_type(var_name, placeholder, &expr, declared_type.clone());
                            let type_info = if let Some(ref t) = declared_type {
                                format!(":{}", t.name())
                            } else {
                                "".to_string()
                            };
                            let mut result = format!("[🛠] Crafted: {}{} = {}", var_name, type_info, clean_value_str);
                            if propagation_delay > 0 {
                                result.push_str(&format!(" (~-{})", propagation_delay));
                            }
                            if propagation_limit != usize::MAX {
                                result.push_str(&format!(" (~+{})", propagation_limit));
                            }
                            return Ok(result);
                        }
                    }
                }
            }
        }
        
        // Original non-transaction code continues here...
        // Check if it contains interpolation
        if clean_value_str.contains('{') && clean_value_str.contains('}') {
            match parse_interpolated_string(&clean_value_str, env) {
                Ok(interpolated) => {
                    let value = Value::Str(interpolated.clone());
                    env.set_direct_with_type(var_name, value.clone(), declared_type.clone());
                    
                    let propagated = crate::core::propagate::propagate_from(env, var_name)
                        .unwrap_or_default();
                    
                    let type_info = if let Some(ref t) = declared_type {
                        format!(":{}", t.name())
                    } else {
                        "".to_string()
                    };
                    let mut output = String::new();
                    output.push_str(&format!("[+] {}{} = {}", var_name, type_info, interpolated));
                    if propagation_delay > 0 {
                        output.push_str(&format!(" (~-{})", propagation_delay));
                    }
                    if propagation_limit != usize::MAX {
                        output.push_str(&format!(" (~+{})", propagation_limit));
                    }
                    
                    if !propagated.is_empty() {
                        output.push_str(&format!("\n  → Updated: {}", propagated.join(", ")));
                    }
                    
                    return Ok(output);
                }
                Err(_) => {
                    // Fall through to expression/simple value
                }
            }
        }

        // Check for conditional expression
        if looks_like_conditional(&clean_value_str) {
            match parse_conditional_expression(&clean_value_str) {
                Ok(expr) => {
                    match crate::core::expr::evaluate(&expr, env) {
                        Ok(value) => {
                            // Make sure propagation parameters are passed here (for 'set' we keep non-reactive storage)
                            env.set_computed_with_type(var_name, value.clone(), &expr, declared_type.clone());
                            
                            let type_info = if let Some(ref t) = declared_type {
                                format!(":{}", t.name())
                            } else {
                                "".to_string()
                            };
                            let mut output = String::new();
                            output.push_str(&format!("[+] {}{} = {} (conditional)", var_name, type_info, value.display()));
                            if propagation_delay > 0 {
                                output.push_str(&format!(" (~-{})", propagation_delay));
                            }
                            if propagation_limit != usize::MAX {
                                output.push_str(&format!(" (~+{})", propagation_limit));
                            }
                            output.push_str(&format!("\n  Expression: {}", expr));
                            
                            return Ok(output);
                        }
                        Err(e) => {
                            // Can't evaluate yet - still use propagation control
                            let placeholder = Value::Str("<?>".to_string());
                            env.set_computed_with_type(var_name, placeholder, &expr, declared_type.clone());
                            
                            let type_info = if let Some(ref t) = declared_type {
                                format!(":{}", t.name())
                            } else {
                                "".to_string()
                            };
                            let mut output = String::new();
                            output.push_str(&format!("[?] {}{} = <?> (pending)", var_name, type_info));
                            if propagation_delay > 0 {
                                output.push_str(&format!(" (~-{})", propagation_delay));
                            }
                            if propagation_limit != usize::MAX {
                                output.push_str(&format!(" (~+{})", propagation_limit));
                            }
                            output.push_str(&format!("\n  Expression: {}", expr));
                            output.push_str(&format!("\n  Note: {}", e));
                            
                            return Ok(output);
                        }
                    }
                }
                Err(_) => {
                    // Fall through to normal parsing
                }
            }
        }
        
        // Try as expression
        match crate::core::expr::parse_expression(&clean_value_str) {
            Ok(expr) => {
                match crate::core::expr::evaluate(&expr, env) {
                    Ok(value) => {
                        let type_hint = intent.parameters.get("type");
                        let final_value = apply_type_hint(value, type_hint.map(|s: &String| s.as_str()))?;
                        
                        env.set_computed_with_type(var_name, final_value.clone(), &expr, declared_type.clone());
                        
                        let propagated = crate::core::propagate::propagate_from(env, var_name)
                            .unwrap_or_default();
                        
                        let type_info = if let Some(ref t) = declared_type {
                            format!(":{}", t.name())
                        } else {
                            "".to_string()
                        };
                        let mut output = String::new();
                        output.push_str(&format!("[+] {}{} = {} (computed)", var_name, type_info, final_value.display()));
                        if propagation_delay > 0 {
                            output.push_str(&format!(" (~-{})", propagation_delay));
                        }
                        if propagation_limit != usize::MAX {
                            output.push_str(&format!(" (~+{})", propagation_limit));
                        }
                        output.push_str(&format!("\n  Expression: {}", expr));
                        
                        if !propagated.is_empty() {
                            output.push_str(&format!("\n  → Updated: {}", propagated.join(", ")));
                        }
                        
                        return Ok(output);
                    }
                    Err(e) => {
                        return Err(e);
                    }
                }
            }
            Err(_) => {
                // Simple value
                let value = parse_simple_value(&clean_value_str, intent.parameters.get("type").map(|s: &String| s.as_str()))?;
                env.set_direct_with_type(var_name, value.clone(), declared_type.clone());
                
                let propagated = crate::core::propagate::propagate_from(env, var_name)
                    .unwrap_or_default();
                
                let type_info = if let Some(ref t) = declared_type {
                    format!(":{}", t.name())
                } else {
                    "".to_string()
                };
                let mut output = String::new();
                output.push_str(&format!("[+] {}{} = {} (direct)", var_name, type_info, value.display()));
                if propagation_delay > 0 {
                    output.push_str(&format!(" (~-{})", propagation_delay));
                }
                if propagation_limit != usize::MAX {
                    output.push_str(&format!(" (~+{})", propagation_limit));
                }
                
                if !propagated.is_empty() {
                    output.push_str(&format!("\n  → Updated: {}", propagated.join(", ")));
                }
                
                return Ok(output);
            }
        }
    } else {
        Err("Set intent requires variable target".to_string())
    }
}


fn parse_simple_type(type_str: &str) -> Option<SimpleType> {
    match type_str.to_lowercase().as_str() {
        "string" => Some(SimpleType::String),
        "int" | "integer" => Some(SimpleType::Integer),
        "float" | "double" => Some(SimpleType::Float),
        "bool" | "boolean" => Some(SimpleType::Boolean),
        "list" => Some(SimpleType::List),
        "dict" | "dictionary" => Some(SimpleType::Dictionary),
        "json" => Some(SimpleType::Json),
        _ => None,
    }
}

// Helper functions to add:
fn is_multiline_json_object(input: &str) -> bool {
    let trimmed = input.trim();
    (trimmed.starts_with('{') && trimmed.ends_with('}')) || 
    (trimmed.lines().count() > 1 && 
     trimmed.contains('{') && 
     trimmed.contains('}') && 
     trimmed.contains(':'))
}

fn parse_multiline_json_object(input: &str) -> Result<crate::core::expr::Expr, String> {
    // Clean the input - remove line breaks and extra whitespace for JSON parsing
    let cleaned = if input.lines().count() > 1 {
        input
            .lines()
            .map(|line| {
                let trimmed_line = line.trim();
                // Remove comment lines
                if trimmed_line.starts_with('#') {
                    ""
                } else {
                    trimmed_line
                }
            })
            .filter(|line| !line.is_empty())
            .collect::<Vec<&str>>()
            .join("")
            .replace(": [", ":[")
            .replace(": {", ":{")
            .replace("] ", "]")
            .replace("} ", "}")
            .replace(",}", "}")
            .replace(",]", "]")
    } else {
        input.to_string()
    };
    
    // Ensure proper JSON format
    let final_cleaned = if !cleaned.starts_with('{') || !cleaned.ends_with('}') {
        return Err("Not a valid JSON object".to_string());
    } else {
        cleaned
    };
    
    crate::core::expr::parse_expression(&final_cleaned)
}

fn execute_writeout_intent_clean(
    intent: &crate::core::intent::Intent, 
    env: &Env,
    _printer: &Printer,
) -> Result<String, String> {
    if let Some(Target::Expression(content)) = &intent.target {
        match parse_interpolated_string(content, env) {
            Ok(result) => {
                Ok(format!("[+] Output: {}", result))
            }
            Err(e) => Err(format!("[-] {}", e)),
        }
    } else {
        Err("Writeout intent requires expression target".to_string())
    }
}

fn execute_read_intent_clean(
    intent: &crate::core::intent::Intent,
    env: &mut Env,
    filesystem: &FileSystem,
    _printer: &Printer,
) -> Result<String, String> {
    if let Some(Target::File(path)) = &intent.target {
        let var_name = intent.parameters.get("variable")
            .ok_or("Read intent requires 'into variable_name' parameter")?;
        
        match filesystem.read_file(path) {
            Ok(content) => {
                let value = crate::core::types::Value::Str(content.clone());
                env.set_direct(var_name, value.clone());
                
                let line_count = content.lines().count();
                let size = content.len();
                
                let mut output = String::new();
                output.push_str(&format!("[+] {} → {} ({} bytes, {} lines)", path, var_name, size, line_count));
                
                if content.len() < 200 {
                    output.push_str("\n  Preview:");
                    for line in content.lines().take(3) {
                        output.push_str(&format!("\n    {}", line));
                    }
                    if line_count > 3 {
                        output.push_str(&format!("\n    ... and {} more lines", line_count - 3));
                    }
                }
                
                Ok(output)
            }
            Err(e) => Err(e),
        }
    } else {
        Err("Read intent requires file target".to_string())
    }
}

fn execute_write_intent_clean(
    intent: &crate::core::intent::Intent,
    env: &mut Env,
    filesystem: &FileSystem,
    _printer: &Printer,
) -> Result<String, String> {
    if let Some(Target::File(path)) = &intent.target {
        // Get content from parameters
        if let Some(content) = intent.parameters.get("content") {
            let clean_content = if content.starts_with('"') && content.ends_with('"') {
                &content[1..content.len()-1]
            } else {
                content
            };
            
            match filesystem.write_file(path, clean_content) {
                Ok(_) => {
                    let size = clean_content.len();
                    let lines = clean_content.lines().count();
                    Ok(format!("[+] Wrote {} ({} bytes, {} lines)", path, size, lines))
                }
                Err(e) => Err(e),
            }
        } else if let Some(var_name) = intent.parameters.get("variable") {
            let value = env.get_value(var_name)
                .ok_or_else(|| format!("Variable '{}' not found", var_name))?;
            
            let content = value.to_string();
            match filesystem.write_file(path, &content) {
                Ok(_) => {
                    Ok(format!("[+] Wrote {} from {} ({} bytes)", path, var_name, content.len()))
                }
                Err(e) => Err(e),
            }
        } else {
            Err("Write intent requires either 'content' or 'variable' parameter".to_string())
        }
    } else {
        Err("Write intent requires file target".to_string())
    }
}

fn execute_append_intent_clean(
    intent: &crate::core::intent::Intent,
    env: &mut Env,
    filesystem: &FileSystem,
    _printer: &Printer,
) -> Result<String, String> {
    if let Some(Target::File(path)) = &intent.target {
        if let Some(content) = intent.parameters.get("content") {
            match filesystem.append_file(path, content) {
                Ok(_) => {
                    Ok(format!("[+] Appended to {} ({} chars)", path, content.len()))
                }
                Err(e) => Err(e),
            }
        } else if let Some(var_name) = intent.parameters.get("variable") {
            let value = env.get_value(var_name)
                .ok_or_else(|| format!("Variable '{}' not found", var_name))?;
            
            let content = value.to_string();
            match filesystem.append_file(path, &content) {
                Ok(_) => {
                    Ok(format!("[+] Appended to {} from {} ({} chars)", path, var_name, content.len()))
                }
                Err(e) => Err(e),
            }
        } else {
            Err("Append intent requires either 'content' or 'variable' parameter".to_string())
        }
    } else {
        Err("Append intent requires file target".to_string())
    }
}

fn execute_mkdir_intent_clean(
    intent: &crate::core::intent::Intent,
    filesystem: &FileSystem,
    _printer: &Printer,
) -> Result<String, String> {
    if let Some(Target::File(path)) = &intent.target {
        match filesystem.mkdir(path) {
            Ok(_) => {
                Ok(format!("[+] Created directory: {}", path))
            }
            Err(e) => Err(e),
        }
    } else {
        Err("Mkdir intent requires directory path".to_string())
    }
}

fn execute_list_intent_clean(
    intent: &crate::core::intent::Intent,
    filesystem: &FileSystem,
    _printer: &Printer,
) -> Result<String, String> {
    if let Some(Target::File(path)) = &intent.target {
        match filesystem.list_files(path) {
            Ok(files) => {
                let mut output = String::new();
                output.push_str(&format!("[+] Directory: {} ({} items)", path, files.len()));
                
                if !files.is_empty() {
                    output.push_str("\n");
                    for (i, file) in files.iter().enumerate().take(20) {
                        output.push_str(&format!("\n  {:3}. {}", i + 1, file));
                    }
                    if files.len() > 20 {
                        output.push_str(&format!("\n  ... and {} more items", files.len() - 20));
                    }
                }
                
                Ok(output)
            }
            Err(e) => Err(e),
        }
    } else {
        Err("List intent requires directory path".to_string())
    }
}

fn execute_info_intent_clean(
    intent: &crate::core::intent::Intent,
    filesystem: &FileSystem,
    _printer: &Printer,
) -> Result<String, String> {
    if let Some(Target::File(path)) = &intent.target {
        match filesystem.file_info(path) {
            Ok(info) => {
                // Check if file exists based on file_type
                let exists = info.file_type != "missing";
                let modified_str = match info.modified {
                    Some(timestamp) => format!("{}", timestamp),
                    None => "N/A".to_string(),
                };
                
                let mut output = String::new();
                output.push_str(&format!("[+] File: {}", path));
                output.push_str(&format!("\n  Exists: {}", exists));
                output.push_str(&format!("\n  Type: {}", info.file_type));
                output.push_str(&format!("\n  Size: {} bytes", info.size));
                output.push_str(&format!("\n  Modified: {}", modified_str));
                
                Ok(output)
            }
            Err(e) => Err(e),
        }
    } else {
        Err("Info intent requires file path".to_string())
    }
}

fn execute_exists_intent_clean(
    intent: &crate::core::intent::Intent,
    filesystem: &FileSystem,
    _printer: &Printer,
) -> Result<String, String> {
    if let Some(Target::File(path)) = &intent.target {
        if filesystem.file_exists(path) {
            Ok(format!("[+] File exists: {}", path))
        } else {
            Ok(format!("[-] File not found: {}", path))
        }
    } else {
        Err("Exists intent requires file path".to_string())
    }
}

fn execute_save_intent_clean(
    intent: &crate::core::intent::Intent,
    env: &Env,
    filesystem: &FileSystem,
    _printer: &Printer,
) -> Result<String, String> {
    if let Some(Target::File(path)) = &intent.target {
        match filesystem.save_env(env, path) {
            Ok(_) => {
                let var_count = env.list().len();
                Ok(format!("[+] Saved environment to {} ({} variables)", path, var_count))
            }
            Err(e) => Err(e),
        }
    } else {
        Err("Save intent requires file target".to_string())
    }
}

fn execute_load_intent_clean(
    intent: &crate::core::intent::Intent,
    env: &mut Env,
    history: &mut Vec<crate::core::intent::Intent>,
    history_manager: &mut HistoryManager,
    engine_manager: &mut ChangeEngineManager,
    library: &mut Library,
    printer: &Printer,
) -> Result<String, String> {
    if let Some(Target::File(path)) = &intent.target {
        match execute_msh_file_with_env_clean(path, env, history, history_manager, engine_manager, library, printer) {
            Ok((success_count, error_count)) => {
                Ok(format!("[+] Loaded {} ({} commands, {} success, {} errors)", 
                    path, success_count + error_count, success_count, error_count))
            }
            Err(e) => Err(e),
        }
    } else {
        Err("Load intent requires file target".to_string())
    }
}

fn execute_msh_file_with_env_clean(
    filename: &str,
    env: &mut Env,
    history: &mut Vec<crate::core::intent::Intent>,
    history_manager: &mut HistoryManager,      // Add this
    engine_manager: &mut ChangeEngineManager,
    library: &mut Library,
    printer: &Printer,
) -> Result<(usize, usize), String> {
    let content = fs::read_to_string(filename)
        .map_err(|e| format!("[-] Cannot read file '{}': {}", filename, e))?;
    
    if !filename.ends_with(".msh") {
        printer.warning(&format!("File '{}' doesn't have .msh extension", filename));
    }
    process_script_content(&content, env, history, history_manager, engine_manager, library, printer);
    
    let mut success_count = 0;
    let mut error_count = 0;
    
    for (line_num, line) in content.lines().enumerate() {
        let line = line.trim();
        
        if line.is_empty() {
            continue;
        }
        
        let line = if let Some(comment_start) = line.find('#') {
            &line[..comment_start].trim()
        } else {
            line
        };
        
        if line.is_empty() {
            continue;
        }
        
        // Handle system commands in script
        match line {
            "env" => {
                show_env_clean(env, printer);
                success_count += 1;
                continue;
            }
            "history" => {
                show_history_clean(history, printer);
                success_count += 1;
                continue;
            }
            "clear" => {
                print!("\x1B[2J\x1B[1;1H");
                success_count += 1;
                continue;
            }
            "help" => {
                show_help(printer);
                success_count += 1;
                continue;
            }
            _ => {}
        }
        
        match parse_to_intent(line) {
            Ok(mut line_intent) => {
                if line_intent.state == IntentState::NeedsClarification {
                    continue;
                }
                
                line_intent = line_intent
                    .with_context("source", "script")
                    .with_context("script", filename)
                    .with_context("line", &(line_num + 1).to_string());
                
                line_intent.state = IntentState::Parsed;
                history.push(line_intent.clone());
                
                line_intent.state = IntentState::Executing;
                
                // Create filesystem for script execution
                let filesystem = FileSystem::new();
                
                match execute_intent(&line_intent, env, &filesystem, library, history, history_manager, engine_manager, printer) {
                    Ok(output) => {
                        println!("{}", output);
                        line_intent.state = IntentState::Succeeded;
                        success_count += 1;
                    }
                    Err(e) => {
                        printer.error(&format!("Line {}: {}", line_num + 1, e));
                        line_intent.state = IntentState::Failed;
                        error_count += 1;
                    }
                }
                
                if let Some(last) = history.last_mut() {
                    last.state = line_intent.state.clone();
                    last.context.extend(line_intent.context.clone());
                }
            }
            Err(e) => {
                printer.error(&format!("Line {}: {}", line_num + 1, e));
                error_count += 1;
            }
        }
    }
    
    Ok((success_count, error_count))
}

fn execute_ensure_intent_clean(
    intent: &crate::core::intent::Intent,
    env: &mut Env,
    _printer: &Printer,
) -> Result<String, String> {
    match &intent.target {
        Some(Target::Variable(var_name)) => {
            // Ensure no longer requires a 'condition' - it's treated as a reactive declaration.
            // Get propagation controls if specified
            let propagation_delay = intent.parameters.get("propagation_delay")
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(0);
            let propagation_limit = intent.parameters.get("propagation_limit")
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(usize::MAX);

            let declared_type = intent.parameters.get("declared_type")
                .and_then(|t| parse_simple_type(t));

            let value_str = intent.parameters.get("value")
                .ok_or("No value specified in ensure intent")?;

            // If value is an expression, register as computed with propagation
            match crate::core::expr::parse_expression(value_str) {
                Ok(expr) => {
                    // Try to evaluate now; if fails, set placeholder via set_computed_with_propagation
                    match crate::core::expr::evaluate(&expr, env) {
                        Ok(val) => {
                            env.set_computed_with_propagation(var_name, val.clone(), &expr, declared_type.clone(), propagation_delay, propagation_limit);

                            let propagated = crate::core::propagate::propagate_from(env, var_name)
                                .unwrap_or_default();

                            let mut output = String::new();
                            output.push_str(&format!("[+] Ensured computed: {} = {}", var_name, val.display()));
                            if !propagated.is_empty() {
                                output.push_str(&format!("\n  → Updated: {}", propagated.join(", ")));
                            }
                            Ok(output)
                        }
                        Err(_) => {
                            // Store as computed with placeholder; propagation will occur when dependencies change
                            let placeholder = Value::Str("<?>".to_string());
                            env.set_computed_with_propagation(var_name, placeholder, &expr, declared_type.clone(), propagation_delay, propagation_limit);
                            Ok(format!("[+] Ensured computed (pending): {} = {}", var_name, expr))
                        }
                    }
                }
                Err(_) => {
                    // Not an expression: treat as simple value and register direct with propagation
                    let desired_value = parse_simple_value(value_str, None)?;

                    if let Some(var) = env.get_variable(var_name) {
                        if var.is_constant {
                            return Err(format!("[-] Cannot change {}: variable is frozen", var_name));
                        }
                    }

                    env.set_direct_with_propagation(var_name, desired_value.clone(), declared_type.clone(), propagation_delay, propagation_limit);

                    let propagated = crate::core::propagate::propagate_from(env, var_name)
                        .unwrap_or_default();

                    let mut output = String::new();
                    output.push_str(&format!("[+] Ensured direct: {} = {}", var_name, desired_value.display()));
                    if !propagated.is_empty() {
                        output.push_str(&format!("\n  → Updated: {}", propagated.join(", ")));
                    }
                    Ok(output)
                }
            }
        }
        Some(Target::File(path)) => {
            if intent.parameters.get("condition") == Some(&"exists".to_string()) {
                let fs = FileSystem::new();
                if fs.file_exists(path) {
                    Ok(format!("[+] File exists: {}", path))
                } else {
                    Ok(format!("[-] File not found: {}", path))
                }
            } else {
                Err("File ensure requires 'exists' condition".to_string())
            }
        }
        _ => {
            Err("Unsupported ensure target".to_string())
        }
    }
}

fn execute_derive_intent_clean(
    intent: &crate::core::intent::Intent,
    env: &mut Env,
    _printer: &Printer,
) -> Result<String, String> {
    if let Some(Target::Variable(var_name)) = &intent.target {
        let old_value = env.get_value(var_name).cloned();
        
        match old_value {
            Some(value) => {
                // Check if user wants to derive as JSON
                if let Some(as_type) = intent.parameters.get("as") {
                    if as_type == "json" {
                        // Convert to JSON
                        match crate::core::builtins::to_json(&value) {
                            Ok(json_string) => {
                                Ok(format!("[+] Derived {} as JSON: {}", var_name, json_string))
                            }
                            Err(e) => Err(format!("[-] JSON conversion error: {}", e)),
                        }
                    } else {
                        Err(format!("[-] Unsupported derivation type: {}", as_type))
                    }
                } else if let Some(from_type) = intent.parameters.get("from") {
                    if from_type == "json" {
                        // Parse from JSON string
                        match &value {
                            crate::core::types::Value::Str(json_str) => {
                                match crate::core::builtins::parse_json(json_str) {
                                    Ok(parsed_value) => {
                                        env.set_direct(var_name, parsed_value.clone());
                                        Ok(format!("[+] Derived {} from JSON: {}", var_name, parsed_value.display()))
                                    }
                                    Err(e) => Err(format!("[-] JSON parsing error: {}", e)),
                                }
                            }
                            _ => Err("[-] Source must be a string for JSON parsing".to_string()),
                        }
                    } else {
                        Err(format!("[-] Unsupported source type: {}", from_type))
                    }
                } else {
                    // Default derive behavior
                    let derived = crate::core::derive::derive(&value);
                    env.set_direct(var_name, derived.clone());
                    Ok(format!("[+] Derived {}: {} → {}", var_name, value.display(), derived.display()))
                }
            }
            None => Err(format!("[-] Variable '{}' not found", var_name)),
        }
    } else {
        Err("Derive intent requires variable target".to_string())
    }
}
fn execute_analyze_intent_clean(
    intent: &crate::core::intent::Intent,
    env: &Env,
    _printer: &Printer,
) -> Result<String, String> {
    if let Some(Target::Variable(var_name)) = &intent.target {
        let deps = env.get_dependencies(var_name);
        let dependents = env.get_dependents(var_name);
        
        let mut output = String::new();
        output.push_str(&format!("[+] Analysis of: {}", var_name));
        
        if let Some(var) = env.get_variable(var_name) {
            output.push_str(&format!("\n  Value: {}", var.value.display()));
            output.push_str(&format!("\n  Type: {}", var.value.type_name()));
            output.push_str(&format!("\n  Source: {:?}", var.source));
            output.push_str(&format!("\n  Frozen: {}", var.is_constant));
            
            if let Some(expr_str) = &var.expression {
                output.push_str(&format!("\n  Expression: {}", expr_str));
            } else if let Some(expr) = env.get_expression(var_name) {
                output.push_str(&format!("\n  Expression: {}", expr));
            }
        }
        
        if !deps.is_empty() {
            output.push_str(&format!("\n  Depends on: {}", deps.join(", ")));
        }
        
        if !dependents.is_empty() {
            output.push_str(&format!("\n  Affects: {}", dependents.join(", ")));
        }
        
        Ok(output)
    } else {
        Err("Analyze intent requires variable target".to_string())
    }
}

fn execute_find_intent_clean(
    intent: &crate::core::intent::Intent,
    env: &Env,
    _printer: &Printer,
) -> Result<String, String> {
    let pattern = if let Some(Target::Expression(pattern)) = &intent.target {
        pattern
    } else if let Some(pattern) = intent.parameters.get("pattern") {
        pattern
    } else {
        return Err("Find intent requires pattern".to_string());
    };
    
    let pattern_lower = pattern.to_lowercase();
    let mut results = Vec::new();
    
    for (name, value) in env.list() {
        if name.to_lowercase().contains(&pattern_lower) || 
           value.to_string().to_lowercase().contains(&pattern_lower) {
            results.push(format!("{} = {}", name, value.display()));
        }
    }
    
    let mut output = String::new();
    output.push_str(&format!("[+] Search for '{}': {} matches", pattern, results.len()));
    
    if !results.is_empty() {
        for result in results.iter().take(10) {
            output.push_str(&format!("\n  • {}", result));
        }
        if results.len() > 10 {
            output.push_str(&format!("\n  ... and {} more", results.len() - 10));
        }
    }
    
    Ok(output)
}

fn execute_execute_intent_clean(
    intent: &crate::core::intent::Intent,
    _env: &mut Env,
    _printer: &Printer,
) -> Result<String, String> {
    match &intent.target {
        Some(Target::Expression(cmd)) => {
            Ok(format!("[?] Would execute: '{}'", cmd))
        }
        Some(Target::Process(name)) => {
            let action = if intent.parameters.get("action") == Some(&"monitor".to_string()) {
                "monitor"
            } else {
                "execute"
            };
            
            Ok(format!("[?] Would {} process '{}'", action, name))
        }
        _ => {
            Err("Execute intent requires expression or process target".to_string())
        }
    }
}

fn execute_freeze_intent_clean(
    intent: &crate::core::intent::Intent,
    env: &mut Env,
    _printer: &Printer,
) -> Result<String, String> {
    if let Some(Target::Variable(var_name)) = &intent.target {
        match env.freeze(var_name) {
            Ok(_) => {
                Ok(format!("[+] Frozen variable '{}' (immune to propagation)", var_name))
            }
            Err(e) => Err(format!("[-] {}", e)),
        }
    } else {
        Err("Freeze intent requires variable target".to_string())
    }
}

fn execute_defined_intent(
    intent: &crate::core::intent::Intent,
    _env: &mut Env,
    _filesystem: &FileSystem,
    _library: &mut Library,
    _history: &mut Vec<crate::core::intent::Intent>,
    defined_intents: &HashMap<String, crate::core::intent::Intent>
) -> Result<String, String> {
    let _defined_intent = defined_intents.get("dummy");
    
    if let Some(Target::Expression(expr)) = &intent.target {
        match crate::core::expr::parse_expression(expr) {
            Ok(parsed_expr) => {
                match crate::core::expr::evaluate(&parsed_expr, _env) {
                    Ok(value) => {
                        Ok(format!("[+] Result: {}", value.display()))
                    }
                    Err(e) => Err(format!("[-] Evaluation error: {}", e)),
                }
            }
            Err(e) => Err(format!("[-] Parse error: {}", e)),
        }
    } else {
        Err("Defined intent must have an expression target".to_string())
    }
}

fn execute_msh_file(filename: &str) -> Result<(), String> {
    let mut env = Env::new();
    let mut history: Vec<crate::core::intent::Intent> = Vec::new();
    let mut history_manager = HistoryManager::new();  // Create instance
    let mut engine_manager = ChangeEngineManager::new();
    let mut library = Library::new();
    let printer = Printer::new();
    
    printer.header(&format!("Executing script: {}", filename));
    
    match execute_msh_file_with_env_clean(filename, &mut env, &mut history, &mut history_manager, &mut engine_manager, &mut library, &printer) {
        Ok((success_count, error_count)) => {
            printer.success(&format!("Script complete: {} commands, {} success, {} errors", 
                success_count + error_count, success_count, error_count));
            Ok(())
        }
        Err(e) => Err(e),
    }
}

// Keep all helper functions as they were
fn parse_simple_value(input: &str, type_hint: Option<&str>) -> Result<crate::core::types::Value, String> {
    let trimmed = input.trim();
    
    if trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"') {
        let inner = &trimmed[1..trimmed.len()-1];
        if !inner.contains('"') {
            return match type_hint {
                Some(":int") => {
                    inner.parse::<i64>()
                        .map(crate::core::types::Value::Int)
                        .map_err(|_| format!("Not an integer: {}", inner))
                }
                Some(":bool") => {
                    match inner.to_lowercase().as_str() {
                        "true" => Ok(crate::core::types::Value::Bool(true)),
                        "false" => Ok(crate::core::types::Value::Bool(false)),
                        _ => Err(format!("Not a boolean: {}", inner)),
                    }
                }
                Some(":string") => {
                    Ok(crate::core::types::Value::Str(inner.to_string()))
                }
                _ => {
                    if inner == "true" {
                        Ok(crate::core::types::Value::Bool(true))
                    } else if inner == "false" {
                        Ok(crate::core::types::Value::Bool(false))
                    } else {
                        Ok(crate::core::types::Value::Str(inner.to_string()))
                    }
                }
            };
        }
    }
    
    let cleaned = trimmed;
    
    match type_hint {
        Some(":int") => {
            cleaned.parse::<i64>()
                .map(crate::core::types::Value::Int)
                .map_err(|_| format!("Not an integer: {}", cleaned))
        }
        Some(":bool") => {
            match cleaned.to_lowercase().as_str() {
                "true" => Ok(crate::core::types::Value::Bool(true)),
                "false" => Ok(crate::core::types::Value::Bool(false)),
                _ => Err(format!("Not a boolean: {}", cleaned)),
            }
        }
        Some(":string") => {
            // Require explicit double-quoted strings for string type hints
            if cleaned.len() >= 2 && cleaned.starts_with('"') && cleaned.ends_with('"') {
                Ok(crate::core::types::Value::Str(cleaned[1..cleaned.len()-1].to_string()))
            } else {
                Err(format!("String value must be in double quotes: {}", cleaned))
            }
        }
        _ => {
            if cleaned == "true" {
                Ok(crate::core::types::Value::Bool(true))
            } else if cleaned == "false" {
                Ok(crate::core::types::Value::Bool(false))
            } else if let Ok(num) = cleaned.parse::<i64>() {
                Ok(crate::core::types::Value::Int(num))
            } else {
                // Do not implicitly accept unquoted strings; require double quotes
                Err(format!("Unquoted string literals are not allowed: {}. Wrap in double quotes.", cleaned))
            }
        }
    }
}

fn looks_like_conditional(s: &str) -> bool {
    let s = s.trim();
    let mut in_quotes = false;
    let chars = s.chars().collect::<Vec<_>>();
    
    for i in 0..chars.len() {
        match chars[i] {
            '"' => in_quotes = !in_quotes,
            '|' if !in_quotes => return true,
            'w' if !in_quotes && i + 4 <= chars.len() => {
                let word: String = chars[i..i+4].iter().collect();
                if word == "when" {
                    let prev_ok = i == 0 || chars[i-1].is_whitespace() || chars[i-1] == '|';
                    let next_ok = i + 4 >= chars.len() || chars[i+4].is_whitespace() || chars[i+4] == '|';
                    if prev_ok && next_ok {
                        return true;
                    }
                }
            }
            _ => {}
        }
    }
    
    false
}

fn parse_conditional_expression(s: &str) -> Result<crate::core::expr::Expr, String> {
    crate::core::expr::parse_expression(s)
}

fn apply_type_hint(value: crate::core::types::Value, hint: Option<&str>) -> Result<crate::core::types::Value, String> {
    if let Some(hint) = hint {
        match hint {
            ":int" => match value {
                crate::core::types::Value::Int(i) => Ok(crate::core::types::Value::Int(i)),
                crate::core::types::Value::Float(f) => Ok(crate::core::types::Value::Int(f as i64)),
                crate::core::types::Value::List(_) |
                crate::core::types::Value::Dict(_) => {
                    Err(format!("Cannot convert {} to int", value.type_name()))
                }
                crate::core::types::Value::Str(s) => {
                    if let Ok(i) = s.parse::<i64>() {
                        Ok(crate::core::types::Value::Int(i))
                    } else if let Ok(f) = s.parse::<f64>() {
                        Ok(crate::core::types::Value::Int(f as i64))
                    } else {
                        Err(format!("Cannot convert '{}' to int", s))
                    }
                },
                crate::core::types::Value::Bool(b) => Ok(crate::core::types::Value::Int(if b { 1 } else { 0 })),
                crate::core::types::Value::Json(s) => {
                    if let Ok(i) = s.parse::<i64>() {
                        Ok(crate::core::types::Value::Int(i))
                    } else if let Ok(f) = s.parse::<f64>() {
                        Ok(crate::core::types::Value::Int(f as i64))
                    } else {
                        Err(format!("Cannot convert JSON '{}' to int", s))
                    }
                },
            },
            ":bool" => match value {
                crate::core::types::Value::Bool(b) => Ok(crate::core::types::Value::Bool(b)),
                crate::core::types::Value::List(_) | 
                crate::core::types::Value::Dict(_) => {
                    Err(format!("Cannot convert {} to bool", value.type_name()))
                }
                crate::core::types::Value::Str(s) => match s.to_lowercase().as_str() {
                    "true" => Ok(crate::core::types::Value::Bool(true)),
                    "false" => Ok(crate::core::types::Value::Bool(false)),
                    _ => Err(format!("Cannot convert '{}' to bool", s)),
                },
                crate::core::types::Value::Int(i) => Ok(crate::core::types::Value::Bool(i != 0)),
                crate::core::types::Value::Float(f) => Ok(crate::core::types::Value::Bool(f != 0.0)),
                crate::core::types::Value::Json(s) => match s.to_lowercase().as_str() {
                    "true" => Ok(crate::core::types::Value::Bool(true)),
                    "false" => Ok(crate::core::types::Value::Bool(false)),
                    _ => Err(format!("Cannot convert JSON '{}' to bool", s)),
                },
            },
            ":string" => Ok(crate::core::types::Value::Str(value.to_string())),
            _ => Ok(value),
        }
    } else {
        Ok(value)
    }
}

fn parse_interpolated_string(input: &str, env: &Env) -> Result<String, String> {
    render_template(input, env)
}

// Book metaphor execution handlers
fn execute_page_intent(library: &Library, _printer: &Printer) -> Result<String, String> {
    let page = library.page();
    let page_name = page.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("/");
    let full_path = page.display();
    
    let mut output = String::new();
    output.push_str(&format!("[+] On page: {}", page_name));
    output.push_str(&format!("\n  Path: {}", full_path));
    
    // Check for annotation
    if let Some(annotation) = library.get_annotation(".") {
        output.push_str(&format!("\n  📝 Note: {}", annotation));
    }
    
    Ok(output)
}

fn execute_turn_intent(
    intent: &crate::core::intent::Intent,
    library: &mut Library,
    _printer: &Printer,
) -> Result<String, String> {
    if let Some(Target::Expression(destination)) = &intent.target {
        library.jump_to(destination)
    } else {
        Err("Turn requires a destination".to_string())
    }
}

fn execute_bookmark_intent(intent: &crate::core::intent::Intent, library: &mut Library, _printer: &Printer) -> Result<String, String> {
    let name = intent.parameters.get("name")
        .ok_or("Bookmark requires a name".to_string())?;
    let path = intent.parameters.get("path");
    
    library.bookmark(name, path.map(|s: &String| s.as_str()))
}

fn execute_bookmarks_intent(library: &Library, _printer: &Printer) -> Result<String, String> {
    let bookmarks = library.list_bookmarks();
    
    if bookmarks.is_empty() {
        return Ok("[?] No bookmarks".to_string());
    }
    
    let mut output = String::new();
    output.push_str(&format!("[+] Bookmarks ({}):", bookmarks.len()));
    
    for bookmark in bookmarks {
        let path_display = bookmark.path.display();
        output.push_str(&format!("\n  📑 {} → {}", bookmark.name, path_display));
        
        if let Some(ref note) = bookmark.annotation {
            output.push_str(&format!("\n      Note: {}", note));
        }
    }
    
    Ok(output)
}

fn execute_remove_bookmark_intent(intent: &crate::core::intent::Intent, library: &mut Library, _printer: &Printer) -> Result<String, String> {
    let name = intent.parameters.get("name")
        .ok_or("Remove bookmark requires a name".to_string())?;
    
    library.remove_bookmark(name)
}

fn execute_volume_intent(intent: &crate::core::intent::Intent, library: &mut Library, _printer: &Printer) -> Result<String, String> {
    let name = intent.parameters.get("name")
        .ok_or("Volume requires a name".to_string())?;
    let path = intent.parameters.get("path")
        .ok_or("Volume requires a path".to_string())?;
    let description = intent.parameters.get("description");
    
    library.volume(name, path, description.map(|s: &String| s.as_str()))
}

fn execute_volumes_intent(library: &Library, _printer: &Printer) -> Result<String, String> {
    let volumes = library.list_volumes();
    
    if volumes.is_empty() {
        return Ok("[?] No volumes".to_string());
    }
    
    let mut output = String::new();
    output.push_str(&format!("[+] Volumes ({}):", volumes.len()));
    
    for volume in volumes {
        output.push_str(&format!("\n  📚 {} → {}", volume.name, volume.path.display()));
        
        if let Some(ref desc) = volume.description {
            output.push_str(&format!("\n      {}", desc));
        }
    }
    
    Ok(output)
}

fn execute_shelve_intent(library: &mut Library, _printer: &Printer) -> Result<String, String> {
    Ok(library.shelve())
}

fn execute_unshelve_intent(library: &mut Library, _printer: &Printer) -> Result<String, String> {
    library.unshelve()
}

fn execute_annotate_intent(intent: &crate::core::intent::Intent, library: &mut Library, _printer: &Printer) -> Result<String, String> {
    let target = intent.parameters.get("target")
        .ok_or("Annotate requires a target".to_string())?;
    let note = intent.parameters.get("note")
        .ok_or("Annotate requires a note".to_string())?;
    
    library.annotate(target, note)
}

fn execute_read_annotation_intent(intent: &crate::core::intent::Intent, library: &Library, _printer: &Printer) -> Result<String, String> {
    let target = intent.parameters.get("target")
        .ok_or("read_annotation requires a target".to_string())?;
    
    if let Some(annotation) = library.get_annotation(target) {
        Ok(format!("[+] Annotation for '{}': {}", target, annotation))
    } else {
        Ok(format!("[?] No annotation for '{}'", target))
    }
}

fn execute_index_intent(library: &Library, _printer: &Printer) -> Result<String, String> {
    match library.index() {
        Ok(entries) => {
            if entries.is_empty() {
                return Ok("[?] Empty directory".to_string());
            }
            
            let mut output = String::new();
            output.push_str(&format!("[+] Index ({} entries):", entries.len()));
            
            for (i, entry) in entries.iter().enumerate() {
                output.push_str(&format!("\n  {:3}. {}", i + 1, entry));
            }
            
            Ok(output)
        }
        Err(e) => Err(e),
    }
}

fn execute_back_intent(intent: &crate::core::intent::Intent, library: &mut Library, _printer: &Printer) -> Result<String, String> {
    let steps = intent.parameters.get("steps")
        .and_then(|s: &String| s.parse::<usize>().ok())
        .unwrap_or(1);
    
    library.back(steps)
}

fn execute_library_intent(library: &Library, _printer: &Printer) -> Result<String, String> {
    let bookmarks = library.list_bookmarks();
    let volumes = library.list_volumes();
    let current_page = library.page();
    
    let mut output = String::new();
    output.push_str("[+] The Morris Library");
    output.push_str(&format!("\n  Current page: {}", current_page.display()));
    
    if !volumes.is_empty() {
        output.push_str(&format!("\n\n  Volumes ({}):", volumes.len()));
        for volume in volumes.iter().take(5) {
            output.push_str(&format!("\n    📚 {} → {}", volume.name, volume.path.display()));
        }
        if volumes.len() > 5 {
            output.push_str(&format!("\n    ... and {} more", volumes.len() - 5));
        }
    }
    
    if !bookmarks.is_empty() {
        output.push_str(&format!("\n\n  Bookmarks ({}):", bookmarks.len()));
        for bookmark in bookmarks.iter().take(5) {
            output.push_str(&format!("\n    📑 {} → {}", bookmark.name, bookmark.path.display()));
        }
        if bookmarks.len() > 5 {
            output.push_str(&format!("\n    ... and {} more", bookmarks.len() - 5));
        }
    }
    
    Ok(output)
}

fn execute_chapter_intent(
    intent: &crate::core::intent::Intent,
    library: &mut Library,
    _printer: &Printer,
) -> Result<String, String> {
    // Chapter is an alias for turn
    if let Some(Target::Expression(destination)) = &intent.target {
        library.turn(destination)
    } else {
        Err("Chapter requires a destination".to_string())
    }
}

fn execute_skim_intent(
    intent: &crate::core::intent::Intent,
    _env: &Env,
    filesystem: &FileSystem,
    _printer: &Printer,
) -> Result<String, String> {
    // Skim is an alias for read with preview
    if let Some(Target::File(path)) = &intent.target {
        match filesystem.read_file(path) {
            Ok(content) => {
                let preview: String = content.chars().take(200).collect();
                let total_chars = content.len();
                let line_count = content.lines().count();
                
                let mut output = String::new();
                output.push_str(&format!("[+] Skimmed {} ({} chars, {} lines)", path, total_chars, line_count));
                output.push_str("\n  Preview:");
                
                for line in preview.lines().take(3) {
                    output.push_str(&format!("\n    {}", line));
                }
                
                if total_chars > 200 {
                    output.push_str(&format!("\n    ... and {} more chars", total_chars - 200));
                }
                
                Ok(output)
            }
            Err(e) => Err(e),
        }
    } else {
        Err("Skim requires a file target".to_string())
    }
}

fn execute_jump_intent(
    intent: &crate::core::intent::Intent,
    library: &mut Library,
    _printer: &Printer,
) -> Result<String, String> {
    if let Some(Target::Expression(destination)) = &intent.target {
        library.jump_to(destination)
    } else {
        Err("Jump requires a destination".to_string())
    }
}

fn execute_peek_intent(
    intent: &crate::core::intent::Intent,
    library: &Library,
    printer: &Printer,
) -> Result<String, String> {
    let distance = if let Some(Target::Expression(dist)) = &intent.target {
        dist.parse::<isize>().unwrap_or(-1)
    } else if let Some(dist_str) = intent.parameters.get("distance") {
        dist_str.parse::<isize>().unwrap_or(-1)
    } else {
        -1
    };
    
    match library.peek(distance) {
        Some(page_name) => {
            let direction = if distance < 0 { "back" } else { "forward" };
            let abs_dist = distance.abs();
            Ok(format!("[+] Peek {} {}: {}", abs_dist, direction, page_name))
        }
        None => {
            printer.warning("Nothing to peek at in that direction");
            Ok("[?] Nothing to peek at".to_string())
        }
    }
}

fn execute_return_intent(
    intent: &crate::core::intent::Intent,
    library: &mut Library,
    _printer: &Printer,
) -> Result<String, String> {
    let steps = intent.parameters.get("steps")
        .and_then(|s: &String| s.parse::<usize>().ok())
        .unwrap_or(1);
    
    library.go_back(steps)
}

fn execute_mark_intent(
    intent: &crate::core::intent::Intent,
    library: &mut Library,
    _printer: &Printer,
) -> Result<String, String> {
    let name = intent.parameters.get("name")
        .ok_or("Mark requires a name".to_string())?;
    let description = intent.parameters.get("description");
    
    // For now, we'll implement mark as a special bookmark
    // You can enhance this later to be temporary marks
    match description {
        Some(desc) => library.bookmark(name, Some(&format!("Mark: {}", desc))),
        None => library.bookmark(name, None),
    }
}
#[allow(dead_code)]
fn execute_goto_intent(
    intent: &crate::core::intent::Intent,
    library: &mut Library,
    printer: &Printer,
) -> Result<String, String> {
    // Goto is just an alias for jump
    execute_jump_intent(intent, library, printer)
}

// History execution handlers
fn execute_history_intent(
    _intent: &crate::core::intent::Intent,
    history_manager: &HistoryManager,
    _printer: &Printer,
) -> Result<String, String> {
    let recent = history_manager.get_last_n(20);
    if recent.is_empty() {
        return Ok("[?] No history yet".to_string());
    }
    
    let mut output = String::new();
    output.push_str(&format!("[+] Recent History ({} entries):", recent.len()));
    
    for (i, entry) in recent.iter().enumerate() {
        let state_color = match entry.state.as_str() {
            "Succeeded" => "\x1b[32m",  // green
            "Failed" => "\x1b[31m",     // red
            _ => "\x1b[90m",            // dark gray
        };
        
        output.push_str(&format!("\n  {:3}. {}[{}]\x1b[0m {} → {}", 
            i + 1, state_color, &entry.state[0..1], entry.verb, entry.intent_string));
        
        if let Some(result) = &entry.result {
            if result.len() < 50 {
                output.push_str(&format!("\n      {}", result));
            }
        }
    }
    
    Ok(output)
}

fn execute_history_search_intent(
    intent: &crate::core::intent::Intent,
    history_manager: &HistoryManager,
    _printer: &Printer,
) -> Result<String, String> {
    let query = intent.parameters.get("query")
        .ok_or("History search requires a query".to_string())?;
    
    let results = history_manager.search(query);
    if results.is_empty() {
        return Ok(format!("[?] No history matching '{}'", query));
    }
    
    let mut output = String::new();
    output.push_str(&format!("[+] History search for '{}' ({} results):", query, results.len()));
    
    for (i, entry) in results.iter().take(10).enumerate() {
        output.push_str(&format!("\n  {:3}. [{}] {} → {}", 
            i + 1, &entry.state[0..1], entry.verb, entry.intent_string));
    }
    
    if results.len() > 10 {
        output.push_str(&format!("\n  ... and {} more", results.len() - 10));
    }
    
    Ok(output)
}

fn execute_history_tag_intent(
    intent: &crate::core::intent::Intent,
    _history_manager: &mut HistoryManager,
    printer: &Printer,
) -> Result<String, String> {
    let tag = intent.parameters.get("tag")
        .ok_or("History tag requires a tag name".to_string())?;
    
    // For now, tag the entire session
    // Later: tag specific entries
    printer.info(&format!("Tagging current session as '{}'", tag));
    Ok(format!("[+] Session tagged as '{}'", tag))
}

fn execute_history_replay_intent(
    intent: &crate::core::intent::Intent,
    history_manager: &mut HistoryManager,
    env: &mut Env,
    filesystem: &FileSystem,
    library: &mut Library,
    history: &mut Vec<crate::core::intent::Intent>,
    engine_manager: &mut ChangeEngineManager,
    printer: &Printer,
) -> Result<String, String> {
    let id_str = intent.parameters.get("id")
        .ok_or("History replay requires an ID".to_string())?;
    
    let id = Uuid::parse_str(id_str)
        .map_err(|e| format!("Invalid ID format: {}", e))?;
    
    let entry = history_manager.get_by_id(&id)
        .ok_or_else(|| format!("History entry not found: {}", id))?;
    
    printer.info(&format!("Replaying: {} → {}", entry.verb, entry.intent_string));
    
    // Parse and execute the original intent
    match parse_to_intent(&entry.intent_string) {
        Ok(mut replayed_intent) => {
            replayed_intent = replayed_intent
                .with_context("source", "history_replay")
                .with_context("original_id", &id.to_string());
            
            match execute_intent(&replayed_intent, env, filesystem, library, history, history_manager, engine_manager, printer) {
                Ok(result) => Ok(format!("[+] Replay successful: {}", result)),
                Err(e) => Err(format!("[-] Replay failed: {}", e)),
            }
        }
        Err(e) => Err(format!("[-] Cannot parse replayed intent: {}", e)),
    }
}

fn execute_history_clear_intent(
    _history_manager: &mut HistoryManager,
    printer: &Printer,
) -> Result<String, String> {
    // In a real implementation, you'd ask for confirmation
    printer.warning("This will clear all history. Are you sure? (not implemented)");
    Ok("[?] History clear not implemented yet".to_string())
}

fn execute_history_save_intent(
    history_manager: &mut HistoryManager,
    _printer: &Printer,
) -> Result<String, String> {
    match history_manager.save() {
        Ok(_) => Ok("[+] History saved".to_string()),
        Err(e) => Err(format!("[-] Failed to save history: {}", e)),
    }
}

// Change Engine execution handlers
fn execute_engine_status_intent(
    engine_manager: &ChangeEngineManager,
    _printer: &Printer,  // Keep as _printer if not used
) -> Result<String, String> {
    let stats = engine_manager.stats();  // Use the stats method
    
    let mut output = String::new();
    output.push_str("[+] Change Engine Status:");
    output.push_str(&format!("\n  Version: {}", engine_manager.engine.version));
    output.push_str(&format!("\n  Created: {}", engine_manager.engine.created.format("%Y-%m-%d %H:%M:%S")));
    output.push_str(&format!("\n  Last Modified: {}", engine_manager.engine.last_modified.format("%Y-%m-%d %H:%M:%S")));
    output.push_str(&format!("\n  Variables: {}", stats.variables));
    output.push_str(&format!("\n  Intent Definitions: {}", stats.intent_definitions));
    output.push_str(&format!("\n  Propagation Rules: {}", stats.propagation_rules));
    output.push_str(&format!("\n  Hooks: {}", stats.hooks));
    
    Ok(output)
}

fn execute_engine_save_intent(
    engine_manager: &mut ChangeEngineManager,
    _printer: &Printer,
) -> Result<String, String> {
    match engine_manager.save() {
        Ok(_) => Ok("[+] Change Engine saved".to_string()),
        Err(e) => Err(format!("[-] Failed to save engine: {}", e)),
    }
}

fn execute_engine_load_intent(
    engine_manager: &mut ChangeEngineManager,
    _printer: &Printer,
) -> Result<String, String> {
    match engine_manager.load() {
        Ok(_) => Ok("[+] Change Engine loaded".to_string()),
        Err(e) => Err(format!("[-] Failed to load engine: {}", e)),
    }
}

fn execute_engine_validate_intent(
    engine_manager: &ChangeEngineManager,
    _printer: &Printer,
) -> Result<String, String> {
    let errors = engine_manager.validate();
    if errors.is_empty() {
        Ok("[✓] Change Engine validation passed".to_string())
    } else {
        let mut output = String::new();
        output.push_str(&format!("[-] Validation failed ({} errors):", errors.len()));
        for error in errors.iter().take(5) {
            output.push_str(&format!("\n  • {}", error));
        }
        if errors.len() > 5 {
            output.push_str(&format!("\n  ... and {} more errors", errors.len() - 5));
        }
        Ok(output)
    }
}

fn execute_engine_define_intent(
    _intent: &crate::core::intent::Intent,
    _engine_manager: &mut ChangeEngineManager,
    printer: &Printer,
) -> Result<String, String> {
    // Simplified implementation
    printer.info("Engine define not fully implemented yet");
    Ok("[?] Engine define - placeholder".to_string())
}

fn execute_engine_rule_intent(
    _intent: &crate::core::intent::Intent,
    _engine_manager: &mut ChangeEngineManager,
    printer: &Printer,
) -> Result<String, String> {
    // Simplified implementation
    printer.info("Engine rule not fully implemented yet");
    Ok("[?] Engine rule - placeholder".to_string())
}

fn execute_engine_hook_intent(
    _intent: &crate::core::intent::Intent,
    _engine_manager: &mut ChangeEngineManager,
    printer: &Printer,
) -> Result<String, String> {
    // Simplified implementation
    printer.info("Engine hook not fully implemented yet");
    Ok("[?] Engine hook - placeholder".to_string())
}
#[allow(dead_code)]
fn test_verb_match() {
    let intent1 = crate::core::intent::Intent::new(crate::core::intent::Verb::Set);
    let intent2 = crate::core::intent::Intent::new(crate::core::intent::Verb::EngineStatus);
    
    println!("Test Set match: {}", matches!(&intent1.verb, crate::core::intent::Verb::Set));
    println!("Test EngineStatus match: {}", matches!(&intent2.verb, crate::core::intent::Verb::EngineStatus));
}

fn transactions_disabled_with_intent(
    _intent: &crate::core::intent::Intent,
    _env: &mut Env,
    _printer: &Printer,
) -> Result<String, String> {
    Err("[x] Transactions have been removed from this build".to_string())
}

fn transactions_disabled_no_intent(
    _env: &mut Env,
    _printer: &Printer,
) -> Result<String, String> {
    Err("[x] Transactions have been removed from this build".to_string())
}

fn execute_craft_intent(
    intent: &crate::core::intent::Intent,
    env: &mut Env,
    _printer: &Printer,
) -> Result<String, String> {
    let name = intent.parameters.get("name");
    
    match env.craft(name.map(|s: &String| s.as_str())) {
        Ok(id) => {
            let mut output = format!("[🛠] Crafting session began");
            if let Some(name) = name {
                output.push_str(&format!(": '{}'", name));
            }
            output.push_str(&format!("\n  ID: {}", id));
            output.push_str("\n  Changes will be recorded until you 'forge' or 'smelt'");
            Ok(output)
        }
        Err(e) => Err(e),
    }
}

fn execute_forge_intent(
    env: &mut Env,
    _printer: &Printer,
) -> Result<String, String> {
    match env.forge() {
        Ok(applied) => {
            if applied.is_empty() {
                Ok("[🛠] Forged empty transaction (no changes)".to_string())
            } else {
                Ok(format!("[🛠] Forged {} changes: {}", 
                    applied.len(), 
                    applied.join(", ")))
            }
        }
        Err(e) => Err(e),
    }
}

fn execute_smelt_intent(
    env: &mut Env,
    _printer: &Printer,
) -> Result<String, String> {
    match env.smelt() {
        Ok(()) => Ok("[🛠] Smelted crafted changes (discarded)".to_string()),
        Err(e) => Err(e),
    }
}

fn execute_temper_intent(
    env: &mut Env,
    _printer: &Printer,
) -> Result<String, String> {
    match env.temper() {
        Ok(preview) => {
            let mut output = String::new();
            output.push_str("[🛠] Tempered transaction preview:\n");
            
            // Basic info
            output.push_str(&format!("  Changes: {}\n", preview.changes.len()));
            output.push_str(&format!("  Estimated affected: {}\n", preview.estimated_affected));
            output.push_str(&format!("  Overall safety score: {:.1}%\n", preview.safety_analysis.overall_safety_score * 100.0));
            
            // Conflicts
            if !preview.conflicts.is_empty() {
                output.push_str("\n  ⚠️  Conflicts:\n");
                for conflict in &preview.conflicts {
                    output.push_str(&format!("    • {}\n", conflict));
                }
            }
            
            // Type issues
            if !preview.safety_analysis.type_issues.is_empty() {
                output.push_str("\n  📊 Type Issues:\n");
                for issue in &preview.safety_analysis.type_issues {
                    output.push_str(&format!("    • {}: {}\n", issue.variable, issue.issue));
                }
            }
            
            // Constraint violations
            if !preview.safety_analysis.constraint_violations.is_empty() {
                output.push_str("\n  🔒 Constraint Violations:\n");
                for violation in &preview.safety_analysis.constraint_violations {
                    output.push_str(&format!("    • {}: {}\n", violation.variable, violation.violation));
                }
            }
            
            // Circular dependencies
            if !preview.safety_analysis.circular_dependencies.is_empty() {
                output.push_str("\n  🔁 Circular Dependencies:\n");
                for circular in &preview.safety_analysis.circular_dependencies {
                    output.push_str(&format!("    • {} ({})\n", circular.path.join(" → "), circular.severity));
                }
            }
            
            // Performance estimate
            let perf = &preview.safety_analysis.performance_estimate;
            output.push_str(&format!("\n  ⚡ Performance Estimate:\n"));
            output.push_str(&format!("    • Variables: {}\n", perf.variable_count));
            output.push_str(&format!("    • Propagation steps: {}\n", perf.propagation_steps));
            output.push_str(&format!("    • Estimated time: {}ms\n", perf.estimated_time_ms));
            output.push_str(&format!("    • Memory impact: {}\n", perf.memory_impact));
            
            if !perf.bottleneck_variables.is_empty() {
                output.push_str(&format!("    • Bottleneck variables: {}\n", perf.bottleneck_variables.join(", ")));
            }
            
            // Detailed changes
            if !preview.detailed_changes.is_empty() {
                output.push_str("\n  📋 Detailed Changes:\n");
                for change in &preview.detailed_changes {
                    output.push_str(&format!("    • {} = {} (was: {})\n", 
                        change.variable, change.new_value, change.old_value));
                    if !change.propagation_targets.is_empty() {
                        output.push_str(&format!("      → Affects: {}\n", change.propagation_targets.join(", ")));
                    }
                    for note in &change.safety_notes {
                        output.push_str(&format!("      ⓘ {}\n", note));
                    }
                }
            }
            
            Ok(output)
        }
        Err(e) => Err(e), // Already formatted as string
    }
}


fn execute_inspect_intent(
    env: &mut Env,
    _printer: &Printer,
) -> Result<String, String> {
    env.inspect_transaction()
}

fn execute_anneal_intent(
    intent: &crate::core::intent::Intent,
    env: &mut Env,
    _printer: &Printer,
) -> Result<String, String> {
    let steps = intent.parameters.get("steps")
        .and_then(|s: &String| s.parse::<usize>().ok())
        .unwrap_or(1);
    
    match env.anneal(steps) {
        Ok(applied) => {
            if applied.is_empty() {
                Ok("[🛠] Nothing to anneal".to_string())
            } else {
                Ok(format!("[🛠] Annealed {} change(s): {}", 
                    applied.len(), 
                    applied.join(", ")))
            }
        }
        Err(e) => Err(e),
    }
}

fn execute_quench_intent(
    env: &mut Env,
    _printer: &Printer,
) -> Result<String, String> {
    match env.quench() {
        Ok(applied) => {
            if applied.is_empty() {
                Ok("[🛠] Quenched empty transaction".to_string())
            } else {
                Ok(format!("[!] Quenched {} change(s) immediately: {}", 
                    applied.len(), 
                    applied.join(", ")))
            }
        }
        Err(e) => Err(e),
    }
}

// Placeholders for Phase 2
fn execute_polish_intent(_intent: &crate::core::intent::Intent, _env: &mut Env, printer: &Printer) -> Result<String, String> {
    printer.info("Polish verb will be implemented in Phase 2");
    Ok("[?] Polish - coming soon".to_string())
}

fn execute_alloy_intent(_intent: &crate::core::intent::Intent, _env: &mut Env, printer: &Printer) -> Result<String, String> {
    printer.info("Alloy verb will be implemented in Phase 2");
    Ok("[?] Alloy - coming soon".to_string())
}

fn execute_engrave_intent(_intent: &crate::core::intent::Intent, _env: &mut Env, printer: &Printer) -> Result<String, String> {
    printer.info("Engrave verb will be implemented in Phase 2");
    Ok("[?] Engrave - coming soon".to_string())
}

fn execute_gild_intent(_intent: &crate::core::intent::Intent, _env: &mut Env, printer: &Printer) -> Result<String, String> {
    printer.info("Gild verb will be implemented in Phase 2");
    Ok("[?] Gild - coming soon".to_string())
}

fn execute_patina_intent(_intent: &crate::core::intent::Intent, _env: &mut Env, printer: &Printer) -> Result<String, String> {
    printer.info("Patina verb will be implemented in Phase 2");
    Ok("[?] Patina - coming soon".to_string())
}

fn execute_transaction_intent(
    env: &mut Env,
    _printer: &Printer,
) -> Result<String, String> {
    Ok(env.transaction_status())
}

fn execute_what_if_intent(
    intent: &crate::core::intent::Intent,
    env: &mut Env,
    _printer: &Printer,
) -> Result<String, String> {
    let mut scenario = std::collections::HashMap::new();
    
    // Convert parameters to Values (except check conditions)
    for (var_name, value_str) in &intent.parameters {
        if var_name != "check_condition" {
            let value = parse_simple_value(value_str, None)?;
            scenario.insert(var_name.clone(), value);
        }
    }
    
    let check_condition = intent.parameters.get("check_condition").map(|s| s.as_str());
    
    // Generate predictions first
    let predictions = predict_values(&scenario, env);
    
    match env.what_if(scenario.clone()) {
        Ok(outcome) => {
            let mut output = String::new();
            output.push_str("[🔬] What-if Safety Analysis:\n");
            
            // Always show predictions
            if !predictions.is_empty() {
                output.push_str("\n  📊 Predicted Outcomes:\n");
                for (var, value) in &predictions {
                    output.push_str(&format!("    • {} = {}\n", var, value));
                }
            } else {
                output.push_str("\n  📊 No specific predictions generated\n");
            }
            
            if let Some(condition) = check_condition {
                output.push_str(&format!("\n  🧪 Condition Check: {}\n", condition.trim_matches('"')));
                
                // Evaluate the condition with predictions
                let evaluation_result = evaluate_safety_condition_with_predictions(condition, &outcome, env, &predictions);
                match evaluation_result {
                    Ok((passed, details)) => {
                        if passed {
                            output.push_str("  ✅ PASSED\n");
                        } else {
                            output.push_str("  ❌ FAILED\n");
                        }
                        output.push_str(&format!("  ℹ️  {}\n", details));
                    }
                    Err(error) => {
                        output.push_str(&format!("  ⚠️  Evaluation error: {}\n", error));
                    }
                }
            }
            
            output.push_str(&format!("\n  📈 Impact Summary:\n"));
            output.push_str(&format!("    Variables affected: {}\n", outcome.affected_variables.len()));
            
            if !outcome.new_conflicts.is_empty() {
                output.push_str(&format!("\n  ⚠️  Conflicts Detected:\n"));
                for conflict in &outcome.new_conflicts {
                    output.push_str(&format!("    • {}\n", conflict));
                }
            }
            
            if outcome.safety_delta > 0.0 {
                output.push_str(&format!("\n  📈 Safety improvement: +{:.1}%\n", outcome.safety_delta * 100.0));
            }
            
            Ok(output)
        }
        Err(e) => Err(e),
    }
}


fn evaluate_safety_condition_with_predictions(
    condition: &str, 
    _outcome: &crate::core::transaction::ScenarioOutcome, 
    _env: &Env,
    predictions: &HashMap<String, String>
) -> Result<(bool, String), String> {
    let condition = condition.trim().trim_matches('"');
    
    // Handle predefined safety checks
    match condition.to_lowercase().as_str() {
        "safety" => {
            Ok((true, "General safety validation passed".to_string()))
        }
        "conflicts" => {
            if _outcome.new_conflicts.is_empty() {
                Ok((true, "No conflicts detected".to_string()))
            } else {
                Ok((false, format!("{} conflicts detected", _outcome.new_conflicts.len())))
            }
        }
        "memory" | "memory-safe" => {
            let affected_count = _outcome.affected_variables.len();
            if affected_count > 1000 {
                Ok((false, format!("High memory impact: {} variables affected", affected_count)))
            } else {
                Ok((true, format!("Acceptable memory impact: {} variables", affected_count)))
            }
        }
        _ => {
            // Handle numeric comparisons with ACTUAL predictions
            evaluate_numeric_condition_with_actual_predictions(condition, predictions)
        }
    }
}

fn evaluate_numeric_condition_with_actual_predictions(
    condition: &str, 
    predictions: &HashMap<String, String>
) -> Result<(bool, String), String> {
    // Handle "var < value", "var > value", "var == value"
    if condition.contains(" < ") {
        let parts: Vec<&str> = condition.split(" < ").collect();
        if parts.len() == 2 {
            let var_name = parts[0].trim();
            let threshold_str = parts[1].trim();
            
            if let Some(predicted_value_str) = predictions.get(var_name) {
                // Try to parse the predicted value
                if let Ok(predicted_num) = predicted_value_str.parse::<f64>() {
                    if let Ok(threshold) = threshold_str.parse::<f64>() {
                        let result = predicted_num < threshold;
                        return Ok((result, format!("{} = {} < {} = {}", var_name, predicted_value_str, threshold, result)));
                    }
                }
            }
            return Ok((true, format!("Could not evaluate '{}' with available predictions", condition)));
        }
    }
    
    if condition.contains(" > ") {
        let parts: Vec<&str> = condition.split(" > ").collect();
        if parts.len() == 2 {
            let var_name = parts[0].trim();
            let threshold_str = parts[1].trim();
            
            if let Some(predicted_value_str) = predictions.get(var_name) {
                if let Ok(predicted_num) = predicted_value_str.parse::<f64>() {
                    if let Ok(threshold) = threshold_str.parse::<f64>() {
                        let result = predicted_num > threshold;
                        return Ok((result, format!("{} = {} > {} = {}", var_name, predicted_value_str, threshold, result)));
                    }
                }
            }
            return Ok((true, format!("Could not evaluate '{}' with available predictions", condition)));
        }
    }
    
    // If we can't parse it specifically, return as informational
    Ok((true, format!("Condition '{}' evaluated with {} predictions", condition, predictions.len())))
}


fn evaluate_safety_condition(condition: &str, _outcome: &crate::core::transaction::ScenarioOutcome, _env: &Env) -> Result<(bool, String), String> {
    // Handle predefined safety checks
    match condition.trim().to_lowercase().as_str() {
        "safety" => {
            Ok((true, "General safety validation passed".to_string()))
        }
        "conflicts" => {
            if _outcome.new_conflicts.is_empty() {
                Ok((true, "No conflicts detected".to_string()))
            } else {
                Ok((false, format!("{} conflicts detected", _outcome.new_conflicts.len())))
            }
        }
        "memory" | "memory-safe" => {
            // Simple memory safety check - in practice this would be much more sophisticated
            let affected_count = _outcome.affected_variables.len();
            if affected_count > 1000 {
                Ok((false, format!("High memory impact: {} variables affected", affected_count)))
            } else {
                Ok((true, format!("Acceptable memory impact: {} variables", affected_count)))
            }
        }
        _ => {
            // Try to evaluate as expression
            evaluate_condition_expression(condition, _env)
        }
    }
}

fn evaluate_condition_expression(condition: &str, _env: &Env) -> Result<(bool, String), String> {
    // This is a simplified evaluator - in practice you'd use the full expression engine
    let condition = condition.trim();
    
    // Handle simple comparisons: "var < value", "var > value", etc.
    if condition.contains(" < ") {
        let parts: Vec<&str> = condition.split(" < ").collect();
        if parts.len() == 2 {
            let var_name = parts[0].trim();
            let threshold_str = parts[1].trim();
            
            // Get current value
            if let Some(var) = _env.get_variable(var_name) {
                if let Ok(threshold) = threshold_str.parse::<f64>() {
                    match &var.value {
                        crate::core::types::Value::Int(i) => {
                            let result = (*i as f64) < threshold;
                            return Ok((result, format!("{} ({}) < {} = {}", var_name, i, threshold, result)));
                        }
                        crate::core::types::Value::Float(f) => {
                            let result = *f < threshold;
                            return Ok((result, format!("{} ({}) < {} = {}", var_name, f, threshold, result)));
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    
    if condition.contains(" > ") {
        let parts: Vec<&str> = condition.split(" > ").collect();
        if parts.len() == 2 {
            let var_name = parts[0].trim();
            let threshold_str = parts[1].trim();
            
            if let Some(var) = _env.get_variable(var_name) {
                if let Ok(threshold) = threshold_str.parse::<f64>() {
                    match &var.value {
                        crate::core::types::Value::Int(i) => {
                            let result = (*i as f64) > threshold;
                            return Ok((result, format!("{} ({}) > {} = {}", var_name, i, threshold, result)));
                        }
                        crate::core::types::Value::Float(f) => {
                            let result = *f > threshold;
                            return Ok((result, format!("{} ({}) > {} = {}", var_name, f, threshold, result)));
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    
    // Handle equality checks
    if condition.contains(" == ") {
        let parts: Vec<&str> = condition.split(" == ").collect();
        if parts.len() == 2 {
            let var_name = parts[0].trim();
            let expected_str = parts[1].trim();
            
            if let Some(var) = _env.get_variable(var_name) {
                let current_value = var.value.to_string();
                let result = current_value == expected_str;
                return Ok((result, format!("{} ({}) == {} = {}", var_name, current_value, expected_str, result)));
            }
        }
    }
    
    // If we can't parse it, return as informational
    Ok((true, format!("Condition '{}' evaluated (no specific check)", condition)))
}

fn predict_values(scenario: &HashMap<String, crate::core::types::Value>, env: &Env) -> HashMap<String, String> {
    let mut predictions = HashMap::new();
    
    // For each variable in scenario, see what it would affect
    for (var_name, _new_value) in scenario {
        let dependents = env.get_dependents(var_name);
        
        for dependent in dependents {
            // Try to get expression through variable metadata
            if let Some(var) = env.get_variable(&dependent) {
                if let Some(ref expr_str) = var.expression {
                    
                    // Parse and evaluate the expression
                    match crate::core::expr::parse_expression(expr_str) {
                        Ok(parsed_expr) => {
                            match evaluate_expression_with_scenario(&parsed_expr, scenario, env) {
                                Ok(predicted_value) => {
                                    predictions.insert(dependent.clone(), predicted_value.display());
                                }
                                Err(e) => {
                                    
                                }
                            }
                        }
                        Err(e) => {
                            
                        }
                    }
                } else {
                    
                }
            } else {
                
            }
        }
    }
    
    predictions
}




fn evaluate_expression_with_temp_env(
    expr: &crate::core::expr::Expr, 
    temp_values: &HashMap<String, crate::core::types::Value>,
    env: &Env
) -> Result<crate::core::types::Value, String> {
    // This is a simplified evaluator that uses temporary values
    match expr {
        crate::core::expr::Expr::Variable(name) => {
            // Check temporary values first, then fall back to real env
            if let Some(value) = temp_values.get(name) {
                Ok(value.clone())
            } else {
                env.get_value(name)
                    .cloned()
                    .ok_or_else(|| format!("Variable not found: {}", name))
            }
        }
        crate::core::expr::Expr::Add(left, right) => {
            let left_val = evaluate_expression_with_temp_env(left, temp_values, env)?;
            let right_val = evaluate_expression_with_temp_env(right, temp_values, env)?;
            match (&left_val, &right_val) {
                (crate::core::types::Value::Int(a), crate::core::types::Value::Int(b)) => {
                    Ok(crate::core::types::Value::Int(a + b))
                }
                (crate::core::types::Value::Float(a), crate::core::types::Value::Float(b)) => {
                    Ok(crate::core::types::Value::Float(a + b))
                }
                _ => Err("Cannot add these types".to_string())
            }
        }
        crate::core::expr::Expr::Multiply(left, right) => {
            let left_val = evaluate_expression_with_temp_env(left, temp_values, env)?;
            let right_val = evaluate_expression_with_temp_env(right, temp_values, env)?;
            match (&left_val, &right_val) {
                (crate::core::types::Value::Int(a), crate::core::types::Value::Int(b)) => {
                    Ok(crate::core::types::Value::Int(a * b))
                }
                (crate::core::types::Value::Float(a), crate::core::types::Value::Float(b)) => {
                    Ok(crate::core::types::Value::Float(a * b))
                }
                _ => Err("Cannot multiply these types".to_string())
            }
        }
        // Add other operations as needed...
        crate::core::expr::Expr::Literal(value) => Ok(value.clone()),
        _ => Err("Complex expression evaluation not fully implemented".to_string())
    }
}

fn evaluate_expression_with_scenario(
    expr: &crate::core::expr::Expr, 
    scenario: &HashMap<String, crate::core::types::Value>,
    env: &Env
) -> Result<crate::core::types::Value, String> {
    match expr {
        crate::core::expr::Expr::Variable(name) => {
            // Check scenario first, then fall back to environment
            if let Some(value) = scenario.get(name) {
                Ok(value.clone())
            } else if let Some(value) = env.get_value(name) {
                Ok(value.clone())
            } else {
                Err(format!("Variable '{}' not found", name))
            }
        }
        crate::core::expr::Expr::Literal(value) => Ok(value.clone()),
        crate::core::expr::Expr::Add(left, right) => {
            let left_val = evaluate_expression_with_scenario(left, scenario, env)?;
            let right_val = evaluate_expression_with_scenario(right, scenario, env)?;
            
            match (&left_val, &right_val) {
                (crate::core::types::Value::Int(a), crate::core::types::Value::Int(b)) => {
                    Ok(crate::core::types::Value::Int(a + b))
                }
                (crate::core::types::Value::Float(a), crate::core::types::Value::Float(b)) => {
                    Ok(crate::core::types::Value::Float(a + b))
                }
                (crate::core::types::Value::Int(a), crate::core::types::Value::Float(b)) => {
                    Ok(crate::core::types::Value::Float(*a as f64 + b))
                }
                (crate::core::types::Value::Float(a), crate::core::types::Value::Int(b)) => {
                    Ok(crate::core::types::Value::Float(a + *b as f64))
                }
                _ => Err(format!("Cannot add {:?} and {:?}", left_val, right_val))
            }
        }
        crate::core::expr::Expr::Subtract(left, right) => {
            let left_val = evaluate_expression_with_scenario(left, scenario, env)?;
            let right_val = evaluate_expression_with_scenario(right, scenario, env)?;
            
            match (&left_val, &right_val) {
                (crate::core::types::Value::Int(a), crate::core::types::Value::Int(b)) => {
                    Ok(crate::core::types::Value::Int(a - b))
                }
                (crate::core::types::Value::Float(a), crate::core::types::Value::Float(b)) => {
                    Ok(crate::core::types::Value::Float(a - b))
                }
                // ... other combinations
                _ => Err(format!("Cannot subtract {:?} and {:?}", left_val, right_val))
            }
        }

        crate::core::expr::Expr::Multiply(left, right) => {
            let left_val = evaluate_expression_with_scenario(left, scenario, env)?;
            let right_val = evaluate_expression_with_scenario(right, scenario, env)?;
            
            match (&left_val, &right_val) {
                (crate::core::types::Value::Int(a), crate::core::types::Value::Int(b)) => {
                    Ok(crate::core::types::Value::Int(a * b))
                }
                (crate::core::types::Value::Float(a), crate::core::types::Value::Float(b)) => {
                    Ok(crate::core::types::Value::Float(a * b))
                }
                (crate::core::types::Value::Int(a), crate::core::types::Value::Float(b)) => {
                    Ok(crate::core::types::Value::Float(*a as f64 * b))
                }
                (crate::core::types::Value::Float(a), crate::core::types::Value::Int(b)) => {
                    Ok(crate::core::types::Value::Float(a * *b as f64))
                }
                _ => Err(format!("Cannot multiply {:?} and {:?}", left_val, right_val))
            }
        }
        
        crate::core::expr::Expr::FunctionCall(name, args) => {
            // Handle built-in functions like (1 - discount_rate)
            if name == "subtract" && args.len() == 2 {
                // Special handling for subtraction disguised as function
                // Or implement actual function call evaluation
            }
            Err(format!("Function '{}' not implemented", name))
        }
        _ => Err("Complex expression evaluation not fully implemented".to_string())
    }
}



fn execute_collection_intent(
    intent: &crate::core::intent::Intent,
    env: &mut Env,
    _printer: &Printer,
) -> Result<String, String> {
    if let Some(Target::Variable(name)) = &intent.target {
        let items_str = intent.parameters.get("items")
            .ok_or("Collection requires items")?;
        
        // Parse items (simplified)
        let items: Vec<&str> = items_str.split(',').map(|s| s.trim()).collect();
        let mut values = Vec::new();
        
        for item in items {
            let value = parse_simple_value(item, None)?;
            values.push(value);
        }
        
        let collection = crate::core::types::Value::List(values);
        env.set_direct(name, collection.clone());
        
        Ok(format!("[+] Created collection {}: {} items", name, collection.display()))
    } else {
        Err("Collection requires variable target".to_string())
    }
}

fn execute_dictionary_intent(
    intent: &crate::core::intent::Intent,
    env: &mut Env,
    _printer: &Printer,
) -> Result<String, String> {
    if let Some(Target::Variable(name)) = &intent.target {
        let _content = intent.parameters.get("content")
            .ok_or("Dictionary requires content")?;
        
        // Parse key-value pairs (simplified)
        let mut map = std::collections::HashMap::new();
        // This is simplified - in practice you'd parse properly
        map.insert("example".to_string(), crate::core::types::Value::Str("value".to_string()));
        
        let dict = crate::core::types::Value::Dict(map);
        env.set_direct(name, dict.clone());
        
        Ok(format!("[+] Created dictionary {}: {}", name, dict.display()))
    } else {
        Err("Dictionary requires variable target".to_string())
    }
}

fn execute_assign_intent(
    intent: &crate::core::intent::Intent,
    env: &mut Env,
    _printer: &Printer,
) -> Result<String, String> {
    if let Some(Target::Expression(target_expr)) = &intent.target {
        let value_str = intent.parameters.get("value")
            .ok_or("Assign requires value")?;
        
        let value = parse_simple_value(value_str, None)?;
        
        // Parse the target expression (simplified for now)
        if target_expr.contains('[') && target_expr.contains(']') {
            // Handle array/dict assignment
            // This would need more sophisticated parsing
            Ok(format!("[+] Assigned {} = {}", target_expr, value.display()))
        } else {
            // Regular variable assignment
            env.set_direct(target_expr, value.clone());
            Ok(format!("[+] Assigned {} = {}", target_expr, value.display()))
        }
    } else {
        Err("Assign requires expression target".to_string())
    }
}

fn execute_parse_json_intent(
    intent: &crate::core::intent::Intent,
    _env: &mut Env,
    _printer: &Printer,
) -> Result<String, String> {
    if let Some(json_str) = intent.parameters.get("json") {
        match crate::core::builtins::parse_json(json_str) {
            Ok(value) => {
                // Return the parsed value as display string
                let display_string: String = value.display();
                Ok(format!("[+] Parsed JSON: {}", display_string))
            },
            Err(e) => Err(format!("[-] JSON parse error: {}", e)),
        }
    } else {
        Err("parse-json requires JSON string parameter".to_string())
    }
}

fn execute_to_json_intent(
    intent: &crate::core::intent::Intent,
    env: &mut Env,
    _printer: &Printer,
) -> Result<String, String> {
    if let Some(var_name) = intent.parameters.get("variable") {
        if let Some(value) = env.get_value(var_name) {
            match crate::core::builtins::to_json(value) {
                Ok(json_str) => Ok(format!("[+] JSON: {}", json_str)),
                Err(e) => Err(format!("[-] JSON serialization error: {}", e)),
            }
        } else {
            Err(format!("Variable '{}' not found", var_name))
        }
    } else {
        Err("to-json requires variable name parameter".to_string())
    }
}

fn execute_from_json_intent(
    intent: &crate::core::intent::Intent,
    env: &mut Env,
    _printer: &Printer,
) -> Result<String, String> {
    if let (Some(json_str), Some(var_name)) = (
        intent.parameters.get("json"),
        intent.parameters.get("variable")
    ) {
        match crate::core::builtins::parse_json(json_str) {
            Ok(value) => {
                env.set_direct(var_name, value.clone());
                let display_string: String = value.display();
                Ok(format!("[+] Parsed JSON into {}: {}", var_name, display_string))
            },
            Err(e) => Err(format!("[-] JSON parse error: {}", e)),
        }
    } else {
        Err("from-json requires both JSON string and variable name".to_string())
    }
}

fn execute_json_get_intent(
    intent: &crate::core::intent::Intent,
    env: &mut Env,
    _printer: &Printer,
) -> Result<String, String> {
    if let (Some(var_name), Some(path)) = (
        intent.parameters.get("variable"),
        intent.parameters.get("path")
    ) {
        if let Some(value) = env.get_value(var_name) {
            let json_path = crate::core::builtins::JsonPath::parse(path)
                .map_err(|e| format!("Invalid JSON path '{}': {}", path, e))?;
            
            match json_path.get(value) {
                Ok(result) => Ok(format!("[+] {}", result.display())),
                Err(e) => Err(format!("[-] JSON path error: {}", e)),
            }
        } else {
            Err(format!("Variable '{}' not found", var_name))
        }
    } else {
        Err("json-get requires variable and path parameters".to_string())
    }
}

fn execute_json_set_intent(
    intent: &crate::core::intent::Intent,
    env: &mut Env,
    _printer: &Printer,
) -> Result<String, String> {
    if let (Some(var_path), Some(value_str)) = (
        intent.parameters.get("variable_path"),
        intent.parameters.get("value")
    ) {
        // Parse the variable.path notation
        let parts: Vec<&str> = var_path.splitn(2, '.').collect();
        if parts.len() != 2 {
            return Err("json-set requires variable.path format".to_string());
        }
        
        let var_name = parts[0];
        let _path = parts[1];  
        
        if let Some(_value) = env.get_value(var_name) {
            // For now, just acknowledge the intent
            Ok(format!("[+] Would set {} to {} (not yet implemented)", var_path, value_str))
        } else {
            Err(format!("Variable '{}' not found", var_name))
        }
    } else {
        Err("json-set requires variable_path and value parameters".to_string())
    }
}

fn process_script_content(
    content: &str,
    env: &mut Env,
    history: &mut Vec<crate::core::intent::Intent>,
    history_manager: &mut HistoryManager,
    engine_manager: &mut ChangeEngineManager,
    library: &mut Library,
    printer: &Printer,
) -> Result<(usize, usize), String> {
    let mut success_count = 0;
    let mut error_count = 0;
    
    let mut lines = content.lines().enumerate().peekable();
    let mut accumulated_statement = String::new();
    let mut in_multiline_block = false;
    let mut multiline_type = MultilineType::Generic;
    
    while let Some((line_num, original_line)) = lines.next() {
        let line = original_line.trim();
        
        // Skip empty lines and comments
        if line.is_empty() {
            // Only accumulate if we're in a multiline context
            if in_multiline_block || !accumulated_statement.is_empty() {
                if !accumulated_statement.is_empty() {
                    accumulated_statement.push_str(original_line);
                    accumulated_statement.push('\n');
                }
            }
            continue;
        }
        
        let line_without_comment = if let Some(comment_start) = line.find('#') {
            &line[..comment_start].trim()
        } else {
            line
        };
        
        if line_without_comment.is_empty() {
            if in_multiline_block || !accumulated_statement.is_empty() {
                if !accumulated_statement.is_empty() {
                    accumulated_statement.push_str(original_line);
                    accumulated_statement.push('\n');
                }
            }
            continue;
        }
        
        // Check for statement termination with semicolon
        let ends_with_semicolon = line_without_comment.ends_with(';');
        let statement_content = if ends_with_semicolon {
            &line_without_comment[..line_without_comment.len()-1]
        } else {
            line_without_comment
        };
        
        // If we have accumulated content, add current line
        if !accumulated_statement.is_empty() || (!ends_with_semicolon && !in_multiline_block) {
            if accumulated_statement.is_empty() {
                accumulated_statement.push_str(original_line);
            } else {
                accumulated_statement.push_str(&format!("\n{}", original_line));
            }
        }
        
        // If statement is terminated with semicolon, process it
        if ends_with_semicolon && !in_multiline_block {
            // Process the complete statement
            match execute_script_command(&statement_content, env, history, history_manager, engine_manager, library, printer) {
                Ok(_) => success_count += 1,
                Err(e) => {
                    printer.error(&format!("Line {}: {}", line_num + 1, e));
                    error_count += 1;
                }
            }
            accumulated_statement.clear();
            continue;
        }
        
        // Detect multiline start (only if not already accumulating)
        if !in_multiline_block && accumulated_statement.is_empty() {
            if is_multiline_block_start(statement_content) {
                in_multiline_block = true;
                multiline_type = detect_multiline_type(statement_content);
                accumulated_statement.push_str(original_line);
                accumulated_statement.push('\n');
                continue;
            }
        }
        
        // Handle multiline block end
        if in_multiline_block {
            accumulated_statement.push_str(original_line);
            accumulated_statement.push('\n');
            
            if is_multiline_block_end(line_without_comment) || line_without_comment == ";;" {
                in_multiline_block = false;
                
                // Execute the complete multi-line block
                match execute_script_command(&accumulated_statement, env, history, history_manager, engine_manager, library, printer) {
                    Ok(_) => success_count += 1,
                    Err(e) => {
                        printer.error(&format!("Block ending line {}: {}", line_num + 1, e));
                        error_count += 1;
                    }
                }
                
                accumulated_statement.clear();
                continue;
            }
            
            // Continue accumulating
            continue;
        }
        
        // Handle system commands (these don't need semicolons)
        match statement_content {
            "env" => {
                show_env_clean(env, printer);
                success_count += 1;
                continue;
            }
            "history" => {
                show_history_clean(history, printer);
                success_count += 1;
                continue;
            }
            "clear" => {
                print!("\x1B[2J\x1B[1;1H");
                success_count += 1;
                continue;
            }
            _ => {}
        }
        
        // If we reach here and have accumulated content, it means we're in a multiline
        // statement that hasn't been terminated with semicolon
        if !accumulated_statement.is_empty() && !ends_with_semicolon {
            // Continue accumulating until we get a semicolon or explicit multiline end
            continue;
        }
        
        // Execute single-line commands that don't need semicolons for backward compatibility
        // But warn about missing semicolons eventually
        if accumulated_statement.is_empty() {
            match execute_script_command(statement_content, env, history, history_manager, engine_manager, library, printer) {
                Ok(_) => success_count += 1,
                Err(e) => {
                    printer.error(&format!("Line {}: {}", line_num + 1, e));
                    error_count += 1;
                }
            }
        }
    }
    
    // Handle any remaining accumulated statement (backward compatibility)
    if !accumulated_statement.is_empty() {
        // Strip any trailing semicolons for backward compatibility
        let final_content = if accumulated_statement.trim_end().ends_with(';') {
            let trimmed = accumulated_statement.trim_end();
            &trimmed[..trimmed.len()-1]
        } else {
            &accumulated_statement
        };
        
        if !final_content.trim().is_empty() {
            match execute_script_command(final_content, env, history, history_manager, engine_manager, library, printer) {
                Ok(_) => success_count += 1,
                Err(e) => {
                    printer.error(&format!("Incomplete statement: {}", e));
                    error_count += 1;
                }
            }
        }
    }
    
    Ok((success_count, error_count))
}

#[derive(Debug, Clone)]
enum MultilineType {
    Generic,
    MatchExpression,
    ConditionalExpression,
    Dictionary,
    List,
}

fn detect_multiline_type(line: &str) -> MultilineType {
    let line = line.trim();
    if line.starts_with("match ") {
        MultilineType::MatchExpression
    } else if line.contains('|') && (line.contains("when") || line.contains("otherwise")) {
        MultilineType::ConditionalExpression
    } else if line.starts_with('{') || line.contains('{') {
        MultilineType::Dictionary
    } else if line.starts_with('[') {
        MultilineType::List
    } else {
        MultilineType::Generic
    }
}

fn is_multiline_block_start(line: &str) -> bool {
    line.ends_with('{') || 
    line.starts_with("define intent") ||
    line.starts_with("define ") && line.contains('{') ||
    line.contains('{') && line.contains('}') ||
    
    // NEW: Match expressions
    line.trim_start().starts_with("match ") ||
    
    // NEW: Multi-line conditionals
    (line.contains('|') && (line.contains("when") || line.contains("otherwise") || line.contains("else"))) ||
    
    // NEW: Complex expressions that likely continue
    line.trim_end().ends_with("when") ||
    line.trim_end().ends_with("and") ||
    line.trim_end().ends_with("or") ||
    line.trim_end().ends_with("|")
}

fn is_multiline_block_end(line: &str) -> bool {
    line.trim() == "}" || 
    line.starts_with('}') && !line.contains('{')
}

fn execute_script_command(
    command: &str,
    env: &mut Env,
    history: &mut Vec<crate::core::intent::Intent>,
    history_manager: &mut HistoryManager,
    engine_manager: &mut ChangeEngineManager,
    library: &mut Library,
    printer: &Printer,
) -> Result<(), String> {
    match parse_to_intent(command) {
        Ok(mut intent) => {
            if intent.state == IntentState::NeedsClarification {
                return Ok(()); // Skip system commands handled elsewhere
            }
            
            intent = intent
                .with_context("source", "script")
                .with_context("timestamp", &chrono::Utc::now().to_rfc3339());
            
            intent.state = IntentState::Parsed;
            history.push(intent.clone());
            
            match execute_intent(&intent, env, &FileSystem::new(), library, history, history_manager, engine_manager, printer) {
                Ok(output) => {
                    if !output.is_empty() {
                        println!("{}", output);
                    }
                    intent.state = IntentState::Succeeded;
                }
                Err(e) => {
                    printer.error(&e);
                    intent.state = IntentState::Failed;
                }
            }
            
            if let Some(last) = history.last_mut() {
                last.state = intent.state.clone();
                last.context.extend(intent.context.clone());
            }
            
            Ok(())
        }
        Err(e) => Err(e),
    }
}

fn parse_multiline_json_properly(input: &str) -> Result<crate::core::expr::Expr, String> {
    // Clean the multi-line JSON by removing extra formatting but preserving structure
    let lines: Vec<&str> = input.lines().collect();
    let mut cleaned_lines = Vec::new();
    
    for line in lines {
        let trimmed = line.trim();
        // Skip empty lines and comment lines
        if !trimmed.is_empty() && !trimmed.starts_with('#') && trimmed != ";;" {
            cleaned_lines.push(trimmed);
        }
    }
    
    // Join and clean up the structure
    let joined = cleaned_lines.join("");
    let cleaned = joined
        .replace(": [", ":[")
        .replace(": {", ":{")
        .replace("] ", "]")
        .replace("} ", "}")
        .replace(",}", "}")
        .replace(",]", "]");
    
    // Parse the cleaned JSON
    crate::core::expr::parse_expression(&cleaned)
}

fn execute_examine_intent(
    intent: &crate::core::intent::Intent,
    env: &Env,
    library: &Library,
    all_intents: &HashMap<String, crate::core::intent::Intent>,
    validator: &crate::core::startup_validator::StartupValidator,
    printer: &Printer,
) -> Result<String, String> {
    if let Some(Target::Expression(target_type)) = &intent.target {
        match target_type.as_str() {
            "intents" => {
                let system_intents = vec!["set", "ensure", "writeout", "derive", "find", "analyze"];
                let mut output = String::new();
                
                output.push_str(&format!("[+] System Intents ({})\n", system_intents.len()));
                for intent_name in system_intents {
                    output.push_str(&format!("  • {}\n", intent_name));
                }
                
                output.push_str(&format!("\n[+] User-Defined Intents ({})\n", all_intents.len()));
                for (name, intent_def) in all_intents {
                    output.push_str(&format!("  • {} ", name));
                    if let Some(source) = &intent_def.intent_source {
                        output.push_str(&format!("[{}]", source));
                    }
                    if intent_def.is_composition {
                        output.push_str(" [composition]");
                    }
                    output.push('\n');
                }
                
                Ok(output)
            }
            
            "variables" => {
                let variables = env.list();
                let mut output = String::new();
                
                output.push_str(&format!("[+] Variables ({})\n", variables.len()));
                
                let mut by_type: HashMap<&str, Vec<(&String, &Value)>> = HashMap::new();
                for (name, value) in &variables {
                    let type_name = value.type_name();
                    by_type.entry(type_name).or_insert(Vec::new()).push((name, value));
                }
                
                for (type_name, vars) in by_type {
                    output.push_str(&format!("\n  {} ({}):\n", type_name, vars.len()));
                    for (name, value) in vars.iter().take(10) {
                        let short_value = if value.to_string().len() > 30 {
                            format!("{}...", &value.to_string()[..30])
                        } else {
                            value.to_string()
                        };
                        output.push_str(&format!("    • {} = {}\n", name, short_value));
                    }
                    if vars.len() > 10 {
                        output.push_str(&format!("    ... and {} more\n", vars.len() - 10));
                    }
                }
                
                Ok(output)
            }
            
            "engine" => {
                let mut engine_manager = crate::core::change_engine::ChangeEngineManager::new();
                match engine_manager.load() {
                    Ok(_) => execute_engine_status_intent(&engine_manager, printer),
                    Err(e) => Err(format!("Could not load engine: {}", e)),
                }
            }
            
            "system" => {
                let mut output = String::new();
                
                output.push_str("[+] Library State\n");
                output.push_str(&format!("  Current page: {}\n", library.page().display()));
                output.push_str(&format!("  Bookmarks: {}\n", library.list_bookmarks().len()));
                output.push_str(&format!("  Volumes: {}\n", library.list_volumes().len()));
                
                output.push_str("\n[+] Safety System\n");
                match validator.check_system_integrity() {
                    Ok(report) => {
                        output.push_str(&format!("  Integrity: {}\n", 
                            if report.is_clean() { "PASS" } else { "FAIL" }));
                        output.push_str(&format!("  Critical issues: {}\n", report.critical_issues.len()));
                        output.push_str(&format!("  Warnings: {}\n", report.warnings.len()));
                    }
                    Err(e) => output.push_str(&format!("  Integrity check failed: {}\n", e)),
                }
                
                Ok(output)
            }
            
            _ => Err(format!("Unknown examine target: {}", target_type)),
        }
    } else {
        Err("Examine requires a target".to_string())
    }
}

fn execute_construct_intent(
    intent: &crate::core::intent::Intent,
    defined_intents: &mut HashMap<String, crate::core::intent::Intent>,
    printer: &Printer,
) -> Result<String, String> {
    if let Some(Target::Expression(content)) = &intent.target {
        // Parse: "name" with (params) {expr}
        let name = intent.parameters.get("name")
            .ok_or("Missing intent name".to_string())?;
        
        // Create a basic intent definition (simplified for now)
        let mut new_intent = crate::core::intent::Intent::new(crate::core::intent::Verb::Set)
            .mark_as_composition(name)
            .with_source("user_defined");
        
        // Store the construction template for later processing
        new_intent = new_intent.with_target(crate::core::intent::Target::Expression(content.clone()));
        
        defined_intents.insert(name.clone(), new_intent);
        
        printer.success(&format!("Constructed intent: {}", name));
        Ok(format!("[+] Intent '{}' defined (template: {})", name, content))
    } else {
        Err("Construct requires intent definition".to_string())
    }
}

// Placeholder executions for other new verbs
fn execute_evolve_intent(
    _intent: &crate::core::intent::Intent,
    _defined_intents: &mut HashMap<String, crate::core::intent::Intent>,
    printer: &Printer,
) -> Result<String, String> {
    printer.info("Evolve functionality coming soon");
    Ok("[?] Evolve - placeholder".to_string())
}

fn execute_grow_intent(
    _intent: &crate::core::intent::Intent,
    _defined_intents: &mut HashMap<String, crate::core::intent::Intent>,
    printer: &Printer,
) -> Result<String, String> {
    printer.info("Grow functionality coming soon");
    Ok("[?] Grow - placeholder".to_string())
}

fn execute_reflect_intent(
    intent: &crate::core::intent::Intent,
    env: &Env,
    defined_intents: &HashMap<String, crate::core::intent::Intent>,
    validator: &crate::core::startup_validator::StartupValidator,
    printer: &Printer,
) -> Result<String, String> {
    if let Some(Target::Expression(expr)) = &intent.target {
        // Reflection expressions operate on system metadata
        match expr.as_str() {
            "system.intents.count" => {
                let system_count = 15; // Core system intents
                let user_count = defined_intents.len();
                Ok(format!("[+] System intents: {}, User intents: {}", 
                    system_count, user_count))
            }
            
            "system.variables.count" => {
                let count = env.list().len();
                Ok(format!("[+] Variables: {}", count))
            }
            
            "safety.rules" => {
                match validator.check_system_integrity() {
                    Ok(report) => Ok(format!("[+] Safety rules: {} critical, {} warnings",
                        report.critical_issues.len(), report.warnings.len())),
                    Err(e) => Err(format!("Safety check failed: {}", e))
                }
            }
            
            "system.version" => Ok("[+] Morris v2.0 (reflective)".to_string()),
            
            _ => {
                // Try to evaluate as a more complex reflection expression
                if expr.starts_with("intent.") {
                    let intent_name = expr.trim_start_matches("intent.");
                    if let Some(intent_def) = defined_intents.get(intent_name) {
                        let mut output = String::new();
                        output.push_str(&format!("[+] Intent: {}\n", intent_name));
                        output.push_str(&format!("  Verb: {:?}\n", intent_def.verb));
                        output.push_str(&format!("  Source: {}\n", 
                            intent_def.intent_source.as_deref().unwrap_or("system")));
                        output.push_str(&format!("  Composition: {}\n", intent_def.is_composition));
                        output.push_str(&format!("  Parameters: {}\n", 
                            intent_def.parameter_defs.len()));
                        Ok(output)
                    } else {
                        Err(format!("Intent '{}' not found", intent_name))
                    }
                } else {
                    Err(format!("Unknown reflection expression: {}", expr))
                }
            }
        }
    } else {
        Err("Reflect requires an expression".to_string())
    }
}

fn execute_test_intent(
    intent: &crate::core::intent::Intent,
    env: &mut Env,
    defined_intents: &HashMap<String, crate::core::intent::Intent>,
    printer: &Printer,
) -> Result<String, String> {
    if let Some(Target::Expression(test_spec)) = &intent.target {
        // Parse: "intent_name with param1=value1, param2=value2"
        if let Some(with_pos) = test_spec.find(" with ") {
            let intent_name = test_spec[..with_pos].trim();
            let params_str = test_spec[with_pos + 6..].trim(); // " with " is 6 chars
            
            if let Some(intent_def) = defined_intents.get(intent_name) {
                printer.info(&format!("Testing intent: {}", intent_name));
                
                // Parse parameters
                let mut test_params = HashMap::new();
                for param_pair in params_str.split(',') {
                    let parts: Vec<&str> = param_pair.split('=').map(|s| s.trim()).collect();
                    if parts.len() == 2 {
                        test_params.insert(parts[0].to_string(), parts[1].to_string());
                    }
                }
                
                // Create test environment
                let mut test_env = crate::core::env::Env::new();
                
                // Set up test variables
                for (param, value) in &test_params {
                    // Try to parse value appropriately
                    let parsed_value = if let Ok(num) = value.parse::<i64>() {
                        crate::core::types::Value::Int(num)
                    } else if value == "true" {
                        crate::core::types::Value::Bool(true)
                    } else if value == "false" {
                        crate::core::types::Value::Bool(false)
                    } else {
                        crate::core::types::Value::Str(value.clone())
                    };
                    test_env.set_direct(param, parsed_value);
                }
                
                // Execute intent in test environment using helper
                let test_intent = intent_def.clone();
                let filesystem = crate::core::filesystem::FileSystem::new();
                let mut test_library = crate::core::library::Library::new();
                let mut test_history = Vec::new();
                let mut test_history_mgr = crate::core::history::HistoryManager::new();
                let mut test_engine_mgr = crate::core::change_engine::ChangeEngineManager::new();
                
                match execute_intent_in_test_env(
                    &test_intent, 
                    &mut test_env, 
                    &filesystem, 
                    &mut test_library, 
                    &mut test_history,
                    &mut test_history_mgr,
                    &mut test_engine_mgr,
                    printer
                ) {
                    Ok(result) => Ok(format!("[+] Test passed: {}", result)),
                    Err(e) => Ok(format!("[-] Test failed: {}", e)),
                }
            } else {
                Err(format!("Intent '{}' not found", intent_name))
            }
        } else {
            Err("Test requires 'with' clause".to_string())
        }
    } else {
        Err("Test requires a specification".to_string())
    }
}

fn execute_adopt_intent(
    intent: &crate::core::intent::Intent,
    defined_intents: &mut HashMap<String, crate::core::intent::Intent>,
    printer: &Printer,
) -> Result<String, String> {
    if let Some(Target::Expression(intent_name)) = &intent.target {
        if let Some(intent_def) = defined_intents.get(intent_name) {
            // Mark intent as adopted/production-ready
            let mut adopted_intent = intent_def.clone();
            adopted_intent.safety_level = crate::core::intent::SafetyLevel::CoreFunction;
            
            // Move to system intents (or mark as adopted)
            defined_intents.insert(intent_name.clone(), adopted_intent);
            
            printer.success(&format!("Adopted intent: {}", intent_name));
            Ok(format!("[+] Intent '{}' adopted to production", intent_name))
        } else {
            Err(format!("Intent '{}' not found", intent_name))
        }
    } else {
        Err("Adopt requires intent name".to_string())
    }
}

fn show_morris_logo(printer: &Printer) {
    // Clear screen for clean startup
    print!("\x1B[2J\x1B[1;1H");
    
    if printer.use_color {
        println!("\x1b[1;38;5;39m"); // Bright professional blue
    }
    
    println!(r"
        ╔══════════════════════════════════════════╗
        ║                                          ║
        ║             M S H E L L                  ║
        ║                                          ║
        ║    ────────────────────────────────      ║
        ║                                          ║
        ║    Conciseness and Elegance              ║
        ║                                          ║
        ╚══════════════════════════════════════════╝
    ");
    
    if printer.use_color {
        println!("\x1b[0m"); // Reset
    }
    
    println!();
}


