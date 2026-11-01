//! itero-ai: a local model for itero, behind the OpenAI Chat Completions
//! API on a Unix socket. itero starts it with what it needs in its
//! environment; it has no commands of its own.
use std::sync::atomic::{AtomicBool, Ordering};

/// Until the engine is made: no model can be loaded, and serving has not
/// begun to hear a stop. Making it can take 15 seconds and more (a Mac may
/// compile Metal's shaders), and nothing else ends it.
static STARTING: AtomicBool = AtomicBool::new(true);

/// Ends the process now while it starts, else serving, as when idle, so
/// the model is freed before the process exits. Safe in a signal handler:
/// it only reads and stores atomics, and `_exit` is async-signal-safe.
fn stop() {
    if STARTING.load(Ordering::SeqCst) {
        // SAFETY: _exit has no preconditions, and runs no exit handlers
        unsafe { libc::_exit(0) };
    }
    itero_ai::server::stop();
}

/// SIGTERM or SIGINT: the daemon stopping, a person's Ctrl-C.
extern "C" fn on_stop(_: libc::c_int) {
    stop();
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
                stop();
            }
        }
    });
    // for the tests: a start as long as compiling Metal's shaders can be
    if let Some(secs) = std::env::var("ITERO_TEST_START_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
    {
        eprintln!("itero-ai: starting");
        std::thread::sleep(std::time::Duration::from_secs(secs));
    }
    let engine = match itero_ai::llama::LlamaEngine::new(cfg.index.clone()) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("itero-ai: {e:#}");
            std::process::exit(1);
        }
    };
    STARTING.store(false, Ordering::SeqCst);
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
