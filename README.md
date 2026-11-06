<div align="center">
   <h1>SNET-AIR</h1>
</div>

<p align="center">
  <img src="https://res.cloudinary.com/unyt8jsx/image/upload/f_auto,q_auto/antinet-logo-black-on-white-1024" alt="snet-air Logo" width="200"/>
</p>

<div align="center">
<p><b>A local model and a cloud model solving tasks together, without your files leaving your machine.</b></p>
</div>

A small model on your machine reads your files. A large cloud model plans the work, asks the questions and writes the answer from what the local model reports. Your files are never sent; only short, pseudonymised reports are.

## Quick start

You need [Rust](https://rustup.rs) and an [Anthropic API key](https://console.anthropic.com). Then pick one way to run the local model:

**Option A: quaynor** (built into `snet`, no server; needs CMake and a C++ compiler)

```bash
cargo install --locked --git https://github.com/iBz-04/snet-air snet-command --features gguf
curl -L -o qwen3-4b.gguf https://huggingface.co/bartowski/Qwen_Qwen3-4B-GGUF/resolve/main/Qwen_Qwen3-4B-Q4_K_M.gguf
export ANTHROPIC_API_KEY=sk-ant-...
snet ask --local gguf:qwen3-4b.gguf "Who signed the contract and when?" contract.txt
```

**Option B: Ollama** (install [Ollama](https://ollama.com) first)

```bash
cargo install --locked --git https://github.com/iBz-04/snet-air snet-command
ollama pull qwen3:4b
export ANTHROPIC_API_KEY=sk-ant-...
snet ask "Who signed the contract and when?" contract.txt
```

Any GGUF file or Ollama model works; `qwen3:4b` is a good start. The answer cites the lines of your file it rests on:

```
The inner seal of pump 3 was replaced on day 7.

[C1] shift-log.txt, bytes 412..466
    Day 7: the inner seal of pump 3 was replaced.
```

For large files or logs, add `--fanout`: the cloud plans small jobs and the local model runs them on every chunk.

## Privacy

- Only the local model reads your files.
- Emails, phone numbers, IBANs, card numbers, IP and MAC addresses are replaced with pseudonyms before anything is sent, and restored on your machine.
- Everything sent is logged to `~/.snet/transcripts/` so you can check it.
- Cloud replies are checked against a strict schema and never run as code.
- Each task stops at 12 cloud requests, 2 MiB sent and 400 000 input tokens.

## Options

```
snet ask [options] <question> <file>...

  --local <gguf:file|ollama:model>   local model (default ollama:qwen3:4b)
  --cloud <provider:model>           anthropic:, openai: or openai-compatible: (default anthropic:claude-sonnet-5-5)
  --cloud-url <url>                  URL of an openai-compatible server
  --fanout                           chunked mode for large files
  --policy <file>                    pseudonymise names or your own patterns too
```

Keys: `ANTHROPIC_API_KEY`, `OPENAI_API_KEY` or `SNET_CLOUD_API_KEY`. Defaults can be set with `SNET_LOCAL` and `SNET_CLOUD`. Run `snet ask --help` for every option.

A policy file adds names (one per line in a text file) and patterns to protect. Every key must be present:

```toml
dictionaries = [{ category = "PERSON", path = "/home/me/people.txt" }]
patterns = [{ category = "EMPLOYEE_ID", regex = 'E-\d{6}' }]
passthrough = []

[builtin]
EMAIL = true
IBAN = true
PAYMENT_CARD = true
TR_IDENTITY = true
PHONE = true
IP_ADDRESS = true
MAC_ADDRESS = true
```

## Development

```bash
cd engine
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Add `--features gguf` for the quaynor backend. Tests needing a real model or key are ignored by default (`cargo test -- --ignored`).

## Licence

MPL-2.0. See [LICENSE](LICENSE).
