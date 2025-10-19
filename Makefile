# PromptGuard CLI - Makefile

.PHONY: help build release install uninstall clean test check format lint fmt-check ci cross-compile

help:
	@echo "PromptGuard CLI - Build Targets"
	@echo ""
	@echo "Development:"
	@echo "  make build          Build debug binary"
	@echo "  make release        Build optimized release binary"
	@echo "  make install        Install binary to ~/.cargo/bin"
	@echo "  make uninstall      Uninstall binary from ~/.cargo/bin"
	@echo ""
	@echo "Quality:"
	@echo "  make test           Run tests"
	@echo "  make check          Quick sanity check"
	@echo "  make lint           Run clippy linter"
	@echo "  make format         Format code with rustfmt"
	@echo "  make fmt-check      Check if code is formatted"
	@echo "  make ci             Run all CI checks locally"
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

uninstall:
	@echo "Uninstalling PromptGuard CLI..."
	cargo uninstall promptguard-cli || cargo uninstall promptguard || true
	@echo ""
	@if command -v promptguard >/dev/null 2>&1; then \
		echo "⚠ Warning: promptguard still found in PATH"; \
		echo "Location: $$(which promptguard)"; \
		echo "You may need to remove it manually"; \
	else \
		echo "✓ PromptGuard CLI uninstalled successfully"; \
	fi
	@echo ""
	@if [ -d "$$HOME/.promptguard" ]; then \
		echo "Configuration directory still exists: $$HOME/.promptguard"; \
		echo "To remove it, run: rm -rf $$HOME/.promptguard"; \
	fi

clean:
	cargo clean
	rm -rf target/
	@echo "✓ Cleaned build artifacts"

test:
	@echo "🧪 Running tests..."
	@cargo test

check:
	@echo "🔍 Quick sanity check..."
	@cargo check --all-targets

lint:
	@echo "🔬 Running clippy..."
	@cargo clippy --all-targets --all-features

format:
	@echo "🎨 Formatting code..."
	@cargo fmt

fmt-check:
	@echo "🔍 Checking formatting..."
	@cargo fmt -- --check

# Run the same checks as CI - catch issues before pushing
ci:
	@echo "🚀 Running full CI checks locally..."
	@echo ""
	@$(MAKE) fmt-check
	@echo ""
	@$(MAKE) lint
	@echo ""
	@$(MAKE) test
	@echo ""
	@$(MAKE) build
	@echo ""
	@echo "✅ All CI checks passed! Safe to push."

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
