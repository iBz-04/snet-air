<div align="center">
   <h1>SNET-AIR</h1>
</div>

<p align="center">
  <img src="https://res.cloudinary.com/unyt8jsx/image/upload/v1793968963/antinet-squircle-logo-black-on-white-1024.png" alt="snet-air Logo" width="200"/>
</p>

<div align="center">
<p><b>Two models, one answer. Your data stays yours.</b></p>
</div>

snet pairs a small model on your computer with a large model in the cloud. The local model reads everything first. It answers what it can on its own; for the rest, it removes anything private and hands the cloud only the short question that's left. Your files never leave your machine, and the cloud reads far fewer tokens.

## Get started

```bash
curl -fsSL https://github.com/iBz-04/snet-air/raw/main/install.sh | sh
snet pull
snet
```

Set `ANTHROPIC_API_KEY` first, or paste your key when snet asks.

## Why snet

**Private.** Your words reach the cloud only after your own computer has removed what identifies you. Your files never do.

**Lean.** Hellos and simple questions never leave your machine. Hard ones reach the cloud short, so they cost less.

**Open.** `/details` shows exactly what the cloud read. Every request is logged on your machine in `~/.snet/transcripts`.

## Ask about your files

```
❯ Who signed the contract? @contract.txt
```

Only your computer reads the file. The answer cites the lines it rests on.

## Choose your models

| Local | |
|---|---|
| `snet pull` | qwen3:4b, the default · 2.5 GB |
| `snet pull qwen3:8b` | sharper · 5 GB |

| Cloud | Key |
|---|---|
| Anthropic (default) | `ANTHROPIC_API_KEY` |
| Melious | `MELIOUS_API_KEY` |
| OpenAI | `OPENAI_API_KEY` |

Already use Ollama? Skip `snet pull`; snet offers your Ollama models.

## More

- Scripts: `snet ask "What is TCP slow start?"` · all options: `snet ask --help`
- Windows: [download from Releases](https://github.com/iBz-04/snet-air/releases). Linux needs Vulkan drivers.

MPL-2.0
