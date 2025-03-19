/// Parse rustc args.
mod args;
pub use args::RustcArgs;

// TODO: rustc config or driver can be added/moved here.

#[cfg(test)]
mod tests_args;
