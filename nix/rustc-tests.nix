{ bash
, charon
, coreutils
, fd
, fetchFromGitHub
, lib
, parallel
, pv
, runCommand
, rustToolchain
, writeScript
}:

let
  # The rustc commit we use to get the tests. We should update it every now and
  # then to match the version of rustc we're using.
  tests_commit = "65ea825f4021eaf77f1b25139969712d65b435a4";
  tests_hash = "sha256-0dsWuGcWjQpj/N4iG6clCzM8kjrDjE+dQfyL3iuBGiY=";

  rustc-test-suite = fetchFromGitHub {
    owner = "rust-lang";
    repo = "rust";
    rev = tests_commit;
    sha256 = tests_hash;
  };

  # The commit that corresponds to our nightly pin, for when we want to update the pinned commit.
  toolchain_commit = runCommand "get-rustc-commit" { } ''
    # This is sad but I don't know a better way.
    cat ${rustToolchain}/share/doc/rust/html/version_info.html \
      | grep 'github.com' \
      | sed 's#.*"https://github.com/rust-lang/rust/commit/\([^"]*\)".*#\1#' \
      > $out
  '';

  # Run charon on a single test file. This writes the charon output to
  # `<file>.rs.charon-output` and the exit status to `<file>.rs.charon-status`.
  run_rustc_test = writeScript "charon-run-rustc-test" ''
    #!${bash}/bin/bash
    FILE="$1"

    has_magic_comment() {
      # Checks for `// magic-comment` and `//@ magic-comment` instructions in files.
      grep -q "^// \?@\? \?$1:" "$2"
    }

    has_feature() {
      # Checks for `#![feature(...)]`.
      grep -q "^#!.feature($1)" "$2"
    }

    if has_magic_comment 'aux-build' "$FILE" \
      || has_magic_comment 'compile-flags' "$FILE" \
      || has_magic_comment 'revisions' "$FILE" \
      || has_magic_comment 'known-bug' "$FILE" \
      || has_magic_comment 'edition' "$FILE"\
      ; then
        result="unsupported-build-settings"
    elif has_feature 'generic_const_exprs' "$FILE" \
      ; then
        result="unsupported-feature"
    else
        ${coreutils}/bin/timeout 10s ${charon}/bin/charon --no-cargo --input "$FILE" --dest-file "$FILE.llbc" > "$FILE.charon-output" 2>&1
        result=$?
    fi
    echo -n $result > "$FILE.charon-status"
  '';

  # Runs charon on the whole rustc ui test suite. This returns the tests
  # directory with a bunch of `<file>.rs.charon-output` and
  # `<file>.rs.charon-status` files, see `run_rustc_test`.
  run_rustc_tests = runCommand "charon-run-rustc-tests"
    {
      src = rustc-test-suite;
      buildInputs = [ rustToolchain parallel pv fd ];
    } ''
    mkdir $out
    cp -r $src/tests/ui/* $out
    chmod -R u+w $out
    cd $out

    SIZE="$(fd -e rs | wc -l)"
    echo "Running $SIZE tests..."
    fd -e rs \
        | parallel ${run_rustc_test} \
        | pv -l -s "$SIZE"
  '';

  # Report the status of a single file.
  analyze_test_output = writeScript "charon-analyze-test-output" ''
    #!${bash}/bin/bash
    FILE="$1"
    echo -n "$FILE: "

    status="$(cat "$FILE.charon-status")"
    if echo "$status" | grep -q '^unsupported'; then
        result="⊘ $status"
    elif [ $status -eq 124 ]; then
        result="❌ timeout"
    elif [ $status -eq 101 ] || [ $status -eq 255 ]; then
        result="❌ charon-panic"
    elif [ -f ${"$"}{FILE%.rs}.stderr ]; then
        # This is a test that should fail
        if [ $status -eq 0 ]; then
            result="❌ success-when-failure-expected"
        else
            result="✅ expected-failure"
        fi
    elif [ $status -eq 0 ]; then
        if [ -e "$FILE.llbc" ]; then
            result="✅ expected-success"
        else
            result="❌ success-but-no-llbc-output"
        fi
    else
        if grep -q 'error.E9999' "$FILE.charon-output"; then
            result="❌ hax-failure-when-success-expected"
        elif [ -e "$FILE.llbc" ]; then
            result="❌ failure-when-success-expected (with llbc output)"
        else
            result="❌ failure-when-success-expected (without llbc output)"
        fi
    fi

    echo "$result"
  '';

  # Adds a `charon-results` file that records
  # `success|expected-failure|failure|panic|timeout` for each file we
  # processed.
  analyze_test_outputs = runCommand "charon-analyze-test-outputs"
    {
      src = run_rustc_tests;
      buildInputs = [ parallel pv fd ];
    } ''
    mkdir $out
    chmod -R u+w $out
    cd $out
    ln -s $src test-results

    SIZE="$(fd --follow -e rs | wc -l)"
    echo "Running $SIZE tests..."
    fd --follow -e rs \
        | parallel ${analyze_test_output} \
        | pv -l -s "$SIZE" \
        > charon-results

    cat charon-results | cut -d':' -f 2 | sort | uniq -c > charon-summary
  '';

in
{
  inherit toolchain_commit rustc-test-suite;
  rustc-tests = analyze_test_outputs;
}
