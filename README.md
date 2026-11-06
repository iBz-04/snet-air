<div align="center">
   <h1>SNET-AIR</h1>
</div>

<p align="center">
  <img src="https://res.cloudinary.com/unyt8jsx/image/upload/f_auto,q_auto/antinet-logo-black-on-white-1024" alt="snet-air Logo" width="200"/>
</p>

<div align="center">
<p><b>A local model reads your files. A cloud model does the thinking. Your files stay on your machine.</b></p>
</div>

## Quick start

Needs [Rust](https://rustup.rs) and an [Anthropic API key](https://console.anthropic.com).

**With quaynor** (no server):

```bash
cargo install --locked --git https://github.com/iBz-04/snet-air snet-command --features gguf
curl -L -o qwen3-4b.gguf https://huggingface.co/bartowski/Qwen_Qwen3-4B-GGUF/resolve/main/Qwen_Qwen3-4B-Q4_K_M.gguf
export ANTHROPIC_API_KEY=sk-ant-...
snet ask --local gguf:qwen3-4b.gguf "Who signed the contract?" contract.txt
```

**With [Ollama](https://ollama.com):**

```bash
cargo install --locked --git https://github.com/iBz-04/snet-air snet-command
ollama pull qwen3:4b
export ANTHROPIC_API_KEY=sk-ant-...
snet ask "Who signed the contract?" contract.txt
```

Other clouds: `--cloud melious:<model>` with `MELIOUS_API_KEY`, `--cloud openai:<model>` with `OPENAI_API_KEY`, or any OpenAI-compatible server with `--cloud openai-compatible:<model> --cloud-url <url>`.

Use `--fanout` for large files. Run `snet ask --help` for all options.

## Privacy

- Your files are read only by the local model.
- Personal data is pseudonymised before anything is sent.
- Everything sent is logged in `~/.snet/transcripts/`.

## Licence

MPL-2.0
