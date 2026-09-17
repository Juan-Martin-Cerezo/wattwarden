.PHONY: all build release test fmt clippy install uninstall clean

all: release

build:
	cargo build --workspace

release:
	cargo build --workspace --release

test:
	cargo test --workspace --all-targets

fmt:
	cargo fmt

clippy:
	cargo clippy --workspace --all-targets -- -D warnings

install: release
	./install.sh

uninstall:
	./uninstall.sh

clean:
	cargo clean
