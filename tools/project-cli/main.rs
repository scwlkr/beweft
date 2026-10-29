use std::{env, process};

fn main() {
    match env::args().nth(1).as_deref() {
        None | Some("help" | "--help" | "-h") => {
            println!("Usage: ./project <command> [args...]\n\nAdd useful project commands in tools/project-cli/main.rs.");
        }
        Some(command) => {
            eprintln!("Unknown command: {command}. Run ./project --help.");
            process::exit(2);
        }
    }
}
