# Model Lab

Model Lab is where NEXUS finds, runs, measures and compares language models. It is local-first: everything works with a runtime on your own machine and no API key, and nothing is sent anywhere else unless you add a remote connection and confirm each use of it.

Open it from the sidebar (**Model Lab**) or the command palette (`Ctrl/Cmd+K` → *Open Model Lab*). It has six tabs.

## Quick start

1. **Runtimes** shows what is installed and answering on this machine. If Ollama is running, click **Add as connection**. If it is installed but not running, start it from the same card.
2. **Library** now lists the models that runtime has. Select one to see what it reports about itself.
3. Click **Open in Playground**, type a prompt and press **Run**.
4. To measure speed, click **Benchmark** on the model, or open **Benchmarks** and choose several models.

If no runtime is installed, each runtime card has **Setup instructions** with the commands to copy. NEXUS never installs software or runs those commands for you.

## Supported runtimes

| Runtime | Connection type | What NEXUS can read from it | Start / stop from NEXUS |
| --- | --- | --- | --- |
| Ollama | Ollama | Models with family, parameters, quantization, context length, capabilities, licence, size; models in memory and their memory use; load, prompt and generation times per request | Yes (`ollama serve` on 127.0.0.1, its own model store) |
| llama.cpp server (`llama-server`) | llama.cpp server | The served model with parameter count, training context, size; the running context size; prompt and generation timings | Yes, for a `.gguf` file in the model folder |
| LM Studio, vLLM, LocalAI and other OpenAI-compatible servers | OpenAI-compatible | Model names only; token counts when the server reports usage | No (start them yourself) |
| Anthropic (remote, optional) | Anthropic | Model names, context window, output limit and capabilities from the Models API | — |

Detection looks only at the usual install locations and the default local ports (Ollama 11434, llama.cpp 8080 and 9931, LM Studio 1234) plus addresses of your saved connections. A server on another port is added by hand in **Providers**.

## Library

Every model your enabled connections report, and every `.gguf` file in the model folder. Search covers names, families, quantization and capabilities; filter by source and status; sort by name, size, source, status, parameters or context; star favourites.

**Status:** *installed* (in a local runtime), *file on disk* (a GGUF file no runtime is serving yet), *remote*, *connection disabled*, *runtime offline* (the runtime did not answer; you see its last known list), *no longer listed* (a favourite the runtime dropped).

**What the details mean.** Each value comes from the runtime or from the model file's own header. If a runtime does not report a value, it is shown as a dash — NEXUS does not fill in specifications from elsewhere. The detail view says where its information came from (for example *Reported by Ollama (/api/show)* or *Read from the GGUF file header*).

**Removing a model.** NEXUS can remove models from a local Ollama and model files it downloaded itself. It asks for confirmation and shows the size. It never removes files you put in the folder yourself.

### Adding models

Nothing is downloaded until you press a download button.

- **Ollama pull:** type a model name (`name:tag`). Ollama downloads, resumes and verifies the layers; NEXUS shows progress and can cancel. Cancelling keeps what Ollama has, so starting again continues.
- **Hugging Face:** searches public repositories that contain GGUF files (your search text is sent to huggingface.co) and lists each file's size and published SHA-256. Gated or private repositories cannot be downloaded; NEXUS has no Hugging Face login.
- **Direct URL:** any HTTPS address of a `.gguf` file, with an optional SHA-256.

### Storage

**Library → Storage** shows the model folder, free and total disk space, and the files in it. **Change folder…** opens a folder picker; files already downloaded stay where they are.

How a file download works:

1. Refused if the name is not a plain `.gguf` file name, if the file is already in the folder, or if it is larger than the free space.
2. Written to `<name>.gguf.part`. If the connection drops, **Resume** continues from where it stopped (HTTP range request).
3. Hashed while it arrives. With a published checksum it must match, or it is discarded; without one, the computed SHA-256 is recorded and the download is marked *no published checksum*.
4. Checked to be a real GGUF file (an HTML error page saved as `.gguf` is rejected).
5. Renamed into place only then.

Redirects may only lead to HTTPS addresses.

## Runtimes

