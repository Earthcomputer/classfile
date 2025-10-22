use proc_macro::TokenStream;
use quote::quote;
use sha2::{Digest, Sha256};
use std::ffi::OsStr;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use walkdir::WalkDir;

#[proc_macro]
pub fn include_class(input: TokenStream) -> TokenStream {
    // parse a single string literal
    let lit = syn::parse_macro_input!(input as syn::LitStr);
    let path_str = lit.value();

    // Resolve path relative to the current working directory (this is normally the consumer crate root
    // when the proc-macro is invoked during `cargo build`). Accept absolute paths as-is.
    let source_path = if Path::new(&path_str).is_absolute() {
        PathBuf::from(&path_str)
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(Path::new(&path_str))
    };

    let source_path = match fs::canonicalize(&source_path) {
        Ok(p) => p,
        Err(e) => {
            return compile_error(&format!(
                "include_class!: can't canonicalize Java path '{}': {}",
                path_str, e
            ));
        }
    };

    // Read source and compute hash to detect changes
    let src_bytes = match fs::read(&source_path) {
        Ok(b) => b,
        Err(e) => {
            return compile_error(&format!(
                "include_class!: can't read Java file '{}': {}",
                source_path.display(),
                e
            ));
        }
    };

    let mut hasher = Sha256::new();
    hasher.update(&src_bytes);
    hasher.update(source_path.to_string_lossy().as_bytes());
    let src_hash = hex::encode(hasher.finalize());

    // Determine cache directory (under proc-macro crate OUT_DIR when available, else system temp)
    let out_dir = std::env::var("OUT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let mut t = std::env::temp_dir();
            t.push("include_class_macro_out");
            t
        });

    let cache_root = out_dir.join("include_class_cache");
    let this_cache = cache_root.join(&src_hash);

    // Check metadata file
    let meta_file = this_cache.join(".source.sha256");
    let need_compile = match fs::read_to_string(&meta_file) {
        Ok(existing_hash) => existing_hash != src_hash,
        Err(_) => true,
    };

    if need_compile {
        // Clean previous cache for this source
        if this_cache.exists() {
            let _ = fs::remove_dir_all(&this_cache);
        }
        fs::create_dir_all(&this_cache).unwrap();

        // Run javac with -d <this_cache>
        let javac = which::which("javac").ok();
        let javac = match javac {
            Some(p) => p,
            None => {
                return compile_error(
                    "include_class!: 'javac' not found on PATH. Please install Java JDK.",
                );
            }
        };

        let output = Command::new(javac)
            .arg("-d")
            .arg(&this_cache)
            .arg(&source_path)
            .output();

        match output {
            Ok(out) => {
                if !out.status.success() {
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    return compile_error(&format!(
                        "include_class!: javac failed for '{}':\n{}",
                        source_path.display(),
                        stderr
                    ));
                }
            }
            Err(e) => {
                return compile_error(&format!(
                    "include_class!: failed to run javac for '{}': {}",
                    source_path.display(),
                    e
                ));
            }
        }

        // Write metadata (source hash) so we can skip next time
        if let Err(e) =
            fs::File::create(&meta_file).and_then(|mut f| f.write_all(src_hash.as_bytes()))
        {
            // Non-fatal; continue
            let _ = e;
        }
    }

    // Find all class files produced under this_cache
    let mut class_paths: Vec<PathBuf> = Vec::new();
    for entry in WalkDir::new(&this_cache).min_depth(1) {
        match entry {
            Ok(e) => {
                if e.file_type().is_file() && e.path().extension() == Some(OsStr::new("class")) {
                    class_paths.push(e.path().to_path_buf());
                }
            }
            Err(_) => {}
        }
    }

    if class_paths.is_empty() {
        return compile_error(&format!(
            "include_class!: compilation didn't produce any .class files for '{}'.",
            source_path.display()
        ));
    }

    // Sort for deterministic output
    class_paths.sort();

    // Create token fragments that are `&(include_bytes!("...")[..])` -> &'static [u8]
    let includes: Vec<proc_macro2::TokenStream> = class_paths
        .iter()
        .map(|p| {
            let lit = syn::LitStr::new(&p.to_string_lossy(), proc_macro2::Span::call_site());
            // note parentheses so slicing happens first, then we take a reference: &( ... [..] )
            quote! { &(include_bytes!(#lit).as_slice()) }
        })
        .collect();

    // Wrap in a const typed as `&'static [&'static [u8]]` so array->slice coercion happens implicitly.
    let expanded = quote! {
        {
            const __INCLUDED_CLASSES: &'static [&'static [u8]] = &[
                #(#includes),*
            ];
            __INCLUDED_CLASSES
        }
    };

    expanded.into()
}

fn compile_error(msg: &str) -> TokenStream {
    let error_msg = syn::LitStr::new(msg, proc_macro2::Span::call_site());
    quote!(compile_error!(#error_msg);).into()
}
