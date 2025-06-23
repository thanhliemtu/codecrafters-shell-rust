#[allow(unused_imports)]
use std::io::{self, Write};
use std::{env, fs};
use std::collections::HashMap;
use std::process::{Command, Stdio};
use std::path::{Path, PathBuf};

#[derive(PartialEq)]
enum TokenizerState {
	InSingleQuote,
	InDoubleQuote,
	BackSlashInDoubleQuote,
	Out, // Outside of quotes
	BackSlashOutsideQuote, // Outside of quotes, but a backslash was encountered
}

#[derive(PartialEq)]
enum ParserState {
	Arguments,
	TruncateRedirect, // In this state, the next token is a file path for truncating redirection
	AppendRedirect, // In this state, the next token is a file path for appending redirection
}

fn tokenize_input(input: &str) -> Vec<String> {
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
				} 
				else if char == '\\' {
					state = TokenizerState::BackSlashOutsideQuote; // If we encounter a backslash, we change the state
					continue; // Skip adding the backslash to the current token
				} 
				else {
					current_token.push(char); // Otherwise, we add the character to the current token
				}
			},
			
			(TokenizerState::BackSlashOutsideQuote, any) =>{
				current_token.push(any);
				state = TokenizerState::Out; // Return to the outside state after handling the backslash
			}

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
				if any == '$' || any == '`' || any == '\\' || any == '"' || any == '\n'{
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

#[derive(Debug)]
struct ParsedCommand {
	argv: Vec<String>, // Arguments for the command
	redirect: Option<Redirection> // Path to the file for redirection
}

#[derive(Debug)]
struct Redirection {
	fd: u8, // Fd destination, e.g., 1 for stdout (1<file means file is stored in fd 1)
	mode: RedirectMode, // Whether to append to the file (true) or overwrite it (false)
	path: PathBuf, // Path to the file for redirection
}

#[derive(Debug)]
enum RedirectMode {
    Truncate,   // >
    Append,     // >>
}

// This takes ownership of the tokens and returns a ParsedCommand wrapped in Result
// If the parsing fails, it returns an error message
fn parse_tokens(tokens: Vec<String>) -> Result<ParsedCommand, String> {
	let mut argv: Vec<String> = Vec::new();
    let mut redirect: Option<Redirection> = Option::None;
	let mut state = ParserState::Arguments;

	for token in tokens {
		match (&state, token.as_str()) {
			(ParserState::Arguments, ">" | "1>") => {
				state = ParserState::TruncateRedirect; // Switch to truncate redirect state
			},

			(ParserState::Arguments, ">>" | "1>>") => {
				state = ParserState::AppendRedirect; // Switch to append redirect state
			},
			
			(ParserState::TruncateRedirect, path) => {
				redirect = Some(Redirection{
					fd: 1, // Standard output
					mode: RedirectMode::Truncate,
					path: PathBuf::from(path),
				});
				state = ParserState::Arguments;
			},

			(ParserState::AppendRedirect, path) => {
				redirect = Some(Redirection{
					fd: 1, // Standard output
					mode: RedirectMode::Append,
					path: PathBuf::from(path),
				});
				state = ParserState::Arguments;
			},
			
			(ParserState::Arguments, arg) => {
				// If we are in the arguments state, we just add the argument to the list
				argv.push(arg.to_owned());
			},
		}
	}

	if state != ParserState::Arguments {
		return Err("Incomplete command: missing file path for redirection".to_owned());
	}

	Ok(ParsedCommand { argv, redirect })
} 

fn open_redir(redir: &Redirection) -> std::io::Result<std::fs::File> {
    use std::fs::{File, OpenOptions};
    match redir.mode {
        RedirectMode::Truncate => File::create(&redir.path),
        RedirectMode::Append   => OpenOptions::new()
                                       .create(true)
                                       .append(true)
                                       .open(&redir.path),
    }
}

/// Return a boxed writer that is either the redirection file
/// or Stdout when no redirection was requested.
fn choose_writer(redir: &Option<Redirection>)
        -> std::io::Result<Box<dyn std::io::Write>> {
    if let Some(r) = redir {
        // open_redir() already picks Truncate vs Append
        Ok(Box::new(open_redir(r)?))
    } else {
        Ok(Box::new(std::io::stdout()))
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
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
		
		let tokens = tokenize_input(input.trim());

		if tokens.is_empty() {
			// If no tokens were found, prompt again
			continue;
		}

		let ParsedCommand { argv, redirect } = match parse_tokens(tokens) {
			Ok(p) => p,
			Err(e) => {
				eprintln!("{e}");
				continue;
			}
		};

		let mut argv = argv.iter().map(|x| x.as_str());
		let cmd = argv.next().unwrap(); // can unwrap safely because we already checked that tokens is not empty

		// Validate input
		match cmd {
			"type" => {
				let Some(query) = argv.next() else {    // no argument after `type`
					eprintln!("type: missing operand");
					continue;
				};

				let mut output_stream = match choose_writer(&redirect) {
					Ok(w)  => w,
					Err(e) => { eprintln!("type: {e}"); continue; }
				};

				let msg = if BUILTIN_COMMANDS.contains(&query) {
					format!("{query} is a shell builtin")
				} else if let Some(path) = path_commands.get(query) {
					format!("{query} is {}", path.display())
				} else {
					format!("{query}: not found")
				};

				writeln!(output_stream, "{}", msg).ok()
					.expect("Failed to write to output stream");
			}

			"echo" => {
				let mut output_stream = match choose_writer(&redirect) {
					Ok(w)  => w,
					Err(e) => { eprintln!("echo: {e}"); continue; }
				};

				writeln!(output_stream, "{}", argv.collect::<Vec<&str>>().join(" ")).ok()
					.expect("Failed to write to output stream");
			},

			"exit" => {
				if argv.next() == Some("0") {std::process::exit(0)} 
				else {
					println!("Did you mean `exit 0`?");
					continue
				}
			},

			"pwd" => {
				let mut output_stream = match choose_writer(&redirect) {
					Ok(w)  => w,
					Err(e) => { eprintln!("pwd: {e}"); continue; }
				};

				match env::current_dir() {
					Ok(path) => writeln!(output_stream, "{}", path.display()).ok()
						.expect("Failed to write to output stream"),
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
			_ => {
				if let Some(_) = path_commands.get(cmd) {
					let mut child = Command::new(cmd);

					child.args(argv)                     
						.stdin(Stdio::inherit()) 
						.stderr(Stdio::inherit());
					
					if let Some(r) = &redirect {
						if r.fd == 1 {
							// use the existing helper instead of a manual match
							let file = match open_redir(r) {
								Ok(f)  => f,
								Err(e) => { eprintln!("{cmd}: {e}"); continue; }
							};
							child.stdout(std::process::Stdio::from(file));
						} else {
							eprintln!("{cmd}: unsupported fd {}", r.fd);
							continue;
						}	
					} else {
						child.stdout(Stdio::inherit()); // If no redirection, inherit stdout
					}
					
					if let Err(e) = child.status() {
						eprintln!("{cmd}: {e}");
					}	
				} else {
					println!("{cmd}: not found");
				}
			} 
		}
    }
}
