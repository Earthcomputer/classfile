use std::env;
use std::ffi::OsStr;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=JAVA_HOME");
    let javac = match env::var_os("JAVA_HOME") {
        Some(java_home) => PathBuf::from(java_home).join("bin").join("javac"),
        None => which::which("javac").expect("Could not find javac in JAVA_HOME or PATH"),
    };

    let version = Command::new(&javac).arg("-version").output().expect("Could not execute javac").stdout;
    let version = version.strip_prefix(b"javac ").expect("Invalid javac -version output");
    if !version.starts_with(b"21.") {
        panic!("javac version 21 expected. Please set the JAVA_HOME env var to a copy of JDK 21");
    }
    println!("cargo:rustc-env=JAVA_VERSION={}", String::from_utf8_lossy(version));

    let input_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).parent().unwrap().join("test_data");
    println!("cargo:rerun-if-changed={}", input_dir.display());

    let output_dir = PathBuf::from(env::var("OUT_DIR").unwrap()).join("test_data/");
    println!("cargo:rustc-env=JAVA_OUT_DIR={}", output_dir.display());

    let mut cmd = Command::new(javac);
    cmd.arg("-d").arg(output_dir);
    cmd.arg("--module-version").arg("1.2.3");
    for file in walkdir::WalkDir::new(&input_dir).min_depth(1) {
        let file = file.unwrap();
        if file.file_type().is_file() && file.path().extension() == Some(OsStr::new("java")) {
            cmd.arg(file.path());
        }
    }

    let compile_output = cmd.output().expect("Could not execute javac");
    if !compile_output.status.success() {
        panic!("Failed to compile with javac: {}", String::from_utf8_lossy(&compile_output.stderr));
    }
}
