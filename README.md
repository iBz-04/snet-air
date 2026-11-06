<div align="center">
   <h1>SNET-AIR</h1>
</div>

<p align="center">
  <img src="https://res.cloudinary.com/unyt8jsx/image/upload/v1793968963/antinet-squircle-logo-black-on-white-1024.png" alt="snet-air Logo" width="200"/>
</p>

<div align="center">
<p><b>A local model reads your files. A cloud model does the thinking. Your files stay on your machine.</b></p>
</div>

## Quick start

Needs [Rust](https://rustup.rs), a cloud API key and a local model.

**1. Install** with [Ollama](https://ollama.com):

```bash
cargo install --locked --git https://github.com/iBz-04/snet-air snet-command
ollama pull qwen3:4b
```

or with quaynor (no server):

```bash
cargo install --locked --git https://github.com/iBz-04/snet-air snet-command --features gguf
curl -L -o qwen3-4b.gguf https://huggingface.co/bartowski/Qwen_Qwen3-4B-GGUF/resolve/main/Qwen_Qwen3-4B-Q4_K_M.gguf
```

**2. Run `snet`.** It asks for your cloud, key and local model, then takes questions. Ask anything; attach files with `@` and only the local model reads them:

```
❯ What is TCP slow start?
❯ Who signed the contract? @contract.txt
```

For scripts: `snet ask "What is TCP slow start?"` or `snet ask "Who signed the contract?" contract.txt`

| Cloud | Flag | Key |
|---|---|---|
| Anthropic (default) | `--cloud anthropic:<model>` | `ANTHROPIC_API_KEY` |
| Melious | `--cloud melious:<model>` | `MELIOUS_API_KEY` |
| OpenAI | `--cloud openai:<model>` | `OPENAI_API_KEY` |
| Any OpenAI-compatible server | `--cloud openai-compatible:<model> --cloud-url <url>` | `SNET_CLOUD_API_KEY` |

| Option | Use |
|---|---|
| `--fanout` | Large files |
| `--help` | All options |

## Privacy

- Your files are read only by the local model. Questions without files go to the cloud model directly.
- Personal data is pseudonymised before anything is sent.
- Everything sent is logged in `~/.snet/transcripts/`.

## Licence

MPL-2.0
