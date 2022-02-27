#![feature(rustc_private, register_tool)]
#![feature(box_syntax, box_patterns)]
#![feature(cell_leak)] // For Ref::leak
// For rustdoc: prevents overflows
#![recursion_limit = "256"]

extern crate env_logger;
extern crate hashlink;
extern crate im;
extern crate linked_hash_set;
extern crate log;
extern crate rustc_ast;
extern crate rustc_borrowck;
extern crate rustc_const_eval;
extern crate rustc_driver;
extern crate rustc_errors;
extern crate rustc_hir;
extern crate rustc_index;
extern crate rustc_interface;
extern crate rustc_middle;
extern crate rustc_mir_dataflow;
extern crate rustc_mir_transform;
extern crate rustc_monomorphize;
extern crate rustc_resolve;
extern crate rustc_session;
extern crate rustc_span;
extern crate rustc_target;

#[macro_use]
mod common;
mod assumed;
mod cfim_ast;
mod cfim_ast_utils;
mod cfim_export;
mod divergent;
mod expressions;
mod expressions_utils;
mod formatter;
mod generics;
mod get_mir;
mod graphs;
mod id_vector;
mod im_ast;
mod im_ast_utils;
mod im_to_cfim;
mod insert_assign_return_unit;
mod names;
mod names_utils;
mod reconstruct_asserts;
mod regions_hierarchy;
mod register;
mod reorder_decls;
mod rust_to_local_ids;
mod simplify_binops;
mod translate_functions_to_im;
mod translate_types;
mod types;
mod types_utils;
mod values;
mod values_utils;

use log::info;
use rustc_driver::{Callbacks, Compilation, RunCompiler};
use rustc_interface::{interface::Compiler, Queries};
use rustc_middle::ty::TyCtxt;
use rustc_session::Session;
use serde::Deserialize;
use serde_json;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use structopt::StructOpt;

struct ToInternal {
    dest_dir: Option<PathBuf>,
    source_file: PathBuf,
    no_code_duplication: bool,
}

impl Callbacks for ToInternal {
    fn after_analysis<'tcx>(&mut self, c: &Compiler, queries: &'tcx Queries<'tcx>) -> Compilation {
        // TODO: extern crates
        queries
            .global_ctxt()
            .unwrap()
            .peek_mut()
            .enter(|tcx| {
                let session = c.session();
                translate(session, tcx, &self)
            })
            .unwrap();
        Compilation::Stop
    }
}

/// Initialize the logger. We use a custom initialization to add some
/// useful debugging information, including the line number in the file.
fn initialize_logger() {
    use chrono::offset::Local;
    use env_logger::fmt::Color;
    use env_logger::{Builder, Env};
    use std::io::Write;

    // Create a default environment, by using the environment variables.
    // We do this to let the user choose the log level (i.e.: trace,
    // debug, warning, etc.)
    let env = Env::default();
    // If the log level is not set, set it to "info"
    let env = env.default_filter_or("info");

    // Initialize the log builder from the environment we just created
    let mut builder = Builder::from_env(env);

    // Modify the output format - we add the line number
    builder.format(|buf, record| {
        // Retreive the path (CRATE::MODULE) and the line number
        let path = match record.module_path() {
            Some(s) => s,
            None => "",
        };
        let line = match record.line() {
            Some(l) => l.to_string(),
            None => "".to_string(),
        };

        // Style for the brackets (change the color)
        let mut bracket_style = buf.style();
        bracket_style.set_color(Color::Rgb(120, 120, 120));

        writeln!(
            buf,
            "{}{} {} {}:{}{} {}",
            bracket_style.value("["),
            Local::now().format("%H:%M:%S"), // Rk.: use "%Y-%m-%d" to also have the date
            buf.default_styled_level(record.level()), // Print the level with colors
            path,
            line,
            bracket_style.value("]"),
            record.args()
        )
    });

    builder.init();
}

/// This structure is used to store the command-line instructions.
/// We automatically derive a command-line parser based on this structure.
#[derive(StructOpt)]
#[structopt(name = "Charon")]
struct CliOpts {
    /// The input file.
    #[structopt(parse(from_os_str))]
    input_file: PathBuf,
    /// The destination directory, if we don't want to generate the output
    /// in the same directory as the input file.
    #[structopt(long = "dest", parse(from_os_str))]
    dest_dir: Option<PathBuf>,
    /// If `true`, use Polonius' non-lexical lifetimes (NLL) analysis.
    #[structopt(long = "nll")]
    use_polonius: bool,
    /// Check that no code duplication happens during control-flow reconstruction.
    /// This is only used to make sure the reconstructed code is of good quality,
    /// and preventing duplication is not always possible (if match branches are
    /// "fused").
    #[structopt(long = "no-code-duplication")]
    no_code_duplication: bool,
}

