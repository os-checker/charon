{
  description = "Charon";

  inputs = {
    flake-utils.url = "github:numtide/flake-utils";
    flake-compat.url = "github:nix-community/flake-compat";
    nixpkgs.url = "nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.flake-utils.follows = "flake-utils";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    crane = {
      url = "github:ipetkov/crane";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, flake-utils, nixpkgs, rust-overlay, crane, flake-compat }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };

        rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.template;
        craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

        cleanedUpSrc = pkgs.lib.cleanSourceWith {
          src = ./charon;
          filter = path: type:
            (craneLib.filterCargoSources path type)
              || (pkgs.lib.hasPrefix (toString ./charon/tests) path)
              || (path == toString ./charon/rust-toolchain);
        };
        craneArgs = {
          src = cleanedUpSrc;
          RUSTFLAGS="-D warnings"; # Turn all warnings into errors.
        };

        charon = craneLib.buildPackage (craneArgs // {
          buildInputs = [ pkgs.makeWrapper pkgs.zlib ];
          # It's important to pass the same `RUSTFLAGS` to dependencies otherwise we'll have to rebuild them.
          cargoArtifacts = craneLib.buildDepsOnly craneArgs;
          # Make sure the toolchain is in $PATH so that `cargo` can work
          # properly.
          postFixup = ''
            wrapProgram $out/bin/charon \
              --set CHARON_TOOLCHAIN_IS_IN_PATH 1 \
              --prefix PATH : "${pkgs.lib.makeBinPath [ rustToolchain ]}"
          '';
          # Check the `ui_llbc` files are correct instead of overwriting them.
          cargoTestCommand = "IN_CI=1 cargo test --profile release";
        });

        # Check rust files are correctly formatted.
        charon-check-fmt = craneLib.cargoFmt craneArgs;

        tests =
          let cargoArtifacts = craneLib.buildDepsOnly { src = ./tests; };
          in craneLib.buildPackage {
            name = "tests";
            src = ./tests;
            inherit cargoArtifacts;
            buildPhase = ''
              # Run the tests for Charon
              DEST=$out CHARON="${charon}/bin/charon" make charon-tests
            '';
            doCheck = false;
            dontInstall = true;
          };

        tests-polonius = let
          cargoArtifacts =
            craneLib.buildDepsOnly { src = ./tests-polonius; };
        in craneLib.buildPackage {
          name = "tests-polonius";
          src = ./tests-polonius;
          inherit cargoArtifacts;
          # We deactivate the tests performed with Cargo (`cargo test`) as
          # they will fail because we need Polonius
          doCheck = false;
          buildPhase = ''
            # Run the tests for Charon
            DEST=$out CHARON="${charon}/bin/charon" make charon-tests

            # Nix doesn't run the cargo tests, so run them by hand
            make cargo-tests
          '';
          dontInstall = true;
        };

        # Runs charon on the whole rustc ui test suite. This returns the tests
        # directory with a bunch of `<file>.rs.charon-output` files that store
        # the charon output when it failed. It also adds a `charon-results`
        # file that records `success|expected-failure|failure|panic|timeout`
        # for each file we processed.
        rustc-tests = let
          analyze_test_file = pkgs.writeScript "charon-analyze-test-file" ''
            #!${pkgs.bash}/bin/bash
            FILE="$1"
            echo -n "$FILE: "

            has_magic_comment() {
              # Checks for `// magic-comment` and `//@ magic-comment` instructions in files.
              grep -q "^// \?@\? \?$1:" "$2"
            }

            ${pkgs.coreutils}/bin/timeout 5s ${charon}/bin/charon --no-cargo --input "$FILE" --no-serialize > "$FILE.charon-output" 2>&1
            status=$?
            if [ $status -eq 124 ]; then
                result=timeout
            elif has_magic_comment 'aux-build' "$FILE" \
              || has_magic_comment 'compile-flags' "$FILE" \
              || has_magic_comment 'revisions' "$FILE" \
              || has_magic_comment 'known-bug' "$FILE" \
              || has_magic_comment 'edition' "$FILE"; then
                # We can't handle these for now
                result=unsupported-build-settings
            elif [ $status -eq 101 ]; then
                result=panic
            elif [ $status -eq 0 ]; then
                result=success
            elif [ -f ${"$"}{FILE%.rs}.stderr ]; then
                # This is a test that should fail
                result=expected-failure
            else
                result=failure
            fi

            # Only keep the informative outputs.
            if [[ $result != "panic" ]] && [[ $result != "failure" ]]; then
                rm "$FILE.charon-output"
            fi

            echo $result
          '';
          run_ui_tests = pkgs.writeScript "charon-analyze-test-file" ''
            PARALLEL="${pkgs.parallel}/bin/parallel"
            PV="${pkgs.pv}/bin/pv"
            FD="${pkgs.fd}/bin/fd"

            SIZE="$($FD -e rs | wc -l)"
            echo "Running $SIZE tests..."
            $FD -e rs \
                | $PARALLEL ${analyze_test_file} \
                | $PV -l -s "$SIZE" \
                > charon-results
          '';
        in pkgs.runCommand "charon-rustc-tests" {
          src = pkgs.fetchFromGitHub {
            owner = "rust-lang";
            repo = "rust";
            # The commit that corresponds to our nightly-2023-06-02 pin.
            rev = "d59363ad0b6391b7fc5bbb02c9ccf9300eef3753";
            sha256 = "sha256-fpPMSzKc/Dd1nXAX7RocM/p22zuFoWtIL6mVw7XTBDo=";
          };
          buildInputs = [ rustToolchain ];
        } ''
          mkdir $out
          cp -r $src/tests/ui/* $out
          chmod -R u+w $out
          cd $out
          ${run_ui_tests}
        '';

        ocamlPackages = pkgs.ocamlPackages;
        easy_logging = ocamlPackages.buildDunePackage rec {
          pname = "easy_logging";
          version = "0.8.2";
          src = pkgs.fetchFromGitHub {
            owner = "sapristi";
            repo = "easy_logging";
            rev = "v${version}";
            sha256 = "sha256-Xy6Rfef7r2K8DTok7AYa/9m3ZEV07LlUeMQSRayLBco=";
          };
          buildInputs = [ ocamlPackages.calendar ];
        };
        charon-name_matcher_parser =
          ocamlPackages.buildDunePackage {
            pname = "name_matcher_parser";
            version = "0.1.0";
            duneVersion = "3";
            nativeBuildInputs = with ocamlPackages; [
              menhir
            ];
            propagatedBuildInputs = with ocamlPackages; [
              ppx_deriving
              visitors
              zarith
              menhirLib
            ];
            src = ./charon-ml;
          };
        mk-charon-ml = doCheck:
          ocamlPackages.buildDunePackage {
            pname = "charon";
            version = "0.1.0";
            duneVersion = "3";
            OCAMLPARAM="_,warn-error=+A"; # Turn all warnings into errors.
            preCheck = if doCheck then ''
              mkdir -p tests/serialized
              cp ${tests}/llbc/* tests/serialized
              cp ${tests-polonius}/llbc/* tests/serialized
            '' else
              "";
            propagatedBuildInputs = with ocamlPackages; [
              ppx_deriving
              visitors
              easy_logging
              zarith
              yojson
              calendar
              charon-name_matcher_parser
            ];
            src = ./charon-ml;
            inherit doCheck;
          };
        charon-ml = mk-charon-ml false;
        charon-ml-tests = mk-charon-ml true;

        charon-ml-check-fmt = pkgs.stdenv.mkDerivation {
          name = "charon-ml-check-fmt";
          src = ./charon-ml;
          buildInputs = [
            ocamlPackages.dune_3
            ocamlPackages.ocaml
            ocamlPackages.ocamlformat
          ];
          buildPhase = ''
            if ! dune build @fmt; then
              echo 'ERROR: Ocaml code is not formatted. Run `make format` to format the project files'.
              exit 1
            fi
          '';
          installPhase = "touch $out";
        };
      in {
        packages = {
          inherit charon charon-ml rustc-tests;
          default = charon;
        };
        devShells.default = pkgs.mkShell {
          # Tell charon that the right toolchain is in PATH. It is added to PATH by the `charon` in `inputsFrom`.
          CHARON_TOOLCHAIN_IS_IN_PATH = 1;

          packages = [
            pkgs.ocamlPackages.ocaml
            pkgs.ocamlPackages.ocamlformat
            pkgs.ocamlPackages.menhir
            pkgs.ocamlPackages.odoc
          ];

          inputsFrom = [
            self.packages.${system}.charon
            self.packages.${system}.charon-ml
          ];
        };
        checks = { inherit tests tests-polonius charon-ml-tests charon-check-fmt charon-ml-check-fmt; };
      });
}
