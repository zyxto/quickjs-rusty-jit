use std::env;
use std::fs;
use std::io::{self, Read};
use quickjs_rusty::Context;

fn main() {
    let args: Vec<String> = env::args().collect();

    let code = if args.len() < 2 {
        // Read from stdin
        let mut buffer = String::new();
        let mut stdin = io::stdin();
        stdin.read_to_string(&mut buffer).expect("Error reading from stdin");
        buffer
    } else {
        // Read from file path
        let path = &args[1];
        fs::read_to_string(path).unwrap_or_else(|err| {
            eprintln!("Error reading file '{}': {}", path, err);
            std::process::exit(1);
        })
    };

    let context = Context::new(None).expect("Failed to create JS Context");

    match context.eval(&code, false) {
        Ok(value) => {
            if !value.is_undefined() {
                match value.js_to_string() {
                    Ok(s) => println!("{}", s),
                    Err(_) => println!("{:?}", value),
                }
            }
        }
        Err(err) => {
            eprintln!("Execution Error: {}", err);
            std::process::exit(1);
        }
    }
}