Each card shows whether the runtime is installed (and where), which addresses answer, its version and model count, and — for Ollama — the models currently in memory, with **Unload** to free that memory now.

**Starting a server from NEXUS** runs the runtime's own program with a fixed argument list and no shell, bound to 127.0.0.1 only, without your environment's credentials. Its output is shown under **Output**. NEXUS stops the servers it started when it closes, when it receives a termination signal (macOS and Linux), and — if it was killed outright — at its next launch.

## Playground

Choose a connection and model, enter a prompt, press **Run** (or Enter). Output streams in; **Stop** ends generation at once and keeps what was produced.

**Generation settings** show only what the selected runtime accepts. An empty field is not sent, so the runtime's own default applies. Claude models reject temperature, top-p and top-k, so those are not offered for Anthropic.

**Measurements** under each reply:

| Shown | Source |
| --- | --- |
| first token, total | Measured by NEXUS from sending the request |
| in, out (tokens) | Reported by the runtime; a dash if it reports none |
| speed | The runtime's own generation timing where it has one; otherwise *(derived)*: reported output tokens divided by the time after the first token |
| load, prompt | Reported by the runtime (Ollama; llama.cpp for prompt time) |

Thinking-capable models show their reasoning collapsed above the answer.

**Sessions** are saved on this machine with their prompts, outputs, settings and measurements. A saved session is updated after every run.

## Benchmarks

A benchmark sends the same prompts with the same settings to each chosen model, one request at a time, several times each.

- **Warm up first** sends one unmeasured request per model so loading does not skew the measured runs (load time is still reported).
- **Measure a cold load** asks Ollama to unload each model first.
- After two failures in a row a model is skipped; the others still run.
- Cancel at any time; the request in progress is stopped.

Results show the median with the range and number of runs for first token, speed, total time and prompt processing, plus load time and memory where the runtime reports them. Every benchmark stores the machine (OS, CPU, cores, memory), the runtime and its version, the model's quantization, and the exact settings. **Export** writes JSON or CSV through a save dialog; an unknown value is an empty CSV cell, never a zero.

Results describe one machine with one configuration. Do not compare them with results from other hardware, other settings, another quantization, or a remote service (whose speed depends on the network and its load).

## Compare

Pick up to four models. The page shows:

- **Specifications** side by side, as reported.
- **Measured performance** from one saved benchmark, so all figures share the same prompts, settings and machine. A model is marked *best* in a row only when its whole range of runs beats every other model's range; overlapping ranges are treated as variation. There is no combined score and output quality is not rated.
- **Same prompt, side by side:** runs one prompt against each model in turn, with your default settings.

## Providers and defaults

**Connections:** add, edit, **Test** (shows what answered and how fast), **Disable** (kept, never contacted) and remove. API keys go to the operating system's keychain; the database only records that a key exists. Keys are sent only to their own connection and are removed from error messages.

**Defaults:** a default connection and model, default generation settings and the model folder. They apply to:

- the Playground and comparison runs;
- **Intelligence → Ask** and the architecture narrative, which start on the default model when it runs on this machine;
- new **Agent** tasks, which start on the default. An agent task stores its connection and model when it is created and never switches to another, remote or not.

## Privacy and safety

- Local connections never send anything off the machine.
- For a remote connection, each screen shows the destination host and needs a ticked confirmation before the first request. Agent tasks, codebase questions and the architecture narrative are also checked in the core, so a remote model receives project content only with that confirmation.
- Model output is only ever displayed as text; it is never run as a command or written to files by the Model Lab.
- See [PRIVACY.md](PRIVACY.md) for everything that can leave the machine.

## Limits

- Memory use is reported only by Ollama. llama.cpp and OpenAI-compatible servers show a dash.
- Model load time is reported by Ollama only.
- OpenAI-compatible endpoints report no specifications, only names.
- NEXUS starts only Ollama and llama.cpp servers. It does not install runtimes, convert models or import GGUF files into Ollama.
- Hugging Face downloads work only for public, non-gated repositories.
- Benchmarks run one request at a time; there is no concurrency or load testing.
- Starting runtimes and the cleanup of left-behind servers were verified on macOS. Windows and Linux use the same code paths but have not been exercised with real runtimes.
