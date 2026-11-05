# snet-air

A small model on your machine reads your files. A large model in the cloud plans the work, asks the questions and writes the answer from what the local model reports. Your files never leave the machine in bulk, and most tokens are read locally.

```
ollama pull qwen3:4b
export ANTHROPIC_API_KEY=sk-...
snet ask "Who signed the contract and when?" contract.txt
```

## Install

Build from source with Rust 1.98 or newer:

```bash
cargo install --locked --git https://github.com/iBz-04/snet-air snet-command
```

You also need [Ollama](https://ollama.com) running on this machine with a model pulled, and an API key for a cloud model.

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
- The local model must run on this machine. `snet` refuses an `OLLAMA_HOST` that points anywhere else.
- No telemetry and no update checks.

## Options

```
snet ask [options] <question> <file>...

  --fanout                  split large files into chunks and run small jobs on each
  --cloud <provider:model>  anthropic:<model>, openai:<model> or openai-compatible:<model>
  --cloud-url <url>         the https URL of an openai-compatible server
  --local <ollama:model>    the local model (default ollama:qwen3:4b)
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
| `SNET_LOCAL` | The default local model, such as `ollama:llama3.2:3b` |
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

The tests that need a real Ollama or cloud key are ignored by default. Run them with `cargo test -- --ignored`.

| Crate | Role |
|---|---|
| `snet-command` | The `snet` command |
| `snet-converse` | The conversation protocol |
| `snet-fanout` | The fan-out protocol |
| `snet-protocol` | What the two protocols share |
| `snet-cloud` | One recorded, budgeted, schema-checked request to a cloud model |
| `snet-local` | The local model through Ollama |
| `snet-pseudonym` | Pseudonymisation and restoration |
| `snet-transcript` | The per-task transcript |
| `snet-wire` | Shared HTTP and JSON pieces |
| `snet-rules` | Repository checks |
| `snet-testkit` | Test servers |

## Licence

MPL-2.0. See [LICENSE](LICENSE).
