#[allow(unused_imports)]
use std::io::{self, Write};

static BUILTIN_COMMANDS: [&str; 3] = ["type", "echo", "exit"];

fn main() {
	// Wait for user input
    loop {
		// Prompt the user for input and read a line
		// Flush stdout to ensure the prompt is displayed immediately
		print!("$ ");
		io::stdout().flush().unwrap();
        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();

		// Validate input
		match input.trim() {
			cmd if cmd.starts_with("type") => {
				let test_cmd = cmd.trim_start_matches("type ");
				if BUILTIN_COMMANDS.contains(&test_cmd) {
					println!("{} is a shell builtin", test_cmd);
				} else {
					println!("{}: not found", test_cmd);
				}
			}
			
			cmd if cmd.starts_with("echo ") => {
				let message = cmd.trim_start_matches("echo ");
				println!("{}", message);
			},

			"exit 0" => std::process::exit(0),

			cmd => println!("{}: command not found", cmd),
		}
    }
}
