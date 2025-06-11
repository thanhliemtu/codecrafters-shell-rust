#[allow(unused_imports)]
use std::io::{self, Write};

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
		if input == "exit 0\n" || input == "exit 0\r\n" {
			std::process::exit(0)
		} 
		else {
			println!("{}: command not found", input.trim());
		}
    }
}
