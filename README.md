# mshell - A Reactive Shell with Dot-Function Chaining

A modern shell inspired by the original [MORRIS shell](https://github.com/Stanislaw3737/MORRIS-shell) but reimagined with a focus on dot-function chaining and selective reactivity.

*"Like the wind blows Willow through the trees, mshell flows through your commands with grace."*

## Features

### 🔄 Selective Reactivity
Unlike the original MORRIS shell which was fundamentally reactive, mshell gives you control:
- `set x = 42` - Creates a static variable
- `ensure x = 42` - Creates a reactive variable that responds to dependencies

### 🔗 Dot-Function Chaining
Everything is chainable with intuitive dot syntax:
```bash
# String manipulation
set name = "Alice".upper().trim()

# JSON parsing and extraction  
set data = parse_json("{users: [{name: 'Bob'}]}").get("users[0].name")

# Error handling with .expect()
set result = risky_operation().expect("Operation failed gracefully")
```

### 🎯 System Integration
Direct access to system interfaces:
- `env` - Full environment dictionary access
- `stdin`, `stdout`, `stderr` - Stream handling
- File operations integrated seamlessly

### 🛡️ Built-in Safety
- No complex transaction system (keeping it simple)
- Intent-based commands prevent accidental damage
- Explicit variable freezing with `freeze` command

## Getting Started

### Installation
```bash
cargo build --release
./target/release/mshell
```

### Basic Usage
```bash
# Interactive mode
mshell

# Run script
mshell script.msh

# Show help
help
```

## Key Differences from MORRIS

| Feature | MORRIS Shell | mshell |
|---------|-------------|---------|
| Reactivity | Everything reactive | Selective (`ensure` vs `set`) |
| Transactions | Full transaction system | None (simplified) |
| Metaprogramming | Supported | Removed |
| Syntax | Traditional shell | Dot-function chaining |
| Philosophy | Reactive by default | Reactive when you want it |

## Example Commands

```bash
# File operations
write "hello.txt" "Hello World"
read "hello.txt" into content
content.upper().writeout()

# Reactive variables
ensure price = base_cost * 1.2  # Updates when base_cost changes
set static_price = base_cost * 1.2  # Static calculation

# System interaction
env.get("HOME").expect("No home directory")
ls(env.HOME).filter("*.rs").len().writeout()

# JSON handling
set config = parse_json(file.read("config.json"))
config.get("database.host").expect("Database host required")
```

## Why "Willow"?

Because like Taylor's ethereal ballad, mshell flows through your system with gentle persistence, adapting to changes while maintaining its core essence. Each command branches naturally from the last, creating a cascading experience of computational poetry.

*"I'm like the water when I creep into every house / Every room, leaf of every tree / Like the willow, I just wanna be everything to everyone."*

But mostly because it sounded cool and we needed a theme song for our shell.

## License

MIT License - Because sharing code should be as effortless as a summer breeze through willow branches.

---

*"In the middle of the night, when I'm pacing in my room, I'm a reactive shell that's always thinking of you..."*