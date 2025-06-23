#[allow(unused_imports)]
use std::io::{self, Write};
use std::{env, fs};
use std::collections::HashMap;
use std::process::{Command, Stdio};
use std::path::{Path, PathBuf};
use std::error::Error;
use std::fs::{File, OpenOptions};

#[derive(PartialEq)]
enum TokenizerState {
	InSingleQuote,
	InDoubleQuote,
	BackSlashInDoubleQuote,
	Out, // Outside of quotes
	BackSlashOutsideQuote, // Outside of quotes, but a backslash was encountered
}

// #[derive(PartialEq)]
// enum ParserState {
// 	Arguments,
// 	TruncateRedirect, // In this state, the next token is a file path for truncating redirection
// 	AppendRedirect, // In this state, the next token is a file path for appending redirection
// }

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
	redirects: HashMap<u8, Redirection> // Path to the file for redirection
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
// fn parse_tokens(tokens: Vec<String>) -> Result<ParsedCommand, Box<dyn Error>> {
// 	let mut argv: Vec<String> = Vec::new();
//     let mut redirect: Option<Redirection> = Option::None;
// 	let mut state = ParserState::Arguments;

// 	for token in tokens {
// 		match (&state, token.as_str()) {
// 			(ParserState::Arguments, ">" | "1>") => {
// 				state = ParserState::TruncateRedirect; // Switch to truncate redirect state
// 			},

// 			(ParserState::Arguments, ">>" | "1>>") => {
// 				state = ParserState::AppendRedirect; // Switch to append redirect state
// 			},
			
// 			(ParserState::TruncateRedirect, path) => {
// 				redirect = Some(Redirection{
// 					fd: 1, // Standard output
// 					mode: RedirectMode::Truncate,
// 					path: PathBuf::from(path),
// 				});
// 				state = ParserState::Arguments;
// 			},

// 			(ParserState::AppendRedirect, path) => {
// 				redirect = Some(Redirection{
// 					fd: 1, // Standard output
// 					mode: RedirectMode::Append,
// 					path: PathBuf::from(path),
// 				});
// 				state = ParserState::Arguments;
// 			},
			
// 			(ParserState::Arguments, arg) => {
// 				// If we are in the arguments state, we just add the argument to the list
// 				argv.push(arg.to_owned());
// 			},
// 		}
// 	}

// 	if state != ParserState::Arguments {
// 		return Err("Incomplete command: missing file path for redirection".to_owned().into());
// 	}

// 	// Ok(ParsedCommand { argv, redirect });
// } 


fn new_token_parser(tokens: Vec<String>)-> Result<ParsedCommand, Box<dyn Error>> {
	let mut argv: Vec<String> = Vec::new();
	let mut pending: Option<(u8, RedirectMode)> = Option::None;
	let mut redirects: HashMap<u8, Redirection> = HashMap::new();

	for token in tokens {
		match token.as_str() {
			">"  | "1>" => pending = Some((1, RedirectMode::Truncate)),
			">>" | "1>>"=> pending = Some((1, RedirectMode::Append)),
			"2>"       => pending = Some((2, RedirectMode::Truncate)),
			"2>>"      => pending = Some((2, RedirectMode::Append)),
			_ => {
				if let Some((fd, mode)) = pending.take() {
					redirects.insert(fd, Redirection { fd, mode, path: token.into() });
				} else {
					argv.push(token);
				}
			}
		}
	}

	if pending.is_some() {
        return Err("syntax error: redirection without file".into());
    }

    Ok(ParsedCommand { argv, redirects })
}


fn open_redir(redir: &Redirection) -> std::io::Result<fs::File> {
    
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
fn writer_for_fd(redirects: &HashMap<u8, Redirection>, fd: u8) -> std::io::Result<Box<dyn std::io::Write>> {
    if let Some(r) = redirects.get(&fd) { // If there is a redirection for this fd
    	Ok(Box::new(open_redir(r)?))
	} else {
		match fd {
			1 => Ok(Box::new(io::stdout())),
			2 => Ok(Box::new(io::stderr())),
			_ => Err(io::Error::new(
				io::ErrorKind::Other,
				format!("unsupported fd {fd}"),
			)),
		}
	}
}

fn main() -> Result<(), Box<dyn Error>> {
	// Define the built-in commands for this shell
	static BUILTIN_COMMANDS: [&str; 4] = ["type", "echo", "exit", "pwd"];

	// Build an index of *external* commands once at start-up
	let val = env::var("PATH")?; // this panics if PATH is not set, in which case what's the point?
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

		let ParsedCommand { argv, redirects } = match new_token_parser(tokens) {
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
					let mut err_out = writer_for_fd(&redirects, 2)?;
					writeln!(err_out, "type: missing operand")?;
					continue;
				};

				let mut out = writer_for_fd(&redirects, 1)?;

				let msg = if BUILTIN_COMMANDS.contains(&query) {
					format!("{query} is a shell builtin")
				} else if let Some(path) = path_commands.get(query) {
					format!("{query} is {}", path.display())
				} else {
					format!("{query}: not found")
				};

				writeln!(out, "{msg}")?;
			}

			"echo" => {
				let mut out = writer_for_fd(&redirects, 1)?;
				let _ = writer_for_fd(&redirects, 2)?;

    			writeln!(out, "{}", argv.collect::<Vec<&str>>().join(" "))?;
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
					Ok(path) => {
						let mut out = writer_for_fd(&redirects, 1)?;
							writeln!(out, "{}", path.display())?;
					}
					Err(e) => {
						let mut err_out = writer_for_fd(&redirects, 2)?;
						writeln!(err_out, "pwd: {e}")?;
					}
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
					
					for redir in redirects.values() {
						let file = open_redir(redir)?;

						// Match the file descriptor to set the appropriate output stream
						// 1 for stdout, 2 for stderr
						match redir.fd {
							1 => { child.stdout(Stdio::from(file)); }
							2 => { child.stderr(Stdio::from(file)); }
							_ => eprintln!("{}: unsupported file descriptor {}", cmd, redir.fd),
						}
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
