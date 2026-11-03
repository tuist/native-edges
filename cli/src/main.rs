//! The terminal edge. It renders the core's screens as text and provides the
//! terminal's platform services: where to keep data, and a private session file.

use edges_core::chatgpt::{self, Session};
use edges_core::routes::{self, Params};
use edges_core::{conversations, store};
use serde_json::Value;
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::process::ExitCode;
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
    if let Err(error) = store::launch(&data_dir()) {
        eprintln!("error: {}", error.message);
        return ExitCode::FAILURE;
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let result = match args.as_slice() {
        ["login"] => login(),
        ["logout"] => std::fs::remove_file(session_path())
            .or(Ok(()))
            .map(|()| println!("Signed out.")),
        ["routes"] => {
            for route in routes::ROUTES {
                println!(
                    "{}\t{}://{}\t{}",
                    route.id,
                    routes::SCHEME,
                    route.path,
                    route.description
                );
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
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn url(route: &str, pairs: &[&str]) -> Result<(), String> {
    let mut params = Params::new();
    for pair in pairs {
        let (key, value) = pair
            .split_once('=')
            .ok_or_else(|| format!("Expected key=value, got {pair}"))?;
        params.insert(key.into(), value.into());
    }
    println!(
        "{}",
        routes::url(route, &params).map_err(|error| format!("{error:?}"))?
    );
    Ok(())
}

fn open(link: &str) -> Result<(), String> {
    let stack = routes::open(link).map_err(|_| format!("No screen matches {link}"))?;
    let screen = stack.last().expect("a stack has at least one screen");
    let store = store::current().map_err(|e| e.message)?;
    let result = match screen.route {
        "conversations" => conversations::list(store),
        "conversation" => conversations::show(store, &screen.params["id"]),
        "new" => return chat(None),
        other => return Err(format!("The terminal can't render {other}")),
    };
    render(&result.map_err(|e| e.message)?);
    Ok(())
}

fn render(screen: &Value) {
    println!(
        "\x1b[1m{}\x1b[0m\n",
        screen["title"].as_str().unwrap_or_default()
    );
    for row in screen["rows"].as_array().into_iter().flatten() {
        println!(
            "  {:<40} \x1b[2m{}\x1b[0m",
            row["title"].as_str().unwrap_or_default(),
            row["url"].as_str().unwrap_or_default()
        );
    }
    for message in screen["messages"].as_array().into_iter().flatten() {
        print_message(message);
    }
}

fn print_message(message: &Value) {
    let text = message["text"].as_str().unwrap_or_default();
    match message["role"].as_str() {
        Some("user") => println!("\x1b[1myou\x1b[0m  {text}"),
        Some("tool") => println!("\x1b[2m  ↳ {text}\x1b[0m"),
        _ => println!("\x1b[35mgpt\x1b[0m  {text}"),
    }
}

/// Sends one message and prints what came back after it.
fn send(id: Option<&str>, text: &str) -> Result<String, String> {
    let session = load()?.ok_or("Not signed in. Run `edges login` first.")?;
    let store = store::current().map_err(|e| e.message)?;
    let before = match id {
        Some(id) => conversations::show(store, id).map_err(|e| e.message)?["messages"]
            .as_array()
            .map_or(0, Vec::len),
        None => 0,
    };
    let (screen, renewed) =
        conversations::send(&session, store, id, text).map_err(|e| e.message)?;
    if let Some(session) = renewed {
        save(&session)?;
    }
    for message in screen["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .skip(before + 1)
    {
        print_message(message);
    }
    let id = screen["id"].as_str().unwrap_or_default().to_string();
    if before == 0 {
        println!(
            "\x1b[2m  {}\x1b[0m",
            screen["url"].as_str().unwrap_or_default()
        );
    }
    Ok(id)
}

fn chat(mut id: Option<String>) -> Result<(), String> {
    let stdin = std::io::stdin();
    loop {
        print!("\x1b[1myou\x1b[0m  ");
        std::io::stdout().flush().ok();
        let mut line = String::new();
        if stdin
            .lock()
            .read_line(&mut line)
            .map_err(|e| e.to_string())?
            == 0
        {
            return Ok(());
        }
        if !line.trim().is_empty() {
            id = Some(send(id.as_deref(), &line)?);
        }
    }
}

fn login() -> Result<(), String> {
    let pending = chatgpt::sign_in().map_err(|e| e.message)?;
    println!(
        "To sign in with ChatGPT, open {} and enter this code:\n\n    {}\n",
        chatgpt::VERIFICATION_URL,
        pending.user_code
    );
    let deadline = Instant::now() + Duration::from_secs(15 * 60);
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_secs(pending.interval));
        match chatgpt::poll_sign_in(&pending) {
            Ok(Some(session)) => {
                save(&session)?;
                println!("Signed in with ChatGPT.");
                return Ok(());
            }
            Ok(None) => {}
            Err(error) if error.kind == "network" => {}
            Err(error) => return Err(error.message),
        }
    }
    Err("The code expired. Run `edges login` again.".into())
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

fn session_path() -> PathBuf {
    home("XDG_CONFIG_HOME", ".config").join("session.json")
}

fn load() -> Result<Option<Session>, String> {
    match std::fs::read(session_path()) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| "The saved session is corrupt. Run `edges login`.".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

/// The terminal's secure storage: a file only the current user can read.
fn save(session: &Session) -> Result<(), String> {
    let path = session_path();
    std::fs::create_dir_all(path.parent().expect("session path has a parent"))
        .map_err(|e| e.to_string())?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(&path).map_err(|e| e.to_string())?;
    file.write_all(&serde_json::to_vec(session).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}
