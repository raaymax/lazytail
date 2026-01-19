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

## Architecture

```
src/
├── main.rs              # Entry point, CLI, and main event loop
├── app.rs               # Application state management
├── reader/
│   ├── mod.rs          # LogReader trait
│   └── file_reader.rs  # Lazy file reader with line indexing
├── filter/
│   ├── mod.rs          # Filter trait
│   ├── engine.rs       # Background filtering engine
│   ├── regex_filter.rs # Regex filter implementation
│   └── string_filter.rs# String matching filter
├── ui/
│   └── mod.rs          # ratatui rendering logic
└── watcher.rs          # File watching with inotify
```

## Installation

### Download Pre-built Binaries

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

### Build from Source

```bash
git clone https://github.com/raaymax/lazytail.git
cd lazytail
cargo build --release
sudo cp target/release/lazytail /usr/local/bin/
```

## Usage

Run the application with a log file:

```bash
cargo run --release -- test.log
```

Or build and run the binary:

```bash
cargo build --release
./target/release/lazytail test.log
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

### Testing Live Reload and Follow Mode

Test the file watching feature with the included log generator:

```bash
# Terminal 1: Start generating logs
./generate_logs.sh live_test.log

# Terminal 2: Watch the logs in real-time
cargo run --release -- live_test.log
```

**Using Follow Mode:**
1. Press `f` to enable follow mode - the status bar will show "FOLLOW"
2. New log lines will automatically scroll into view as they arrive
3. Press `f` again to disable follow mode and manually navigate
4. Any manual scroll action (↑/↓/PgUp/PgDn/g/G) automatically disables follow mode

New log lines will appear automatically as they're written to the file. File watching is enabled by default, but you can disable it with `--no-watch` if needed.

### Testing with Colored Logs

Test with ANSI-colored logs using the colored log generator:

```bash
# Terminal 1: Generate colored logs
./generate_colored_logs.sh live_test_colored.log

# Terminal 2: View the logs with full color rendering
cargo run --release -- live_test_colored.log
```

The viewer parses ANSI escape codes and renders them in full color! Colored logs from other tools (like `docker logs`, `kubectl logs`, or application logs with color formatting) display beautifully:
- **Green** for INFO
- **Cyan** for DEBUG
- **Yellow** for WARN
- **Red** for ERROR
- Plus all other ANSI colors and styles (bold, dim, etc.)

## Testing

Test log files are included:
- `test.log` - Plain text logs with various log levels (INFO, DEBUG, WARN, ERROR)
- `generate_logs.sh` - Script to generate plain text logs
- `generate_colored_logs.sh` - Script to generate ANSI-colored logs

## Performance

The viewer is designed to handle large log files efficiently:
- **Line indexing**: O(n) one-time indexing, then O(1) random access
- **Viewport rendering**: Only renders visible lines
- **Background filtering**: Non-blocking filter execution in separate thread
- **Memory usage**: ~constant regardless of file size (only viewport buffer in RAM)

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

## Dependencies

- **ratatui** - TUI framework
- **crossterm** - Cross-platform terminal manipulation
- **notify** - File system watching
- **regex** - Regular expression support
- **serde_json** - JSON parsing
- **clap** - CLI argument parsing
- **anyhow** - Error handling
- **ansi-to-tui** - ANSI escape code parsing and color rendering

## CI/CD

The project uses GitHub Actions for continuous integration and releases:

- **CI Workflow** (`.github/workflows/ci.yml`): Runs on every push and pull request
  - Tests on Linux and macOS
  - Runs clippy for linting
  - Checks code formatting
  - Builds artifacts for all supported platforms

- **Release Workflow** (`.github/workflows/release.yml`): Triggered on version tags
  - Builds optimized binaries for Linux (x86_64) and macOS (x86_64, aarch64)
  - Strips debug symbols to reduce binary size
  - Creates a GitHub release with compressed binaries

### Creating a Release

To create a new release:

```bash
# Tag the commit
git tag v0.1.0
git push origin v0.1.0
```

The release workflow will automatically:
1. Build binaries for all platforms
2. Create a GitHub release
3. Upload the compressed binaries as release assets
