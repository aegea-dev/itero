//! What only reads model files' headers starts its itero-ai without the
//! GPU; only the itero-ai that answers steps uses it. A stand-in for
//! itero-ai says what it was given, and ends. Its own test binary: it sets
//! HOME, XDG_CACHE_HOME and ITERO_AI_BIN for the process.
#![cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

const SAYS_DEVICES: &str = "#!/bin/sh\necho \"aygıt=[$GGML_METAL_DEVICES]\" >&2\nexit 1\n";

#[test]
fn only_what_answers_steps_uses_the_gpu() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path().join("ev");
    let models = home.join(".lmstudio/models/örnek/Küçük-GGUF");
    std::fs::create_dir_all(&models).unwrap();
    let file = models.join("Küçük-Q4_K_M.gguf");
    std::fs::write(&file, b"GGUF").unwrap();
    // written by a child, so no thread of this process keeps it open
    let bin = d.path().join("itero-ai");
    let wrote = Command::new("sh")
        .arg("-c")
        .arg(format!("cat > '{}'", bin.display()))
        .stdin(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut c| {
            use std::io::Write;
            c.stdin.take().unwrap().write_all(SAYS_DEVICES.as_bytes())?;
            c.wait()
        })
        .unwrap();
    assert!(wrote.success());
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    // SAFETY: this binary's only test, before any thread starts
    unsafe {
        std::env::set_var("HOME", &home);
        std::env::set_var("XDG_CACHE_HOME", d.path().join("önbellek"));
        std::env::set_var("HF_HUB_CACHE", d.path().join("hf-yok"));
        std::env::set_var("ITERO_AI_BIN", &bin);
        std::env::remove_var("ITERO_AI_SOCKET");
    }
    let run = d.path().join("k");

    let steps = itero::ai::ensure_at(run.join("a.sock"), Duration::from_secs(5), |_| ());
    let e = format!("{:#}", steps.err().expect("the stand-in ends"));
    assert!(e.contains("aygıt=[]"), "steps keep the GPU: {e}");

    let e = itero::ai::find::scan_on(run.join("b.sock")).unwrap_err();
    assert!(e.contains("aygıt=[0]"), "a scan needs no GPU: {e}");

    let out = Command::new(Path::new(env!("CARGO_BIN_EXE_itero")))
        .args(["ai", "inspect"])
        .arg(&file)
        .current_dir(d.path())
        .output()
        .unwrap();
    let e = String::from_utf8_lossy(&out.stderr);
    assert!(e.contains("aygıt=[0]"), "inspect needs no GPU: {e}");
}
