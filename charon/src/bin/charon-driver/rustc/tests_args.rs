use super::RustcArgs;

fn make_args(v: &[&str]) -> Vec<String> {
    v.iter().map(|&s| s.to_owned()).collect()
}

#[test]
#[should_panic]
fn invalid_rustc_args() {
    // [stderr] error: Unrecognized option: 'invalid'
    RustcArgs::new(&make_args(&["--invalid"]));
}

#[test]
fn compact_opt_val() {
    // compact `--opt=val`
    let args = RustcArgs::new(&make_args(&["a.rs", "--crate-name=b.rs"]));
    assert_eq!(args.get_source_file().unwrap(), "a.rs");
    assert_eq!(args.get_opt_str("crate-name").unwrap(), "b.rs");
}

#[test]
fn non_compact_opt_val() {
    // `--opt val`
    let args = RustcArgs::new(&make_args(&["a.rs", "--crate-name", "b.rs"]));
    assert_eq!(args.get_source_file().unwrap(), "a.rs");
    assert_eq!(args.get_opt_str("crate-name").unwrap(), "b.rs");
}

#[test]
fn source_file_vs_non_compact_opt_val() {
    // free arg (source file) vs non-compact val
    let args = RustcArgs::new(&make_args(&["--crate-name", "b.rs", "a.rs"]));
    assert_eq!(args.get_source_file().unwrap(), "a.rs");
    assert_eq!(args.get_opt_str("crate-name").unwrap(), "b.rs");
}
