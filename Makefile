# PromptGuard CLI - Makefile

.PHONY: help build release install clean test check format cross-compile

help:
	@echo "PromptGuard CLI - Build Targets"
	@echo ""
	@echo "Development:"
	@echo "  make build          Build debug binary"
	@echo "  make release        Build optimized release binary"
	@echo "  make install        Install binary to ~/.cargo/bin"
	@echo "  make test           Run tests"
	@echo "  make check          Check for errors"
	@echo "  make format         Format code"
	@echo ""
	@echo "Distribution:"
	@echo "  make cross-compile  Build for all platforms"
	@echo "  make clean          Clean build artifacts"

build:
	cargo build

release:
	cargo build --release
	@echo ""
	@echo "✓ Release binary built:"
	@ls -lh target/release/promptguard
	@echo ""
	@echo "To install: make install"

install:
	cargo install --path .
	@echo ""
	@echo "✓ Installed to ~/.cargo/bin/promptguard"
	@which promptguard

clean:
	cargo clean
	rm -rf target/
	@echo "✓ Cleaned build artifacts"

test:
	cargo test

check:
	cargo check
	cargo clippy -- -D warnings

format:
	cargo fmt

# Cross-compile for all platforms
cross-compile:
	@echo "Building for all platforms..."
	@mkdir -p dist

	@echo "→ macOS (Intel)"
	cargo build --release --target x86_64-apple-darwin
	cp target/x86_64-apple-darwin/release/promptguard dist/promptguard-darwin-amd64

	@echo "→ macOS (Apple Silicon)"
	cargo build --release --target aarch64-apple-darwin
	cp target/aarch64-apple-darwin/release/promptguard dist/promptguard-darwin-arm64

	@echo "→ Linux (AMD64)"
	cargo build --release --target x86_64-unknown-linux-gnu
	cp target/x86_64-unknown-linux-gnu/release/promptguard dist/promptguard-linux-amd64

	@echo "→ Linux (ARM64)"
	cargo build --release --target aarch64-unknown-linux-gnu
	cp target/aarch64-unknown-linux-gnu/release/promptguard dist/promptguard-linux-arm64

	@echo "→ Windows (AMD64)"
	cargo build --release --target x86_64-pc-windows-msvc
	cp target/x86_64-pc-windows-msvc/release/promptguard.exe dist/promptguard-windows-amd64.exe

	@echo ""
	@echo "✓ Binaries built:"
	@ls -lh dist/
	@echo ""
	@echo "Upload to GitHub releases:"
	@echo "  gh release create v2.0.0 dist/* --title 'v2.0.0' --notes 'Release notes'"

# Quick development loop
dev:
	cargo watch -x 'run -- --help'

# Binary size analysis
size:
	@echo "Binary sizes:"
	@ls -lh target/release/promptguard 2>/dev/null || echo "Build release first: make release"
	@echo ""
	@echo "Stripped size:"
	@strip target/release/promptguard 2>/dev/null && ls -lh target/release/promptguard || echo "N/A"
