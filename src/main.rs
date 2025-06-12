#[allow(unused_imports)]
use std::io::{self, Write};
use std::{env, fs};
use std::collections::HashMap;
use std::process::Command;
use std::path::{Path, PathBuf};

#[derive(PartialEq)]
enum TokenizerState {
	InSingleQuote,
	InDoubleQuote,
	BackSlashInDoubleQuote,
	Out,
}

fn tokenize(input: &str) -> Vec<String> {
	let mut tokens = Vec::new();
	let mut current_token = String::new();
	let mut state = TokenizerState::Out;

	for ch in input.chars() {
		match (&state, ch) {
			(TokenizerState::Out, '\"') => {
				state = TokenizerState::InDoubleQuote;
			},
			
			(TokenizerState::Out, '\'') => {
				state = TokenizerState::InSingleQuote;
			},

			(TokenizerState::InSingleQuote, '\'') => {
				state = TokenizerState::Out;
			},

			(TokenizerState::InDoubleQuote, '\"') => {
				state = TokenizerState::Out;
			},

			(TokenizerState::Out, char) => {
				if char.is_whitespace() { // If we encounter whitespace, we finalize the current token
					if !current_token.is_empty() {
						tokens.push(current_token.clone());
						current_token.clear();
					}
				} else {
					current_token.push(char); // Otherwise, we add the character to the current token
				}
			},

			(TokenizerState::InSingleQuote, any) => {
				current_token.push(any); // In single quotes, we just add the character to the current token
			},

			(TokenizerState::InDoubleQuote, any) => {
				if any == '\\' {
					state = TokenizerState::BackSlashInDoubleQuote; // In double quotes, a backslash changes the state
					continue; // Skip adding the backslash to the current token
				}
				current_token.push(any); // In double quotes, we just add the character to the current token
			},

			(TokenizerState::BackSlashInDoubleQuote, any) => {
				if any == '$' || any == '`' || any == '\\' || any == '"' {
					// In double quotes, we escape $, `, \ and " characters
					current_token.push(any);
				}
				else {
					current_token.push('\\');
					current_token.push(any); // In double quotes, we just add the character to the current token
				}
				state = TokenizerState::InDoubleQuote; // Return to double quote state
			}
		}
	};

	// If we have a token left at the end, we push it to the list
	// This handles the case where the last token is not followed by whitespace
	// or a closing quote
	if !current_token.is_empty() {
		tokens.push(current_token);
	}

	tokens
}

fn main() -> Result<(), std::env::VarError> {
	// Define the built-in commands for this shell
	static BUILTIN_COMMANDS: [&str; 4] = ["type", "echo", "exit", "pwd"];

	// Build an index of *external* commands once at start-up
	let val = env::var("PATH").unwrap(); // this panics if PATH is not set, in which case what's the point?
	let paths: Vec<&str> = val
		.split(':')
		.filter(|x| !x.contains("/mnt/c"))
		.filter(|x| !x.contains("/home/admin/.vscode-server"))
		.collect();

		let path_commands: HashMap<String, PathBuf> = paths
			.into_iter()
			.flat_map(|dir| {
				fs::read_dir(dir)
					.ok()
					.into_iter()
					.flatten()
					.filter_map(Result::ok)
					.filter_map(|e| {
						if !e.file_type().map_or(false, |ft| ft.is_file()) {
							// Only consider files, skip directories and other types
							// Also skip if the filetype cannot be determined
							return None;
						}

						let p = e.path();
						let name = match p.file_name().and_then(|n| n.to_str()) {
							Some(s) => s.to_owned(),
							None => return None,
						};
						Some((name, p)) 
					})
			})
			.fold(HashMap::new(), |mut acc, (name, path)| {
				acc.entry(name).or_insert(path);
				acc
			});

	// Wait for user input
    loop {
		// Prompt the user for input
		print!("$ ");
		io::stdout().flush().unwrap();

		// Read a line of input
        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();
		
		let tokens = tokenize(input.trim());
		let mut argv = tokens.iter().map(|x| x.as_str());
		let Some(cmd) = argv.next() else { // empty input -> prompt again
			continue;
    	};

		// Validate input
		match cmd {
			"type" => {
				let Some(query) = argv.next() else {    // no argument after `type`
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
				println!("{}", argv.collect::<Vec<&str>>().join(" "));
			},

			"exit" => {
				if argv.next() == Some("0") {std::process::exit(0)} 
				else {
					println!("Did you mean `exit 0`?");
					continue
				}
			},

			"pwd" => {
				match env::current_dir() {
					Ok(path) => println!("{}", path.display()),
					Err(e) => eprintln!("pwd: {}", e),
				}
			},

			"cd" => {
				// If no argument is given, change to the home directory,
				// or to the root directory if HOME is not set
				let fallback = env::var("HOME").unwrap_or_else(|_| "/".to_owned());
				let query = 
				match argv.next() {
					Some("~") => fallback, 
					Some(q) => q.to_owned(),
					None => fallback
				};
				
				let dir = Path::new(&query).canonicalize();
				match dir {
					Err(_) => eprintln!("cd: {query}: No such file or directory"),
					Ok(path) => env::set_current_dir(path).unwrap()
				}
			},

			// Handle external commands, i.e., commands not in the built-in list
			other => {
				if let Some(_) = path_commands.get(other) {
					let output = Command::new(other)
						.args(argv)
						.output()
						.expect("Failed to execute command");
					
					if !output.stdout.is_empty() {
						print!("{}", String::from_utf8_lossy(&output.stdout));
					}
					if !output.stderr.is_empty() {
						eprintln!("{}", String::from_utf8_lossy(&output.stderr));
					}
				} else {
					println!("{other}: not found");
				}
			} 
		}
    }
}