// The following helpers are used to read crate manifests (the `Cargo.toml` files),
// and come from [hacspec](https://github.com/hacspec/): all credits to them.

#[derive(Default, Deserialize)]
struct Dependency {
    name: String,
    #[allow(dead_code)]
    kind: Option<String>,
}

#[derive(Default, Deserialize)]
struct Target {
    #[allow(dead_code)]
    name: String,
    #[allow(dead_code)]
    kind: Vec<String>,
    #[allow(dead_code)]
    crate_types: Vec<String>,
    #[allow(dead_code)]
    src_path: String,
}

#[derive(Default, Deserialize)]
struct Package {
    #[allow(dead_code)]
    name: String,
    #[allow(dead_code)]
    targets: Vec<Target>,
    dependencies: Vec<Dependency>,
}

#[derive(Default, Deserialize)]
struct Manifest {
    packages: Vec<Package>,
    #[allow(dead_code)]
    target_directory: String,
}

/// Small helper. See [compute_external_deps]
fn compiled_to_lib_name(remove_pre: bool, no_ext_filename: String) -> String {
    // We need to convert the filename to a vector of chars - slices of strings
    // operate over bytes, not characters
    let filename: Vec<char> = no_ext_filename.chars().collect();

    // Remove the "lib" prefix, if necessary.
    // We have to clone because borrows can't outlive the blocks in which
    // they are created, which is slightly annoying...
    let filename: Vec<char> = if remove_pre {
        let pre: Vec<char> = "lib".to_string().chars().collect();
        assert!(filename.len() > pre.len());
        assert!(&filename[0..pre.len()] == pre);
        filename[pre.len()..].to_vec()
    } else {
        filename
    };

    // Remove the hash suffix
    assert!(filename.len() > 0);
    let mut i = filename.len() - 1;
    while i > 0 {
        if filename[i] == '-' {
            return filename[0..i].iter().collect::<String>();
        }
        i -= 1;
    }
    // If we got there, it means we couldn't spot the '-' character delimiting
    // the hash suffix
    unreachable!("Invalid compiled file name: {:?}", no_ext_filename);
}

/// Small utility. See [compute_external_deps].
fn insert_if_not_present(map: &mut HashMap<String, String>, lib_name: String, filename: String) {
    // Check that there isn't another compiled library for this dependency
    if map.contains_key(&lib_name) {
        let prev_filename = map.get(&lib_name).unwrap();
        error!("Found two compiled library files for the same external dependency ({:?}): {:?}, {:?}. You may want to clean and rebuild the project: `cargo clean && cargo build`",
                    lib_name, prev_filename, filename);
        panic!();
    }

    // Insert in the map
    trace!("lib to compiled: {:?} -> {:?}", lib_name, filename);
    map.insert(lib_name, filename);
}

