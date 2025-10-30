# OpenAI Hello World Test Fixture

This fixture provides simple OpenAI examples for testing the PromptGuard CLI.

## Files

- `hello.ts` - TypeScript OpenAI example
- `hello.js` - JavaScript OpenAI example
- `hello.py` - Python OpenAI example
- `.env` - Test environment variables
- `package.json` - Node.js dependencies
- `requirements.txt` - Python dependencies

## Usage with PromptGuard CLI

### Test Scanning

```bash
cd tests/fixtures/openai-hello-world
promptguard scan
```

Expected output: Should detect OpenAI SDK usage in all 3 files

### Test Initialization

```bash
promptguard init --api-key pg_sk_test_xxx --dry-run
```

Expected behavior: Should show transformations for TypeScript, JavaScript, and Python files

### Test Apply

```bash
promptguard init --api-key pg_sk_test_xxx
promptguard apply
```

Expected behavior: Should transform all OpenAI constructor calls to use PromptGuard proxy

### Test Revert

```bash
promptguard revert
```

Expected behavior: Should restore original files from backups

## Expected Detections

The CLI should detect:

- **TypeScript (hello.ts)**: 1 OpenAI instance
- **JavaScript (hello.js)**: 1 OpenAI instance
- **Python (hello.py)**: 1 OpenAI instance
- **Total**: 3 instances across 3 files

## Expected Transformations

### TypeScript/JavaScript
Before:
```typescript
const openai = new OpenAI({
  apiKey: process.env.OPENAI_API_KEY,
});
```

After:
```typescript
const openai = new OpenAI({
  apiKey: process.env.PROMPTGUARD_API_KEY,
  baseURL: "https://api.promptguard.co/api/v1/proxy"
});
```

### Python
Before:
```python
client = OpenAI(
    api_key=os.environ.get("OPENAI_API_KEY"),
)
```

After:
```python
client = OpenAI(
    api_key=os.environ.get("PROMPTGUARD_API_KEY"),
    base_url="https://api.promptguard.co/api/v1/proxy"
)
```
