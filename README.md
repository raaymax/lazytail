# LazyTail - Terminal-Based Log Viewer

A fast, universal terminal-based log viewer with live filtering and follow mode. Works with any text log files from applications, services, containers, or systems.

![LazyTail Screenshot](screenshot.png)

## Features

### Implemented
- **Multi-tab support** - Open multiple log files in tabs with side panel navigation
- **Stdin support** - Pipe logs directly with auto-detection (`cmd | lazytail`)
- **Lazy file reading** - Efficiently handles large log files using indexed line positions
- **TUI interface** - Clean terminal UI with ratatui
- **Line selection** - Navigate through logs with keyboard controls
- **Live filtering** - See results instantly as you type your search string
- **Filter history** - Navigate previous filter patterns with Up/Down arrows
- **Background filtering** - Non-blocking regex and string matching filters
- **File watching** - Auto-reload when log file is modified (using inotify on Linux)
- **Follow mode** - Auto-scroll to show latest logs as they arrive (like `tail -f`)
- **ANSI color support** - Parses and renders ANSI escape codes in full color
- **Raw view mode** - Display logs in their original format with line numbers
- **Memory efficient** - Viewport-based rendering keeps RAM usage low
- **Help overlay** - Built-in keyboard shortcut reference (`?` key)
- **Vim-style navigation** - Line jumping (`:123`), vim keybindings, mouse scroll

### Keyboard Controls

**Navigation:**
- `↑`/`k` - Scroll up one line
- `↓`/`j` - Scroll down one line
- `PgUp` - Scroll up one page
- `PgDn` - Scroll down one page
- `g` - Jump to start (first line)
- `G` - Jump to end (last line)
- `:123` - Jump to line 123 (vim-style)
- `zz` - Center selection on screen
- `zt` - Move selection to top of screen
- `zb` - Move selection to bottom of screen
- `f` - Toggle follow mode (auto-scroll to new logs)
- Mouse wheel - Scroll up/down (selection follows scroll)

**Tabs (when multiple files open):**
- `Tab` - Switch to next tab
- `Shift+Tab` - Switch to previous tab
- `1-9` - Jump directly to tab by number

**Filtering:**
- `/` - Enter live filter mode
- Type any text - Results update instantly as you type
- `↑`/`↓` - Navigate filter history (in filter mode)
- `Backspace` - Delete characters
- `Enter` - Close filter prompt (keeps filter active, shown in title bar)
- `Esc` - Clear filter and close prompt
- Active filter phrase is always visible in the window title

**General:**
- `?` - Show help overlay with all keyboard shortcuts
- `q` or `Ctrl+C` - Quit

## Installation

```bash
curl -fsSL https://raw.githubusercontent.com/raaymax/lazytail/master/install.sh | bash
```

That's it! The script auto-detects your OS and architecture, downloads the latest release, and installs to `~/.local/bin`.

<details>
<summary>Alternative installation methods</summary>

### Custom install directory

```bash
curl -fsSL https://raw.githubusercontent.com/raaymax/lazytail/master/install.sh | INSTALL_DIR=/usr/local/bin bash
```

### Arch Linux (AUR)

```bash
yay -S lazytail
```

### Build from source

Requires Rust 1.70+:

```bash
git clone https://github.com/raaymax/lazytail.git
cd lazytail
cargo install --path .
```

</details>

## Usage

Run LazyTail with a log file:

```bash
lazytail /path/to/your/logfile.log
```

Open multiple files in tabs:

```bash
lazytail app.log error.log access.log
```

Pipe logs from other commands (auto-detected):

```bash
kubectl logs pod-name | lazytail
docker logs -f container | lazytail
journalctl -f | lazytail
```

Combine sources - stdin, files, and process substitution:

```bash
app_logs | lazytail error.log <(kubectl logs pod-name)
```

### Command Line Options

```bash
lazytail [OPTIONS] [FILES]...

Options:
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

**Filter History:**
- Press `↑` while in filter mode to recall previous filter patterns
- Press `↓` to navigate forward through history
- Up to 50 recent filter patterns are saved
- Selecting from history immediately applies the filter

### Jumping to Line Numbers

You can jump directly to any line number using vim-style syntax:
1. Press `:` to enter line jump mode
2. Type the line number - e.g., `:150`
3. Press `Enter` to jump to that line
4. Press `Esc` to cancel

This works in both normal and filtered views. In filtered view, it jumps to the nearest matching line.

### Using Follow Mode

**Follow Mode** allows you to automatically scroll to new log lines as they're written to the file (like `tail -f`):
1. Press `f` to enable follow mode - the status bar will show "FOLLOW"
2. New log lines will automatically scroll into view as they arrive
3. Press `f` again to disable follow mode and manually navigate
4. Any manual scroll action (↑/↓/PgUp/PgDn/g/G) automatically disables follow mode

File watching is enabled by default, so new log lines will appear automatically as they're written to the file. You can disable it with `--no-watch` if needed.

### ANSI Color Support

LazyTail parses ANSI escape codes and renders them in full color! Colored logs from other tools display beautifully with their original formatting preserved.

### Use Cases

LazyTail works with any text-based log files:

**Application Logs:**
```bash
lazytail /var/log/myapp/application.log
lazytail ~/.pm2/logs/app-out.log
```

**System Logs:**
```bash
lazytail /var/log/syslog
lazytail /var/log/auth.log
```

**Container Logs:**
```bash
# Docker
docker logs my-container > container.log
lazytail container.log

# Kubernetes
kubectl logs pod-name > pod.log
lazytail pod.log
```

**Web Server Logs:**
```bash
lazytail /var/log/nginx/access.log
lazytail /var/log/apache2/error.log
```

**Build/CI Logs:**
```bash
lazytail build-output.log
lazytail ci-pipeline.log
```

Any plain text log file works - from development logs to production system logs, with or without ANSI colors.

## Contributing

Contributions are welcome! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for:
- Development setup and guidelines
- Commit message conventions
- CI/CD workflow documentation
- Release process
- Pull request guidelines

## License

LazyTail is licensed under the [MIT License](LICENSE).