/// Compute the external dependencies of a crate, by reading the manifest.
///
/// We face the issue that we directly call the rust compiler, rather than
/// `cargo`, and thus have to give very precise arguments to our invocation
/// of rustc (more specifically: we need to provide the list of external
/// dependencies).
///
/// This is slightly annoying to do, and we place ourselves in the situation
/// where the project is built through `cargo`, and the user built the
/// (debug version) of the project *before* calling Charon. In this situation,
/// we can leverage the fact that the external dependencies have already been
/// compiled, and can be found in `/target/debug/deps/`.
/// We thus don't have to build them (and don't want anyway! Charon is not a
/// build system), and just need to:
/// - use the manifest (the `Cargo.toml` file) to retrieve the list of external
///   dependencies
/// - explore the `/target/debug/deps` folder to retrieve the names of the
///   compiled libraries, to compute the arguments with which to invoke the
///   Rust compiler
///
/// Finally, the code used in this function to read the manifest and compute
/// the list of external dependencies is greatly inspired by the code used in
/// [hacspec](https://github.com/hacspec/), so all credits to them.
fn compute_external_deps(source_file: &PathBuf) -> Vec<String> {
    use std::str::FromStr;

    // Compute the path to the crate
    // Use the source file as a starting point.
    // Remove the file name
    let source_file = std::fs::canonicalize(&source_file).unwrap();
    let crate_path = source_file.as_path().parent().unwrap().parent().unwrap();
    let mut manifest_path = crate_path.to_path_buf();
    manifest_path.push(PathBuf::from_str("Cargo.toml").unwrap());

    // First, read the manifest (comes from hacspec)
    info!("Reading manifest: {:?}", manifest_path);

    // Compute the command to apply
    let output_args = vec![
        // We want to read the metadata
        "metadata".to_string(),
        // Don't list the dependencies of the dependencies (useful if we
        // implement something like cargo and need to transitively build all
        // the dependencies, but this is not the point here)
        "--no-deps".to_string(),
        // For stability (and to prevent cargo from printing an annoying warning
        // message), select a format version
        "--format-version".to_string(),
        "1".to_string(),
        // We need to provide the path to the manifest
        "--manifest-path".to_string(),
        manifest_path.to_str().unwrap().to_string(),
    ];

    trace!("cargo metadata command args: {:?}", output_args);

    // Apply the command
    let output = std::process::Command::new("cargo")
        .args(output_args)
        .output()
        .expect(" ⚠️  Error reading cargo manifest.");
    let stdout = output.stdout;
    if !output.status.success() {
        let error =
            String::from_utf8(output.stderr).expect(" ⚠️  Failed reading cargo's stderr output");
        panic!("Error running cargo metadata: {:?}", error);
    }
    let json_string = String::from_utf8(stdout).expect(" ⚠️  Failed reading cargo output");
    let manifest: Manifest = serde_json::from_str(&json_string)
        .expect(" ⚠️  Error reading the manifest (Cargo.toml file) processed by cargo");

    // Build systems can be annoying, especially if we use different versions
    // of the compiler (Charon relies on a nightly version, which may be
    // different from the one used by the user to compile his project! - this
    // can result in rustc considering the compiled libraries as invalid,
    // because of a version mismatch).
    // We don't want to take the user by surprise if something goes wrong,
    // so we print as much information as we can.
    // Rk.: this is a rather problematic issue, because we don't want to force
    // the user to compile his project with a specific version of the compiler.
    // We need to think of a way around (the most brutal way would be to clone
    // the project in a subdirectory, and compile it in debug mode with the
    // proper compiler - by inserting the proper `rust-toolchain` file - before
    // calling charon; this should be easy to script).

    // List the dependencies.
    // We do something simple: we list the dependencies for all the packages,
    // as having useless dependencies shouldn't be a problem.
    // We make sure we don't have duplicates while doing so.
    let mut deps: HashSet<String> = HashSet::new();
    for package in &manifest.packages {
        trace!("Packages: {}", package.name);

        for dep in &package.dependencies {
            deps.insert(dep.name.clone());
        }
    }
    trace!("List of external dependencies: {:?}", deps);

    // Compute the path to the compiled dependencies
    let deps_dir = PathBuf::from_str("target/debug/deps/").unwrap();
    let deps_dir = crate_path.join(deps_dir);
    info!(
        "Looking for the compiled external dependencies in {:?}",
        deps_dir
    );

    // List the files in the dependencies
    // There are .rlib, .d and .so files.
    // All the files have a hash suffix.
    // The .rlib and .so files have a "lib" prefix.
    // Ex.:
    // - External "remote" crates:
    //   "libserde_json-25bfd2343c819291.rlib"
    // - Local crates:
    //   "attributes-b73eebf157017326.d"
    //   "libattributes-b73eebf157017326.so"
    //
    // We list all the compiled files in the target directory and retrieve the
    // original library name (i.e., "serde_json" or "attributes" in the above
    // examples), then comptue a map from library name to compiled files.
    // We check that there is only one compiled file per external
    // dependency while doing so.
    let files = std::fs::read_dir(deps_dir.clone()).unwrap();
    let mut lib_to_rlib: HashMap<String, String> = HashMap::new();
    let mut lib_to_so: HashMap<String, String> = HashMap::new();
    let mut lib_to_d: HashMap<String, String> = HashMap::new();
    for file in files {
        trace!("File: {:?}", file);
        match file {
            std::io::Result::Ok(entry) => {
                let entry = entry.path();

                // We only keep the files with .rlib or .d extension
                let extension = entry.extension();
                if extension.is_none() {
                    continue;
                }
                let extension = extension.unwrap().to_str().unwrap();
                if extension != "rlib" && extension != "so" && extension != "d" {
                    continue;
                }
                // The file has a "lib" prefix if and only if its extension is ".rlib"
                // or ".so"
                let is_rlib = extension == "rlib";
                let is_so = extension == "so";
                let has_prefix = is_rlib || is_so;

                // Retrieve the file name
                let filename = PathBuf::from(entry.file_name().unwrap());

                // Remove the extension
                let no_ext_filename = filename.file_stem().unwrap().to_str().unwrap().to_string();

                // Compute the library name (remove the "lib" prefix for .rlib files,
                // remove the hash suffix)
                let lib_name = compiled_to_lib_name(has_prefix, no_ext_filename);

                // Only keep the libraries for the dependencies we need
                if !(deps.contains(&lib_name)) {
                    continue;
                }

                // Insert in the proper map - note that we need the full path
                let full_path = deps_dir.join(entry).to_str().unwrap().to_string();
                if is_rlib {
                    insert_if_not_present(&mut lib_to_rlib, lib_name, full_path);
                } else if is_so {
                    insert_if_not_present(&mut lib_to_so, lib_name, full_path);
                } else {
                    insert_if_not_present(&mut lib_to_d, lib_name, full_path);
                }
            }
            std::io::Result::Err(_) => {
                panic!("Unexpected error while reading files in: {:?}", deps_dir);
            }
        }
    }

    // Generate the additional arguments
    let mut args: Vec<String> = Vec::new();

    // Add the "-L" dependency
    args.push("-L".to_string());
    args.push(format!("dependency={}", deps_dir.to_str().unwrap().to_string()).to_string());

    // Add the "--extern" arguments
    for dep in deps {
        // Retrieve the path to the compiled library.
        // We first look in the .rlib files, then in the .so files
        let compiled_path = lib_to_rlib.get(&dep);
        let compiled_path = if compiled_path.is_none() {
            lib_to_so.get(&dep)
        } else {
            compiled_path
        };

        if compiled_path.is_none() {
            error!(
                "Could not find a compiled file for the external dependency {:?} in {:?}. You may need to build the crate: `cargo build`.",
                dep, deps_dir
            );
            panic!();
        }
        args.push("--extern".to_string());
        args.push(format!("{}={}", dep, compiled_path.unwrap()).to_string());
    }

    // Return
    trace!("Args vec: {:?}", args);
    args
}

