#[allow(unused_imports)]
use std::io::{self, Write};
use std::{env, fs};
use std::collections::HashMap;

fn main() -> Result<(), std::env::VarError> {
	// Define the built-in commands for this shell
	static BUILTIN_COMMANDS: [&str; 3] = ["type", "echo", "exit"];

	// Build an index of *external* commands once at start-up
	let val = env::var("PATH").unwrap();
	let paths: Vec<&str> = val
		.split(':')
		.filter(|x| !x.contains("/mnt/c"))
		.filter(|x| !x.contains("/home/admin/.vscode-server"))
		.collect();

		let path_commands: HashMap<String, std::path::PathBuf> = paths
			.into_iter()
			.flat_map(|dir| {
				fs::read_dir(dir)
					.ok()
					.into_iter()
					.flatten()
					.filter_map(Result::ok)
					.filter_map(|e| {
						let p = e.path();
						let name = match p.file_name().and_then(|n| n.to_str()) {
							Some(s) => s.to_owned(),
							None => return None,
						};
						Some((name, p))          // borrow already over; p can move
					})
			})
			.fold(HashMap::new(), |mut acc, (name, path)| {
				acc.entry(name).or_insert(path);
				acc
			});

	println!("{:?}", path_commands.iter().take(10).collect::<Vec<_>>());
	// Wait for user input
    loop {
		// Prompt the user for input
		print!("$ ");
		io::stdout().flush().unwrap();

		// Read a line of input
        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();
		
		let mut parts = input.trim().split_whitespace();
		let Some(cmd) = parts.next() else { // empty input -> prompt again
			continue;
    	};

		// Validate input
		match cmd {
			"type" => {
				let Some(query) = parts.next() else {    // no argument after `type`
					eprintln!("type: missing operand");
					continue;
				};

				if BUILTIN_COMMANDS.contains(&query) {
					println!("{query} is a shell builtin");
				} else if let Some(path) = path_commands.get(query) {
					println!("{query} is {}", path.display());
				} else {
					println!("{query}: not found");
				}
			}

			"echo" => {
				println!("{}", parts.collect::<Vec<&str>>().join(" "));
			},

			"exit" => {
				if parts.next() == Some("0") {std::process::exit(0)} 
				else {
					println!("Did you mean `exit 0`?");
					continue
				}
			},

			other => println!("{other}: command not found"),
		}
    }
}
