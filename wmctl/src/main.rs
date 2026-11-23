//! wmctl — thin client. Joins args into one command line, sends it to
//! wmd, prints the reply. Exit 0 on "ok*", 1 otherwise.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: wmctl <command> [args...]   e.g. wmctl focus --direction left");
        std::process::exit(2);
    }
    let line = args.join(" ");
    let addr = std::env::var("TILEKIT_ADDR").unwrap_or_else(|_| wm_ipc::DEFAULT_ADDR.into());
    match wm_ipc::send(&addr, &line) {
        Ok(reply) => {
            println!("{reply}");
            std::process::exit(if reply.starts_with("ok") { 0 } else { 1 });
        }
        Err(e) => {
            eprintln!("wmctl: cannot reach wmd at {addr} ({e}). Is the daemon running?");
            std::process::exit(1);
        }
    }
}
