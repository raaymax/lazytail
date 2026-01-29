# LazyTail - Terminal-Based Log Viewer

A fast, universal terminal-based log viewer with live filtering and follow mode. Works with any text log files from applications, services, containers, or systems.

![LazyTail Screenshot](screenshot.png)

## Features

- **Multi-tab support** - Open multiple log files in tabs with side panel navigation
- **Stdin support** - Pipe logs directly with auto-detection (`cmd | lazytail`)
- **Lazy file reading** - Efficiently handles large log files using indexed line positions
- **TUI interface** - Clean terminal UI with ratatui
- **Live filtering** - See results instantly as you type with regex or plain text
- **Filter history** - Navigate and reuse previous filter patterns
- **Background filtering** - Non-blocking filtering keeps UI responsive
- **File watching** - Auto-reload when log file is modified (using inotify on Linux)
- **Follow mode** - Auto-scroll to show latest logs as they arrive (like `tail -f`)
- **ANSI color support** - Parses and renders ANSI escape codes in full color
- **Line expansion** - Expand long lines for better readability
- **Memory efficient** - Viewport-based rendering keeps RAM usage low
- **Vim-style navigation** - Familiar keybindings for efficient navigation

Press `?` in the app to see all keyboard shortcuts.

## Installation

```bash
curl -fsSL https://raw.githubusercontent.com/raaymax/lazytail/master/install.sh | bash
# or
wget -qO- https://raw.githubusercontent.com/raaymax/lazytail/master/install.sh | bash
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
  -n, --name <NAME>        Capture stdin to ~/.config/lazytail/data/<NAME>.log
      --no-watch           Disable file watching
  -h, --help               Print help
```

### Source Discovery Mode

Run `lazytail` with no arguments to auto-discover log sources:

```bash
lazytail  # Opens all *.log files from ~/.config/lazytail/data/
```

### Capture Mode

Capture logs from any command to a named source (tee-like behavior):

```bash
# Terminal 1: Capture API logs
kubectl logs -f api-pod | lazytail -n "API"

# Terminal 2: Capture worker logs
docker logs -f worker | lazytail -n "Worker"

# Terminal 3: View all sources
lazytail  # Shows tabs: [API] [Worker] with live status
```

Captured sources show active (●) or ended (○) status in the UI.

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
kubectl logs pod-name | lazytail
docker logs -f container | lazytail
```

**Web Server Logs:**
```bash
lazytail /var/log/nginx/access.log
lazytail /var/log/apache2/error.log
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
