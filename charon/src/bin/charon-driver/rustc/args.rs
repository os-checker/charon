/// The rustc cli parser wrapper:
/// * all args are parsed by rustc
/// * invalid cli args will cause panic on construction in [`RustcArgs::new`]
pub struct RustcArgs {
    opts: rustc_session::getopts::Matches,
    _ctxt: rustc_session::EarlyDiagCtxt,
}

impl RustcArgs {
    /// Pass rustc args except the first arg rustc bin itself.
    pub fn new(args: &[String]) -> Self {
        let early_dcx = rustc_session::EarlyDiagCtxt::new(Default::default());
        Self {
            opts: rustc_driver::handle_options(&early_dcx, args).unwrap(),
            _ctxt: early_dcx,
        }
    }

    pub fn get_source_file(&self) -> Option<&str> {
        // Only check the first free opts as source file, because we leave the error
        // reporting job to rustc:
        // $ rustc a.rs b.rs
        // error: multiple input filenames provided (first two filenames are `a.rs` and `b.rs`)
        self.opts.free.first().map(|s| &**s)
    }

    /// Get the single value for a given cli argument. The arg should be *without leading `--`*.
    pub fn get_opt_str(&self, arg: &str) -> Option<String> {
        self.opts.opt_str(arg)
    }
}
