//! Loopback HTTP fixture server. A request can publish the next generation after
//! its response bytes have been read, letting package managers span publication.
use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Component, Path};
use std::process::Command;

fn serve(
    mut stream: TcpStream,
    root: &Path,
    trigger: &Path,
    action: &Path,
    port: u16,
) -> io::Result<()> {
    stream.set_read_timeout(Some(std::time::Duration::from_secs(30)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut request = line.split_whitespace();
    let method = request.next().unwrap_or("");
    let target = request.next().unwrap_or("");
    let path = target.split('?').next().unwrap_or("");
    let mut header = String::new();
    loop {
        header.clear();
        if reader.read_line(&mut header)? == 0 || header == "\r\n" {
            break;
        }
    }
    let relative = Path::new(path.trim_start_matches('/'));
    let safe = relative
        .components()
        .all(|part| matches!(part, Component::Normal(_)));
    let mut body = if safe {
        fs::read(root.join(relative)).ok()
    } else {
        None
    };
    // Mirrorlists contain the production origin. Only this unsigned routing file
    // is rewritten. Signed metadata and package bytes are served unchanged.
    if path == "/rpm/mirrorlist"
        && let Some(bytes) = &mut body
    {
        *bytes = String::from_utf8_lossy(bytes)
            .replace(
                "https://packages.fastmash.io",
                &format!("http://127.0.0.1:{port}"),
            )
            .into_bytes();
    }
    if method == "GET" && fs::read_to_string(trigger).is_ok_and(|value| value.trim() == path) {
        fs::remove_file(trigger)?;
        if !Command::new("bash").arg(action).status()?.success() {
            return Err(io::Error::other("publication action failed"));
        }
        eprintln!("published during GET {path}");
    }
    let status = if body.is_some() {
        "200 OK"
    } else {
        "404 Not Found"
    };
    let body = body.unwrap_or_default();
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    if method != "HEAD" {
        stream.write_all(&body)?;
    }
    eprintln!("{method} {path} {status}");
    Ok(())
}

fn main() -> io::Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 4 {
        return Err(io::Error::other(
            "usage: SERVER ROOT READY_FIFO TRIGGER_FILE ACTION_SCRIPT",
        ));
    }
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    writeln!(fs::File::create(&args[1])?, "{port}")?;
    for connection in listener.incoming() {
        serve(
            connection?,
            Path::new(&args[0]),
            Path::new(&args[2]),
            Path::new(&args[3]),
            port,
        )?;
    }
    Ok(())
}
