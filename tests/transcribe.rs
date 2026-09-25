//! End-to-end: voice a fixture with `say`, run it through `transcribe-file`,
//! expect the words back. Ignored by default (it downloads the tiny model on
//! first use); CI runs it with `cargo test -- --ignored` and a cached models
//! folder.

use std::process::Command;

#[test]
#[ignore]
fn transcribe_say_fixture() {
    let dir = std::env::temp_dir().join(format!("momr-e2e-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let fixture = dir.join("fixture.aiff");
    let said = Command::new("say")
        .arg("-o")
        .arg(&fixture)
        .arg("The quick brown fox jumps over the lazy dog")
        .status()
        .expect("say voices the fixture");
    assert!(said.success());
    let transcribed = Command::new(env!("CARGO_BIN_EXE_momr"))
        .arg("transcribe-file")
        .arg(&fixture)
        .arg("--model")
        .arg("tiny")
        .arg("--language")
        .arg("en")
        .output()
        .expect("transcribe-file runs");
    assert!(transcribed.status.success());
    let text = String::from_utf8_lossy(&transcribed.stdout).to_lowercase();
    assert!(text.contains("fox"), "{text}");
    assert!(text.contains("dog"), "{text}");
    assert!(text.contains("speaker 1"), "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}
