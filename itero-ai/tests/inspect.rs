//! One model file's header. llama.cpp's backend starts once a process:
//! this file's only test.
#[test]
fn a_files_header_is_read_without_its_weights() {
    let backend = llama_cpp_2::llama_backend::LlamaBackend::init().unwrap();
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/vocab.gguf");
    let v = itero_ai::inspect::inspect(&backend, &p).unwrap();
    assert_eq!(v["architecture"], "bert");
    assert_eq!(v["licence"], "unknown");
    assert_eq!(v["chat"], false);
    assert_eq!(v["thinking"], false);
    assert_eq!(v["bytes"], std::fs::metadata(&p).unwrap().len());
    assert!(v["runs"].is_boolean());
    let gone = p.with_file_name("yok.gguf");
    let e = itero_ai::inspect::inspect(&backend, &gone).unwrap_err();
    assert!(e.to_string().contains("yok.gguf"), "{e}");
    let not = p.with_file_name("değil.gguf");
    std::fs::write(&not, b"bir metin").unwrap();
    let e = itero_ai::inspect::inspect(&backend, &not).unwrap_err();
    std::fs::remove_file(&not).unwrap();
    assert!(e.to_string().contains("not a GGUF file"), "{e}");
}
