use std::process::Command;

#[test]
fn binary_starts_without_output() {
    let output = Command::new(env!("CARGO_BIN_EXE_oafmt"))
        .output()
        .expect("oafmt binary should start");

    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}
