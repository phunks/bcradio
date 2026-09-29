# BCRADIO
A command line music player for https://bandcamp.com, written in Rust.


![Screenshot](./.github/images/bcradio_play_osx.png)

## Usage

```
Usage: bcradio [OPTIONS] [COMMAND]

Commands:
  ai-key     Manage the AI API key in the OS credential store
  ai-config  Configure the OpenAI-compatible chat API
  help       Print this message or the help of the given subcommand(s)

Options:
  -v, --verbose... verbose log. check `$env:TEMP`
      --no-ssl-verify  disable SSL verification
  -i, --img-width <IMG_WIDTH>  image size [default: 30]
  -h, --help       Print help
  -V, --version    Print version
  
[Key]                [Description]
 0-9                  adjust volume
 h                    help
 H                    playback history
 I                    generate AI playlist from a description
 i                    play info
 s                    free word search
 f                    favorite search
 n                    play next
 m                    menu
 l                    playlist (up:k, down:j, select:enter key)
 p                    play/pause
 Q                    finish the current song, then exit
 Esc                  cancel a pending Q and resume normal playback
 Ctrl+C               exit immediately
```

## Proxy configuration

All HTTP requests (Bandcamp and AI) use reqwest's default proxy settings:
`HTTP_PROXY`, `HTTPS_PROXY`, and `ALL_PROXY` (or their lowercase variants).
Use `NO_PROXY` (or `no_proxy`) to bypass the proxy for specific hosts, such as
a LiteLLM instance on the local network. Match the hostname used in the AI URL:

```
export ALL_PROXY=[socks5h|http(s)]://proxy.internal:1080
export NO_PROXY=litellm.internal,localhost,127.0.0.1
bcradio
```

The former `--proxy` option is no longer supported.

## AI API key storage

AI API keys are stored in the operating system's secure keychain / credential store,
not in a bcradio configuration file. Manage the key without exposing it in shell history:

```
bcradio ai-key set
bcradio ai-key status
bcradio ai-key delete
```

On macOS this uses Keychain, on Windows Credential Manager, and on Linux the
Secret Service (a running session service is required). If the store is unavailable,
bcradio reports an error rather than saving the key as plaintext. These commands
only manage the credential; use `I` during playback to generate a playlist.

## OpenAI-compatible API

Set the base URL (including `/v1` if your provider uses it) and model:

```
bcradio ai-config set --url https://api.openai.com/v1 --model YOUR_MODEL
bcradio ai-config set --url http://litellm.internal:4000/v1 --model YOUR_MODEL
bcradio ai-config show
```

The client sends chat completions to `<base URL>/chat/completions` with the
API key from the OS credential store. The non-secret URL and model are saved
to `~/Library/Application Support/bcradio/ai.json` on macOS,
`${XDG_CONFIG_HOME:-~/.config}/bcradio/ai.json` on Linux, or
`%APPDATA%\bcradio\ai.json` on Windows. No configuration directory is created
until `ai-config set` is used. HTTP is also supported for LiteLLM on another
host; over HTTP, requests and the API key are transmitted without encryption.

### AI usage cost (example)

One observed AI playlist request using `openai/gpt-6-sol`:

| Metric | Value |
| --- |------------------------------------------------:|
| Tokens | 714 (116 prompt tokens + 598 completion tokens) |
| Reasoning tokens | 531 |
| Cost | $0.00621200 |
| AI API response time | 14.217 s |

In other observed requests, AI responses took roughly 14 to 22 seconds.

Even for a short description, the AI request includes playlist instructions
and the artist names and titles of up to 30 songs played in the last 60 minutes.
On subsequent requests it also includes previously used search terms. The AI
is asked to avoid those songs and terms to reduce repeats, though this is not
guaranteed. These details also add to the prompt token count.

For each AI request, the model suggests 6 to 8 Bandcamp search terms rather
than tracks. bcradio searches for playable songs and adds up to 3 per term,
with a maximum of 15 tracks per request (not an average). Fewer tracks may
be added if searches fail, find no playable songs, or return duplicates.
If an exact artist-and-track search finds nothing, bcradio retries with a
shorter search term (such as `Portico Quartet Ruins` → `Portico Quartet`).
Tracks found through this fallback are only used if the artist matches the
shorter term.
Genre and mood are best-effort: bcradio does not analyze the audio, so a jazz
playlist may occasionally include a track that sounds more like hip-hop or
dance music.
Including the subsequent Bandcamp searches, creating a playlist typically
takes around 30 seconds; this can vary with AI response time and Bandcamp load.
When fewer than 2 tracks remain queued in an AI playlist, bcradio requests
more suggestions automatically; each additional AI request may incur a charge.

The cost above is for one observed request, not a fixed price per playlist.
Actual cost and response time depend on the provider, model, and request size;
check your provider's pricing and usage for current charges.

If you find a song you love, please support the artist on Bandcamp!

### ⚠ About building and running on Linux

This program uses [rustaudio/cpal](https://github.com/rustaudio/cpal) lib to play audio, which requires ALSA development files on Linux.

In order to build and run this program on Linux, you need to install：

- `libasound2-dev` on Debian / Ubuntu
- `alsa-lib-devel` on Fedora
- `alsa-lib`       on Alpine

If AAAA records are returned slowly in the information screen, add "options single-request-reopen" to resolve.conf. It is not my fault.

### ⚠ About running on Windows

The program can also play audio using the [ASIO4ALL](https://asio4all.org) driver instead of WASAPI.

Note: Windows is a pain to boot up, so I haven't done much software testing. Well, it will work.


## License
The source code is licensed MIT. The website content is licensed CC BY 4.0,see LICENSE.

## Special Thanks

- [JasonWei512 / code-radio-cli](https://github.com/JasonWei512/code-radio-cli)
