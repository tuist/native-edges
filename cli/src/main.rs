//! The terminal edge. It renders the core's screens as text and provides the
//! terminal's platform services: where to keep data, and a private session file.

use edges_core::conversations::{ChatMessage, ConversationScreen};
use edges_core::{CoreError, SecureStorage};
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, Instant};

const USAGE: &str = "\
Usage: edges <command>

Commands:
  login                 Sign in with ChatGPT
  logout                Forget the saved session
  routes                List every screen and its deep link pattern
  url <route> [k=v...]  Print the deep link for a screen
  open [url]            Render the screen a deep link points to (default: edges://conversations)
  send [--to <id>] <message>
                        Send a message; without --to, start a new conversation
  chat [id]             Chat interactively

Deep links open the same screen in the apps:
  xcrun simctl openurl booted \"$(edges url conversation id=<id>)\"
  adb shell am start -a android.intent.action.VIEW -d edges://conversations";

fn main() -> ExitCode {
    if let Err(error) = edges_core::launch(data_dir(), Arc::new(PrivateFiles)) {
        eprintln!("error: {error}");
        return ExitCode::FAILURE;
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let result = match args.as_slice() {
        ["login"] => login(),
        ["logout"] => {
            edges_core::sign_out();
            println!("Signed out.");
            Ok(())
        }
        ["routes"] => {
            for route in edges_core::routes() {
                println!("{}\t{}\t{}", route.id, route.pattern, route.description);
            }
            Ok(())
        }
        ["url", route, pairs @ ..] => url(route, pairs),
        ["open"] => open("edges://conversations"),
        ["open", link] => open(link),
        ["send", "--to", id, words @ ..] if !words.is_empty() => {
            send(Some(id), &words.join(" ")).map(|_| ())
        }
        ["send", words @ ..] if !words.is_empty() => send(None, &words.join(" ")).map(|_| ()),
        ["chat"] => chat(None),
        ["chat", id] => chat(Some(id.to_string())),
        ["help" | "--help" | "-h"] => {
            println!("{USAGE}");
            Ok(())
        }
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn url(route: &str, pairs: &[&str]) -> Result<(), CoreError> {
    let mut params = HashMap::new();
    for pair in pairs {
        let (key, value) = pair.split_once('=').ok_or_else(|| CoreError::Failed {
            kind: "invalid".into(),
            message: format!("Expected key=value, got {pair}"),
        })?;
        params.insert(key.into(), value.into());
    }
    println!("{}", edges_core::url(route.into(), params)?);
    Ok(())
}

/// Renders the screen a deep link points to, like the apps do.
fn open(link: &str) -> Result<(), CoreError> {
    let stack = edges_core::open(link.into())?;
    let screen = stack.last().expect("a stack has at least one screen");
    match screen.route.as_str() {
        "conversations" => {
            let list = edges_core::conversations()?;
            println!("\x1b[1m{}\x1b[0m\n", list.title);
            for row in list.rows {
                println!("  {:<40} \x1b[2m{}\x1b[0m", row.title, row.url);
            }
        }
        "conversation" => {
            let conversation = edges_core::conversation(screen.params["id"].clone())?;
            println!("\x1b[1m{}\x1b[0m\n", conversation.title);
            conversation.messages.iter().for_each(print_message);
        }
        _ => return chat(None),
    }
    Ok(())
}

fn print_message(message: &ChatMessage) {
    let text = &message.text;
    match message.role.as_str() {
        "user" => println!("\x1b[1myou\x1b[0m  {text}"),
        "tool" => println!("\x1b[2m  ↳ {text}\x1b[0m"),
        _ => println!("\x1b[35mgpt\x1b[0m  {text}"),
    }
}

/// Sends one message and prints what came back after it.
fn send(id: Option<&str>, text: &str) -> Result<ConversationScreen, CoreError> {
    let before = match id {
        Some(id) => edges_core::conversation(id.into())?.messages.len(),
        None => 0,
    };
    let screen = edges_core::send(id.map(str::to_string), text.into())?;
    screen.messages.iter().skip(before + 1).for_each(print_message);
    if before == 0 {
        println!("\x1b[2m  {}\x1b[0m", screen.url);
    }
    Ok(screen)
}

fn chat(mut id: Option<String>) -> Result<(), CoreError> {
    let stdin = std::io::stdin();
    loop {
        print!("\x1b[1myou\x1b[0m  ");
        std::io::stdout().flush().ok();
        let mut line = String::new();
        if stdin.lock().read_line(&mut line).unwrap_or(0) == 0 {
            return Ok(());
        }
        if !line.trim().is_empty() {
            id = Some(send(id.as_deref(), &line)?.id);
        }
    }
}

fn login() -> Result<(), CoreError> {
    let code = edges_core::sign_in()?;
    println!(
        "To sign in with ChatGPT, open {} and enter this code:\n\n    {}\n",
        code.verification_url, code.user_code
    );
    let deadline = Instant::now() + Duration::from_secs(15 * 60);
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_secs(code.interval));
        match edges_core::poll_sign_in(code.clone()) {
            Ok(true) => {
                println!("Signed in with ChatGPT.");
                return Ok(());
            }
            Ok(false) => {}
            Err(CoreError::Failed { kind, .. }) if kind == "network" => {}
            Err(error) => return Err(error),
        }
    }
    Err(CoreError::Unauthorized {
        message: "The code expired. Run `edges login` again.".into(),
    })
}

fn home(variable: &str, fallback: &str) -> PathBuf {
    std::env::var_os(variable)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(fallback)
        })
        .join("native-edges")
}

/// Where the terminal keeps its database.
fn data_dir() -> String {
    home("XDG_DATA_HOME", ".local/share")
        .to_string_lossy()
        .into_owned()
}

/// The terminal's secure storage: files only the current user can read.
struct PrivateFiles;

impl PrivateFiles {
    fn path(key: &str) -> PathBuf {
        home("XDG_CONFIG_HOME", ".config").join(format!("{key}.json"))
    }
}

impl SecureStorage for PrivateFiles {
    fn read(&self, key: String) -> Option<String> {
        std::fs::read_to_string(Self::path(&key)).ok()
    }

    fn write(&self, key: String, value: String) {
        let path = Self::path(&key);
        let _ = std::fs::create_dir_all(path.parent().expect("the path has a parent"));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        if let Ok(mut file) = options.open(&path) {
            let _ = file.write_all(value.as_bytes());
        }
    }

    fn delete(&self, key: String) {
        let _ = std::fs::remove_file(Self::path(&key));
    }
}