fn main() {
    // Initialize the logger
    initialize_logger();

    // Retrieve the executable path - this is not considered an argument,
    // and won't be parsed by CliOpts
    let exec_path = match std::env::args().next() {
        Some(s) => s.to_owned(),
        None => panic!("Impossible: zero arguments on the command-line!"),
    };

    // Parse the command-line
    let args = CliOpts::from_args();

    // Retrieve the sysroot (the path to the executable of the compiler)
    let out = std::process::Command::new("rustc")
        .arg("--print=sysroot")
        .current_dir(".")
        .output()
        .unwrap();
    let sysroot = std::str::from_utf8(&out.stdout).unwrap().trim();
    let sysroot_arg = format!("--sysroot={}", sysroot).to_owned();

    // Retrieve the list of external dependencies by reading the manifest
    let mut external_deps = compute_external_deps(&args.input_file);

    // Call the Rust compiler with the proper options
    let mut compiler_args = vec![
        exec_path,
        sysroot_arg,
        args.input_file.as_path().to_str().unwrap().to_string(),
        "--crate-type=lib".to_string(),
        "--edition=2018".to_string(),
    ];
    if args.use_polonius {
        compiler_args.push("-Zpolonius".to_string());
    }
    compiler_args.append(&mut external_deps);

    trace!("Compiler args: {:?}", compiler_args);

    // When calling the compiler we provide a callback, which allows us
    // to retrieve the result of compiler queries
    RunCompiler::new(
        &compiler_args,
        &mut ToInternal {
            dest_dir: args.dest_dir,
            source_file: args.input_file,
            no_code_duplication: args.no_code_duplication,
        },
    )
    .run()
    .unwrap();
}

