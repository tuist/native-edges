//! The terminal edge. It renders the core's screens as text and provides the
//! terminal's platform services: a loopback server for the sign-in redirect,
//! the system browser, and a private session file.

use edges_core::routes::{self, Params};
use edges_core::tuist::{self, Session};
use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "\
Usage: edges <command>

Commands:
  login                 Sign in with Tuist in your browser
  logout                Forget the saved session
  routes                List every screen and its deep link pattern
  url <route> [k=v...]  Print the deep link for a screen
  open [url]            Render the screen a deep link points to (default: edges://projects)
  projects              Shortcut for `open edges://projects`
  project <account/project>
                        Shortcut for `open edges://projects/<account>/<project>`

Deep links open the same screen in the apps:
  xcrun simctl openurl booted \"$(edges url project account=tuist project=tuist)\"
  adb shell am start -a android.intent.action.VIEW -d \"$(edges url projects)\"";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let result = match args.as_slice() {
        ["login"] => login(),
        ["logout"] => logout(),
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
        ["open"] | ["projects"] => open("edges://projects"),
        ["open", link] => open(link),
        ["project", handle] => match handle.split_once('/') {
            Some((account, project)) => open(&format!("edges://projects/{account}/{project}")),
            None => Err("Pass the project as <account>/<project>".into()),
        },
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
    let link = routes::url(route, &params).map_err(|error| format!("{error:?}"))?;
    println!("{link}");
    Ok(())
}

fn open(link: &str) -> Result<(), String> {
    let stack = routes::open(link).map_err(|_| format!("No screen matches {link}"))?;
    let screen = stack.last().expect("a stack has at least one screen");
    let session = load()?.ok_or("Not signed in. Run `edges login` first.")?;
    let result = match screen.route {
        "projects" => tuist::projects(&session),
        "project" => tuist::project(
            &session,
            &screen.params["account"],
            &screen.params["project"],
        ),
        other => return Err(format!("The terminal can't render {other} yet")),
    };
    let (screen, renewed) = result.map_err(|error| error.message)?;
    if let Some(session) = renewed {
        save(&session)?;
    }
    render(&screen);
    Ok(())
}

fn render(screen: &Value) {
    println!(
        "\x1b[1m{}\x1b[0m",
        screen["title"].as_str().unwrap_or_default()
    );
    if let Some(subtitle) = screen["subtitle"].as_str() {
        println!("\x1b[2m{subtitle}\x1b[0m");
    }
    println!();
    for row in screen["rows"].as_array().into_iter().flatten() {
        println!(
            "  {:<40} {:<8} \x1b[2m{}\x1b[0m",
            row["id"].as_str().unwrap_or_default(),
            row["badge"].as_str().unwrap_or_default(),
            row["url"].as_str().unwrap_or_default()
        );
    }
    for field in screen["fields"].as_array().into_iter().flatten() {
        println!(
            "  {:<16} {}",
            field["label"].as_str().unwrap_or_default(),
            field["value"].as_str().unwrap_or_default()
        );
    }
    if let Some(url) = screen["dashboard_url"].as_str() {
        println!("\n  Dashboard: {url}");
    }
}

fn login() -> Result<(), String> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let redirect_uri = format!("http://127.0.0.1:{port}/callback");
    let (authorize_url, pending) = tuist::sign_in(&redirect_uri).map_err(|e| e.message)?;

    println!("Opening your browser to sign in with Tuist…\n{authorize_url}");
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(opener)
        .arg(&authorize_url)
        .status();

    for stream in listener.incoming() {
        let mut stream = stream.map_err(|e| e.to_string())?;
        let mut request_line = String::new();
        BufReader::new(&stream)
            .read_line(&mut request_line)
            .map_err(|e| e.to_string())?;
        let path = request_line.split_whitespace().nth(1).unwrap_or_default();
        if !path.starts_with("/callback") {
            let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
            continue;
        }
        let callback_url = format!("http://127.0.0.1:{port}{path}");
        let result = tuist::complete_sign_in(pending, &callback_url);
        let body = if result.is_ok() {
            "Signed in. You can close this tab and return to the terminal."
        } else {
            "Sign-in failed. Return to the terminal for details."
        };
        let _ = write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let session = result.map_err(|e| e.message)?;
        save(&session)?;
        println!("Signed in with Tuist.");
        return Ok(());
    }
    Err("The browser never returned to the terminal".into())
}

fn logout() -> Result<(), String> {
    match std::fs::remove_file(session_path()?) {
        Ok(()) => println!("Signed out."),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => println!("Not signed in."),
        Err(error) => return Err(error.to_string()),
    }
    Ok(())
}

fn session_path() -> Result<PathBuf, String> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .ok_or("Set HOME or XDG_CONFIG_HOME")?;
    Ok(base.join("native-edges").join("session.json"))
}

fn load() -> Result<Option<Session>, String> {
    match std::fs::read(session_path()?) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| "The saved session is corrupt. Run `edges login`.".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

/// The terminal's secure storage: a file only the current user can read.
fn save(session: &Session) -> Result<(), String> {
    let path = session_path()?;
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
