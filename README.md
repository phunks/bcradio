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
