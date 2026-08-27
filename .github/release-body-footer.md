<!-- Appended verbatim to every CLI GitHub Release body by
     .github/workflows/release-cli.yml. The per-release notes come from
     CHANGELOG.md via scripts/changelog-section.sh and are prepended above
     this file, so keep only STANDING content here -- install steps,
     platforms, links. Anything release-specific belongs in CHANGELOG.md.

     The heading below was '### What's New' while it was inlined in the
     workflow, which is what it was not: the same fixed bullets shipped on
     every release regardless of what changed. -->
Drop-in LLM security for your applications - Built with Rust + Tree-sitter

### Installation

**One-line install (macOS/Linux)**:
```bash
curl -fsSL https://raw.githubusercontent.com/acebot712/promptguard-cli/main/install.sh | sh
```

**Manual install**:
1. Download the binary for your platform below
2. Make it executable: `chmod +x promptguard-*`
3. Move to PATH: `sudo mv promptguard-* /usr/local/bin/promptguard`
4. Verify: `promptguard --version`

### Checksums

SHA256 checksums are provided for each binary (`.sha256` files).

### About PromptGuard CLI

- ✅ Real AST transformations using Tree-sitter
- ✅ TypeScript, JavaScript, Python support
- ✅ OpenAI, Anthropic, Cohere, HuggingFace providers
- ✅ Automatic backups with safe revert
- ✅ Single 5.3MB static binary

### Supported Platforms

- macOS ARM64 (M1/M2/M3)
- macOS x86_64 (Intel)
- Linux x86_64
- Windows x86_64

---

📖 **Documentation**: https://docs.promptguard.co/cli
🏠 **Homepage**: https://promptguard.co
🐛 **Issues**: https://github.com/acebot712/promptguard-cli/issues
