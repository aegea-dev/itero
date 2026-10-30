//! #find-models: the models already on the machine, named by where they
//! are. Its own test binary: it sets HOME, XDG_CACHE_HOME, HF_HUB_CACHE
//! and ITERO_AI_BIN for the process.
#![cfg(unix)]
use itero::ai::find::{Root, model_files, rebuild};
use itero::engine::worker::{self, Outcome, StepRun};
use serde_json::Map;
use std::path::{Path, PathBuf};

fn touch(p: &Path) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, b"GGUF").unwrap();
}

fn step(root: &Path) -> StepRun {
    StepRun {
        file: PathBuf::from(worker::FIND_MODELS),
        root: root.into(),
        workflow: "models".into(),
        env: Default::default(),
        input: Map::new(),
        memory: root.join("hafıza"),
        waiting_since: None,
        quiet: 30,
        most: 120,
        now: String::new(),
    }
}

#[test]
fn files_are_found_and_named_by_where_they_are() {
    let d = tempfile::tempdir().unwrap();
    let lm = d.path().join("lmstudio");
    touch(&lm.join("unsloth/Qwen3.5-4B-GGUF/Qwen3.5-4B-Q4_K_M.gguf"));
    touch(&lm.join("unsloth/Qwen3.5-4B-GGUF/Qwen3.5-4B-UD-Q3_K_XL.gguf"));
    touch(&lm.join("unsloth/Qwen3.5-35B-A3B-GGUF/mmproj-F32.gguf"));
    touch(&lm.join("örnek/Büyük-GGUF/Büyük-Q8_0-00001-of-00002.gguf"));
    touch(&lm.join("örnek/Büyük-GGUF/Büyük-Q8_0-00002-of-00002.gguf"));
    touch(&lm.join("yalnız.gguf"));
    let hf = d.path().join("hub");
    touch(&hf.join("models--unsloth--Qwen3.5-4B-GGUF/snapshots/abc123/Qwen3.5-4B-Q4_K_M.gguf"));
    touch(&hf.join("models--google--siglip-base/snapshots/def/model.safetensors"));
    let found = model_files(&[
        Root::LmStudio(lm),
        Root::HfCache(hf),
        Root::LmStudio(d.path().join("yok")),
    ]);
    let mut names: Vec<(String, Option<String>)> = found
        .iter()
        .map(|f| (f.repo.clone(), f.quant_hint.clone()))
        .collect();
    names.sort();
    assert_eq!(
        names,
        vec![
            ("unsloth/Qwen3.5-4B-GGUF".into(), Some("Q4_K_M".into())),
            ("unsloth/Qwen3.5-4B-GGUF".into(), Some("Q4_K_M".into())),
            // Unsloth's Dynamic files keep their prefix, as llama.cpp's -hf names them
            ("unsloth/Qwen3.5-4B-GGUF".into(), Some("UD-Q3_K_XL".into())),
            ("örnek/Büyük-GGUF".into(), Some("Q8_0".into())),
        ]
    );
}

#[test]
fn an_unchanged_file_takes_the_name_its_file_name_says() {
    // an index from before Unsloth's UD- prefix was kept: the file is not
    // read again, but its name is put right
    let d = tempfile::tempdir().unwrap();
    let lm = d.path().join("lmstudio");
    let path = lm.join("unsloth/Qwen3.5-4B-GGUF/Qwen3.5-4B-UD-Q4_K_XL.gguf");
    touch(&path);
    let found = model_files(&[Root::LmStudio(lm)]);
    let m = std::fs::metadata(&path).unwrap();
    let secs = m
        .modified()
        .unwrap()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let old = itero_ai_index::Index {
        models: vec![itero_ai_index::Entry {
            name: "unsloth/Qwen3.5-4B-GGUF:Q4_K_XL".into(),
            repo: "unsloth/Qwen3.5-4B-GGUF".into(),
            quant: "Q4_K_XL".into(),
            paths: vec![path.clone()],
            maker: "unsloth".into(),
            params: Some("4B".into()),
            architecture: "qwen35".into(),
            context: Some(262_144),
            bytes: m.len(),
            licence: "apache-2.0".into(),
            chat: true,
            runs: true,
            thinking: true,
            modified: secs,
            downloaded: None,
            removed: Some("2026-10-29T10:00:00Z".into()),
        }],
    };
    let (ix, inspected) = rebuild(&old, &found, &mut |_| panic!("read again")).unwrap();
    assert_eq!(inspected, 0);
    let e = &ix.models[0];
    assert_eq!(
        (e.name.as_str(), e.quant.as_str()),
        ("unsloth/Qwen3.5-4B-GGUF:UD-Q4_K_XL", "UD-Q4_K_XL")
    );
    assert!(e.removed.is_some());
}

#[test]
fn the_index_is_written_kept_and_its_marks_carried() {
    let bin = Path::new(env!("CARGO_BIN_EXE_itero")).with_file_name("itero-ai");
    assert!(
        bin.is_file(),
        "{} is not built: cargo build -p itero-ai",
        bin.display()
    );
    let d = tempfile::tempdir().unwrap();
    let home = d.path().join("ev");
    let lm = home.join(".lmstudio/models/örnek/Sözlük-GGUF");
    std::fs::create_dir_all(&lm).unwrap();
    let vocab = Path::new(env!("CARGO_MANIFEST_DIR")).join("itero-ai/tests/data/vocab.gguf");
    std::fs::copy(&vocab, lm.join("Sözlük-F16.gguf")).unwrap();
    // SAFETY: this binary's only test that sets them, before any thread
    unsafe {
        std::env::set_var("HOME", &home);
        std::env::set_var("XDG_CACHE_HOME", d.path().join("önbellek"));
        std::env::set_var("HF_HUB_CACHE", d.path().join("hf-yok"));
        std::env::set_var("ITERO_AI_BIN", &bin);
        std::env::remove_var("ITERO_AI_SOCKET");
    }
    let root = d.path().join("çalışma");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("itero.config.json"), "{}").unwrap();

    let out = worker::run_step(&step(&root)).unwrap();
    assert_eq!(
        out,
        Outcome::Done(serde_json::json!({ "models": 1, "inspected": 1 }))
    );
    let index = d.path().join("önbellek/itero/models.json");
    let mut ix = itero_ai_index::Index::read(&index).unwrap();
    let e = &ix.models[0];
    assert_eq!(e.repo, "örnek/Sözlük-GGUF");
    assert_eq!(e.architecture, "bert");
    assert!(!e.chat);

    // a mark is kept; an unchanged file is not inspected again
    ix.models[0].removed = Some("2026-10-29T10:00:00Z".into());
    ix.write(&index).unwrap();
    let out = worker::run_step(&step(&root)).unwrap();
    assert_eq!(
        out,
        Outcome::Done(serde_json::json!({ "models": 1, "inspected": 0 }))
    );
    let ix = itero_ai_index::Index::read(&index).unwrap();
    assert!(ix.models[0].removed.is_some());

    // a file gone takes its entry with it
    std::fs::remove_file(lm.join("Sözlük-F16.gguf")).unwrap();
    let out = worker::run_step(&step(&root)).unwrap();
    assert_eq!(
        out,
        Outcome::Done(serde_json::json!({ "models": 0, "inspected": 0 }))
    );
}
