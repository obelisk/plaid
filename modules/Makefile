# Build the modules, get wasm-opt, optimize everything, and print the module hashes

WASMS := $(wildcard target/wasm32-unknown-unknown/release/*.wasm)
WHOAMI := $(shell whoami)

optimize_darwin:
	@echo "Optimizing modules"
	@curl -s -L https://github.com/WebAssembly/binaryen/releases/download/version_123/binaryen-version_123-arm64-macos.tar.gz | tar xvz - -C /tmp
	# For every wasm file in the target/wasm32-unknown-unknown/release directory
	@$(foreach f, $(WASMS), echo Processing $(f) && \
		/tmp/binaryen-version_123/bin/wasm-opt --strip-dwarf --strip-debug --vacuum --strip -O4 $(f) -o $(f) && \
		echo "Hash: $$(cat $(f) | sha256sum | cut -d' ' -f1)" && \
		echo "----------------------------------------";)
	@echo "Done optimizing modules"

build_darwin: export RUSTFLAGS=--remap-path-prefix=/Users/$(WHOAMI)/=/ -C debuginfo=0
build_darwin: 
	@echo "Building modules"
	@echo "$(RUSTFLAGS)"
	@PATH="/opt/homebrew/opt/llvm/bin:${PATH}" cargo build --release --all

clean:
	@echo "Cleaning modules"
	@cargo clean