/// Translate a crate to LLBC (Low-Level Borrow Calculus).
///
/// This function is a callback function for the Rust compiler.
fn translate(sess: &Session, tcx: TyCtxt, internal: &ToInternal) -> Result<(), ()> {
    trace!();
    // Retrieve the crate name.
    let crate_name = tcx
        .crate_name(rustc_span::def_id::LOCAL_CRATE)
        .to_ident_string();
    trace!("# Crate: {}", crate_name);

    // # Step 1: check and register all the definitions, to build the graph
    // of dependencies between them (we need to know in which
    // order to extract the definitions, and which ones are mutually
    // recursive). While building this graph, we perform as many checks as
    // we can to make sure the code is in the proper rust subset. Those very
    // early steps mostly involve checking whether some features are used or
    // not (ex.: raw pointers, inline ASM, etc.). More complex checks are
    // performed later. In general, whenever there is ambiguity on the potential
    // step in which a step could be performed, we perform it as soon as possible.
    // Building the graph of dependencies allows us to translate the definitions
    // in the proper order, and to figure out which definitions are mutually
    // recursive.
    // We iterate over the HIR items, and explore their MIR bodies/ADTs/etc.
    // (when those exist - for instance, type aliases don't have MIR translations
    // so we just ignore them).
    let registered_decls = register::register_crate(sess, tcx)?;

    // # Step 2: reorder the graph of dependencies and compute the strictly
    // connex components to:
    // - compute the order in which to extract the definitions
    // - find the recursive definitions
    // - group the mutually recursive definitions
    let ordered_decls = reorder_decls::reorder_declarations(&registered_decls)?;

    // # Step 3: generate identifiers for the types and functions, and compute
    // the mappings from rustc identifiers to our own identifiers
    let ordered_decls = rust_to_local_ids::rust_to_local_ids(&ordered_decls);

    // # Step 4: translate the types
    let (types_constraints, type_defs) = translate_types::translate_types(tcx, &ordered_decls)?;

    // # Step 5: translate the functions to IM (our Internal representation of MIR).
    // Note that from now onwards, both type and function definitions have been
    // translated to our internal ASTs: we don't interact with rustc anymore.
    let im_defs = translate_functions_to_im::translate_functions(
        tcx,
        &ordered_decls,
        &types_constraints,
        &type_defs,
    )?;

    // # Step 6: go from IM to CFIM (Control-Flow Internal MIR) by reconstructing
    // the control flow.
    // TODO: rename CFIM to LLBC (low-level borrow calculus)
    let cfim_defs =
        im_to_cfim::translate_functions(internal.no_code_duplication, &type_defs, &im_defs);

    //
    // =================
    // **Micro-passes**:
    // =================
    // At this point, the bulk of the translation is done. From now onwards,
    // we simply apply some micro-passes to make the code cleaner, before
    // serializing the result.
    //

    // # Step 7: simplify the calls to binops
    // Note that we assume that the sequences have been flattened.
    let cfim_defs = simplify_binops::simplify(cfim_defs);

    for def in &cfim_defs {
        trace!(
            "# After binop simplification:\n{}\n",
            def.fmt_with_defs(&type_defs, &cfim_defs)
        );
    }

    // # Step 8: reconstruct the asserts
    let cfim_defs = reconstruct_asserts::simplify(cfim_defs);

    for def in &cfim_defs {
        trace!(
            "# After asserts reconstruction:\n{}\n",
            def.fmt_with_defs(&type_defs, &cfim_defs)
        );
    }

    // # Step 9: add the missing assignments to the return value.
    // When the function return type is unit, the generated MIR doesn't
    // set the return value to `()`. This can be a concern: in the case
    // of Aeneas, it means the return variable contains ⊥ upon returning.
    // For this reason, when the function has return type unit, we insert
    // an extra assignment just before returning.
    let cfim_defs = insert_assign_return_unit::transform(cfim_defs);

    // # Step 10: compute which functions are potentially divergent. A function
    // is potentially divergent if it is recursive, contains a loop or transitively
    // calls a potentially divergent function.
    // Note that in the future, we may complement this basic analysis with a
    // finer analysis to detect recursive functions which are actually total
    // by construction.
    let _divergent = divergent::compute_divergent_functions(&ordered_decls, &cfim_defs);

    // # Step 11: generate the files.
    cfim_export::export(
        crate_name,
        &ordered_decls,
        &type_defs,
        &cfim_defs,
        &internal.dest_dir,
        &internal.source_file,
    )?;

    trace!("Done");

    Ok(())
}
