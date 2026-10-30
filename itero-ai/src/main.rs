//! itero-ai: a local model for itero, behind the OpenAI Chat Completions
//! API on a Unix socket. itero starts it with what it needs in its
//! environment; it has no commands of its own.

/// SIGTERM or SIGINT (the daemon stopping, a person's Ctrl-C) ends serving
/// as when idle, so the model is freed before the process exits.
extern "C" fn on_stop(_: libc::c_int) {
    // only stores to an atomic
    itero_ai::server::stop();
}

fn main() {
    // SAFETY: the handler only stores to an atomic
    unsafe {
        let handler = on_stop as extern "C" fn(libc::c_int) as libc::sighandler_t;
        libc::signal(libc::SIGINT, handler);
        libc::signal(libc::SIGTERM, handler);
    }
    let cfg = match itero_ai::server::config_from_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("itero-ai: {e:#}");
            std::process::exit(2);
        }
    };
    // started by itero: when that process is gone, so is the reason to run
    // SAFETY: getppid has no preconditions and cannot fail
    let parent = unsafe { libc::getppid() };
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(std::time::Duration::from_secs(2));
            // SAFETY: as above
            if unsafe { libc::getppid() } != parent {
                itero_ai::server::stop();
            }
        }
    });
    let engine = match itero_ai::llama::LlamaEngine::new(cfg.index.clone()) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("itero-ai: {e:#}");
            std::process::exit(1);
        }
    };
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("itero-ai: {e}");
            std::process::exit(1);
        }
    };
    if let Err(e) = runtime.block_on(itero_ai::server::serve(cfg, Box::new(engine))) {
        eprintln!("itero-ai: {e:#}");
        std::process::exit(1);
    }
}
