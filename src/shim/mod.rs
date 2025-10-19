/// Runtime shim generation and injection
///
/// This module provides the core functionality for `PromptGuard`'s runtime
/// interception system, which ensures 100% coverage of LLM SDK calls by
/// monkey-patching (Python) or wrapping (TypeScript/JavaScript) SDK constructors.
///
/// ## Architecture
///
/// The runtime shim system consists of three components:
///
/// 1. **Templates** - Embedded shim code templates for each language and provider
/// 2. **Generator** - Creates shim files from templates with configuration injected
/// 3. **Injector** - Detects entry points and injects shim imports
///
/// ## Usage
///
/// ```no_run
/// use promptguard::shim::{ShimGenerator, ShimInjector};
/// use promptguard::types::{Provider, Language};
///
/// // Generate shim files
/// let generator = ShimGenerator::new(
///     "/path/to/project",
///     "https://api.promptguard.co/v1".to_string(),
///     "PROMPTGUARD_API_KEY".to_string(),
///     vec![Provider::OpenAI, Provider::Anthropic],
/// );
///
/// generator.generate_shims(&[Language::Python]).unwrap();
///
/// // Inject into entry points
/// let injector = ShimInjector::new("/path/to/project");
/// let injected = injector.inject_shims(Language::Python).unwrap();
/// ```
///
/// ## How It Works
///
/// ### Python
///
/// 1. Generate `.promptguard/promptguard_shim.py` with monkey-patching code
/// 2. Detect entry points (main.py, files with `if __name__ == "__main__":`, etc.)
/// 3. Inject `import promptguard_shim` at the top of each entry point
/// 4. When app starts, shim patches SDK constructors before any SDK imports
///
/// ### TypeScript/JavaScript
///
/// 1. Generate `.promptguard/promptguard-shim.ts` with wrapper classes
/// 2. Detect entry points (package.json main, index.ts, etc.)
/// 3. Recommend tsconfig.json path aliases or direct imports
/// 4. App imports from shim instead of original SDK package
///
/// ## Benefits
///
/// - **100% Coverage**: Catches all SDK usage, even dynamic initialization
/// - **Zero Config**: Works without understanding the codebase
/// - **Safe**: Can be disabled without breaking the app
/// - **Transparent**: Developers see exactly what's being intercepted
pub mod generator;
pub mod injector;
pub mod templates;

pub use generator::ShimGenerator;
pub use injector::ShimInjector;
