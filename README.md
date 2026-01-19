# LazyTail - TUI Log Viewer for Rust

A terminal-based log viewer with filtering capabilities, built with Rust and ratatui.

## Features

### Implemented
- **Lazy file reading** - Efficiently handles large log files using indexed line positions
- **TUI interface** - Clean terminal UI with ratatui
- **Line selection** - Navigate through logs with keyboard controls
- **Live filtering** - See results instantly as you type your search string
- **Background filtering** - Non-blocking regex and string matching filters
- **File watching** - Auto-reload when log file is modified (using inotify on Linux)
- **Follow mode** - Auto-scroll to show latest logs as they arrive (like `tail -f`)
- **ANSI color support** - Parses and renders ANSI escape codes in full color
- **Raw view mode** - Display logs in their original format with line numbers
- **Memory efficient** - Viewport-based rendering keeps RAM usage low

### Keyboard Controls

**Navigation:**
- `↑`/`k` - Scroll up one line
- `↓`/`j` - Scroll down one line
- `PgUp` - Scroll up one page
- `PgDn` - Scroll down one page
- `g` - Jump to start (first line)
- `G` - Jump to end (last line)
- `f` - Toggle follow mode (auto-scroll to new logs)

**Filtering:**
- `/` - Enter live filter mode
- Type any text - Results update instantly as you type
- `Backspace` - Delete characters
- `Enter` - Close filter prompt (keeps filter active, shown in title bar)
- `Esc` - Clear filter and close prompt
- Active filter phrase is always visible in the window title

**General:**
- `q` or `Ctrl+C` - Quit

## Installation

Download the latest release for your platform from the [Releases page](https://github.com/raaymax/lazytail/releases):

```bash
# Linux (x86_64)
wget https://github.com/raaymax/lazytail/releases/latest/download/lazytail-linux-x86_64.tar.gz
tar xzf lazytail-linux-x86_64.tar.gz
chmod +x lazytail
sudo mv lazytail /usr/local/bin/

# macOS (Intel)
wget https://github.com/raaymax/lazytail/releases/latest/download/lazytail-macos-x86_64.tar.gz
tar xzf lazytail-macos-x86_64.tar.gz
chmod +x lazytail
sudo mv lazytail /usr/local/bin/

# macOS (Apple Silicon)
wget https://github.com/raaymax/lazytail/releases/latest/download/lazytail-macos-aarch64.tar.gz
tar xzf lazytail-macos-aarch64.tar.gz
chmod +x lazytail
sudo mv lazytail /usr/local/bin/
```

For building from source, see [CONTRIBUTING.md](CONTRIBUTING.md).

## Usage

Run LazyTail with a log file:

```bash
lazytail /path/to/your/logfile.log
```

### Command Line Options

```bash
lazytail [OPTIONS] <FILE>

Options:
  -s, --stdin              Read from stdin instead of a file
  -w, --watch              Enable file watching (default: true)
      --no-watch           Disable file watching
  -h, --help               Print help
```

### Live Filtering Example

1. Press `/` to enter filter mode
2. Start typing - e.g., `err`
3. Watch the results update instantly as you type
4. Continue typing - e.g., `error`
5. Press `Enter` to close the filter prompt (filter stays active)
6. Press `Esc` (in normal mode) to clear the filter

The filter searches through all lines in the background without blocking the UI, so even with large files the interface remains responsive.

### Using Follow Mode

**Follow Mode** allows you to automatically scroll to new log lines as they're written to the file (like `tail -f`):
1. Press `f` to enable follow mode - the status bar will show "FOLLOW"
2. New log lines will automatically scroll into view as they arrive
3. Press `f` again to disable follow mode and manually navigate
4. Any manual scroll action (↑/↓/PgUp/PgDn/g/G) automatically disables follow mode

File watching is enabled by default, so new log lines will appear automatically as they're written to the file. You can disable it with `--no-watch` if needed.

### ANSI Color Support

LazyTail parses ANSI escape codes and renders them in full color! Colored logs from other tools (like `docker logs`, `kubectl logs`, or application logs with color formatting) display beautifully:
- **Green** for INFO
- **Cyan** for DEBUG
- **Yellow** for WARN
- **Red** for ERROR
- Plus all other ANSI colors and styles (bold, dim, etc.)

## Upcoming Features

- [x] File watching with auto-reload (inotify)
- [x] Interactive filter input
- [x] Live filter preview
- [x] Follow mode (tail -f style)
- [ ] STDIN support for piping logs
- [ ] Regex filter mode (regex parsing already implemented)
- [ ] JSON log parsing and formatting
- [ ] Multiple display modes
- [ ] Search highlighting
- [ ] Line number jump
- [ ] Copy selected line
- [ ] Case-sensitive filter toggle
- [ ] Bookmark lines

## Contributing

Contributions are welcome! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for:
- Development setup and guidelines
- Commit message conventions
- CI/CD workflow documentation
- Release process
- Pull request guidelines

## License

LazyTail is licensed under the [MIT License](LICENSE).
