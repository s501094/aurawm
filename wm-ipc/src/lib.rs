//! Line-oriented request/response over localhost TCP.
//! One connection = one command = one reply. Bound to loopback only.

use std::io::{BufRead, BufReader, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::thread;

pub const DEFAULT_ADDR: &str = "127.0.0.1:9787";

/// Client side: send one command line, get one reply line.
pub fn send(addr: &str, line: &str) -> std::io::Result<String> {
    let mut s = TcpStream::connect(addr)?;
    s.write_all(line.as_bytes())?;
    s.write_all(b"\n")?;
    s.shutdown(Shutdown::Write)?;
    let mut reply = String::new();
    BufReader::new(s).read_line(&mut reply)?;
    Ok(reply.trim_end().to_string())
}

/// Server side: run `handler` for each incoming line, sequentially.
/// Sequential is deliberate: commands mutate one WM state anyway.
pub fn serve(
    addr: &str,
    handler: impl Fn(String) -> String + Send + 'static,
) -> std::io::Result<thread::JoinHandle<()>> {
    let listener = TcpListener::bind(addr)?;
    Ok(thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(mut conn) = conn else { continue };
            let mut line = String::new();
            let mut reader = BufReader::new(match conn.try_clone() {
                Ok(c) => c,
                Err(_) => continue,
            });
            if reader.read_line(&mut line).is_err() {
                continue;
            }
            let reply = handler(line.trim_end().to_string());
            let _ = conn.write_all(reply.as_bytes());
            let _ = conn.write_all(b"\n");
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn round_trip() {
        let addr = "127.0.0.1:19787";
        let _srv = serve(addr, |line| format!("echo:{line}")).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert_eq!(send(addr, "focus left").unwrap(), "echo:focus left");
    }
}
