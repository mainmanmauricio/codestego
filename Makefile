.PHONY: test deb rpm packages

test:
	cargo test

deb:
	@command -v cargo-deb >/dev/null || { echo "install: cargo install cargo-deb"; exit 1; }
	cargo deb

rpm:
	@command -v cargo-generate-rpm >/dev/null || { echo "install: cargo install cargo-generate-rpm"; exit 1; }
	cargo build --release
	strip -s target/release/codestego || true
	cargo generate-rpm

packages: deb rpm
