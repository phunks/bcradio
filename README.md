# BCRADIO
A command line music player for https://bandcamp.com, written in Rust.


![Screenshot](./.github/images/bcradio_play_osx.png)
![Screenshot](./.github/images/bcradio_info_osx.png)

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

## Usage

```
Usage: bcradio

Options:
  -v, --verbose... verbose log
      --no-ssl-verify  disable SSL verification
  -i, --img-width <IMG_WIDTH>  image size [default: 30]
  -h, --help       Print help
  -V, --version    Print version
  
[Key]                [Description]
 0-9                  adjust volume
 h                    help
 i                    play info
 s                    free word search
 f                    favorite search
 n                    play next
 m                    menu
 l                    playlist (up:k, down:j, select:enter key)
 p                    play/pause
 Q                    graceful kill
 Ctrl+C               exit
```
## License
The source code is licensed MIT. The website content is licensed CC BY 4.0,see LICENSE.

## Special Thanks

- [JasonWei512 / code-radio-cli](https://github.com/JasonWei512/code-radio-cli)
