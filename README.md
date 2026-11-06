<div align="center">
   <h1>SNET-AIR</h1>
</div>

<p align="center">
  <img src="https://res.cloudinary.com/unyt8jsx/image/upload/f_auto,q_auto/antinet-logo-black-on-white-1024" alt="snet-air Logo" width="200"/>
</p>

<div align="center">
<p><b>A local model and a cloud model solving tasks together, without your files leaving your machine.</b></p>
</div>

A small model on your machine reads your files. A large model in the cloud plans the work, asks the questions and writes the answer from what the local model reports. Your files never leave your machine in bulk, and most tokens are read locally.

## Choose your local engine

`snet` runs the local model on your machine with either of two engines. Pick one:

| | quaynor | Ollama |
|---|---|---|
| What it is | The [quaynor](https://github.com/iBz-04/quaynor) engine built into `snet`, running a GGUF file in the same process | A separate [Ollama](https://ollama.com) server on your machine |
| What you need | A GGUF model file | Ollama running, with a model pulled |
| Server | None | `ollama serve` |
| Choose it with | `--local gguf:<file>` | `--local ollama:<model>` (the default) |

Both run fully on your machine, and both get the same checks, limits and transcript.

## Install and run

You need Rust 1.98 or newer and an API key for a cloud model.

### With quaynor

```bash
cargo install --locked --git https://github.com/iBz-04/snet-air snet-command --features gguf
export ANTHROPIC_API_KEY=sk-...
snet ask --local gguf:./Qwen_Qwen3-4B-Q4_K_M.gguf "Who signed the contract and when?" contract.txt
```

The `gguf` feature builds llama.cpp, so it needs CMake and a C++ compiler (and the Vulkan SDK on Linux and Windows). The local model's answers are forced into the expected JSON shape by a grammar, and the file's SHA-256 is recorded as the model's identity. Download GGUF files yourself; `snet` fetches nothing. Set `SNET_LOCAL=gguf:<file>` to make it your default.

### With Ollama

```bash
cargo install --locked --git https://github.com/iBz-04/snet-air snet-command
ollama pull qwen3:4b
export ANTHROPIC_API_KEY=sk-...
snet ask "Who signed the contract and when?" contract.txt
```

## How it works

`snet ask` runs one of two protocols:

- **Conversation** (default): the cloud model asks the local model one question per round, for up to four rounds, then answers. Good for a few files up to 64 KiB each.
- **Fan-out** (`--fanout`): the cloud model writes a plan of small jobs. The local model runs each job on every chunk of every file, in parallel. The results are filtered locally, and the cloud model combines them or plans another round. Good for large files and logs up to 1 MiB each.

Every answer cites the passages it rests on, quoted from your files, so you can check it:

```
The inner seal of pump 3 was replaced on day 7.

[C1] shift-log.txt, bytes 412..466
    Day 7: the inner seal of pump 3 was replaced.
```

## What leaves your machine

- Only the local model reads your files. The cloud model gets your question, its own earlier messages and the local model's short reports.
- Before anything is sent, personal data such as email addresses, phone numbers, IBANs, card numbers, IP and MAC addresses is replaced with pseudonyms like `⟦EMAIL_1⟧`. The mapping stays in memory on your machine, and the answer is restored locally before it is printed. To protect names or your own identifiers too, write a policy file (see below).
- A local report that copies a quote or spells out a protected value is refused, not sent.
- Every request is written to a transcript before it is sent, exactly as sent and without the API key: `~/.snet/transcripts/<task>.jsonl`. Its path is printed after each answer. The pseudonym mapping is never written there.
- The cloud's replies are checked against a strict schema. They are data, never code: nothing the cloud writes is run, opened or fetched.
- The local model runs on your machine: inside `snet` through quaynor, or in Ollama, and `snet` refuses an `OLLAMA_HOST` that points anywhere else.
- No telemetry and no update checks.

## Options

```
snet ask [options] <question> <file>...

  --fanout                  split large files into chunks and run small jobs on each
  --cloud <provider:model>  anthropic:<model>, openai:<model> or openai-compatible:<model>
  --cloud-url <url>         the https URL of an openai-compatible server
  --local <gguf:file>       a GGUF file run by quaynor (needs --features gguf),
          <ollama:model>    or an Ollama model (default ollama:qwen3:4b)
  --context <tokens>        the local model's context window (default 8192)
  --effort <level>          how hard the cloud model thinks (default low)
  --rounds <n>              conversation rounds, 2 to 8 (default 4)
  --plan-rounds <n>         fan-out planning rounds, 1 to 4 (default 2)
  --policy <file>           a pseudonymisation policy file
```

| Variable | Meaning |
|---|---|
| `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `SNET_CLOUD_API_KEY` | The key for `anthropic`, `openai` or `openai-compatible` |
| `SNET_CLOUD` | The default cloud model, such as `openai:gpt-x` (default `anthropic:claude-sonnet-5-5`) |
| `SNET_CLOUD_URL` | The URL of an `openai-compatible` server |
| `SNET_LOCAL` | The default local model, such as `ollama:llama3.2:3b` or `gguf:/models/small.gguf` |
| `OLLAMA_HOST` | Where Ollama listens (default `127.0.0.1:11434`) |
| `SNET_HOME` | Where transcripts go (default `~/.snet`) |

Exit codes: `0` answer, `1` no answer (not found or the task failed), `2` usage, `3` configuration, `4` other failure.

### Limits

Every task stops at 12 cloud requests, 2 MiB sent, 400 000 cloud input tokens and 64 000 cloud output tokens. A conversation stops after 10 minutes and a fan-out after an hour. The limits are checked before each request is sent.

### Policy files

The built-in policy runs every detector. A policy file can also replace names from dictionaries and values that match your own patterns:

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

A dictionary is a UTF-8 file with one term per line, named by its absolute path.

## Development

```bash
cd engine
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p snet-rules -- ..
```

The tests that need a real Ollama or cloud key are ignored by default. Run them with `cargo test -- --ignored`. The GGUF backend builds and tests with `--features gguf`; its live test reads the model path from `SNET_LIVE_GGUF`.

| Crate | Role |
|---|---|
| `snet-command` | The `snet` command |
| `snet-converse` | The conversation protocol |
| `snet-fanout` | The fan-out protocol |
| `snet-protocol` | What the two protocols share |
| `snet-cloud` | One recorded, budgeted, schema-checked request to a cloud model |
| `snet-local` | The local model, through Ollama or quaynor |
| `snet-pseudonym` | Pseudonymisation and restoration |
| `snet-transcript` | The per-task transcript |
| `snet-wire` | Shared HTTP and JSON pieces |
| `snet-rules` | Repository checks |
| `snet-testkit` | Test servers |

## Licence

MPL-2.0. See [LICENSE](LICENSE).
