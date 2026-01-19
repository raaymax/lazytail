# Contributing to LazyTail

Thank you for your interest in contributing to LazyTail! This document provides guidelines and information for contributors.

## Development Setup

### Prerequisites

- Rust (latest stable version)
- Git

### Building from Source

```bash
# Clone the repository
git clone https://github.com/raaymax/lazytail.git
cd lazytail

# Build in debug mode
cargo build

# Build in release mode
cargo build --release

# Run tests
cargo test

# Run with a log file
cargo run -- test.log

# Run in release mode with a log file
cargo run --release -- test.log
```

## Project Architecture

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

For detailed architecture documentation, see `CLAUDE.md`.

## Performance Characteristics

The viewer is designed to handle large log files efficiently:
- **Line indexing**: O(n) one-time indexing, then O(1) random access
- **Viewport rendering**: Only renders visible lines
- **Background filtering**: Non-blocking filter execution in separate thread
- **Memory usage**: ~constant regardless of file size (only viewport buffer in RAM)
- **Incremental filtering**: Only filters new log lines when file grows

## Dependencies

- **ratatui** - TUI framework
- **crossterm** - Cross-platform terminal manipulation
- **notify** - File system watching
- **regex** - Regular expression support
- **serde_json** - JSON parsing
- **clap** - CLI argument parsing
- **anyhow** - Error handling
- **ansi-to-tui** - ANSI escape code parsing and color rendering

### Development Tools

```bash
# Run clippy for linting
cargo clippy

# Check code formatting
cargo fmt -- --check

# Format code
cargo fmt
```

## Testing

### Test Log Files

The repository includes test log files and generators:
- `test.log` - Plain text logs with various log levels (INFO, DEBUG, WARN, ERROR)
- `generate_logs.sh` - Script to generate plain text logs continuously
- `generate_colored_logs.sh` - Script to generate ANSI-colored logs continuously

### Testing Live Reload and Follow Mode

Test the file watching feature:

```bash
# Terminal 1: Start generating logs
./generate_logs.sh live_test.log

# Terminal 2: Watch the logs in real-time
cargo run --release -- live_test.log
```

Press `f` to enable follow mode and watch new lines scroll into view automatically.

### Testing with Colored Logs

Test ANSI color support:

```bash
# Terminal 1: Generate colored logs
./generate_colored_logs.sh live_test_colored.log

# Terminal 2: View the logs with full color rendering
cargo run --release -- live_test_colored.log
```

## Commit Convention

This project uses [Conventional Commits](https://www.conventionalcommits.org/) for automatic changelog generation and version management.

### Commit Message Format

```
<type>: <description>

[optional body]

[optional footer(s)]
```

### Commit Types

- **feat**: A new feature (bumps minor version: 0.1.0 → 0.2.0)
- **fix**: A bug fix (bumps patch version: 0.1.0 → 0.1.1)
- **docs**: Documentation only changes
- **style**: Changes that don't affect code meaning (whitespace, formatting)
- **refactor**: Code change that neither fixes a bug nor adds a feature
- **perf**: Performance improvement
- **test**: Adding or modifying tests
- **chore**: Changes to build process or auxiliary tools

### Examples

```bash
# Feature addition
git commit -m "feat: add JSON log parsing support"

# Bug fix
git commit -m "fix: resolve viewport scrolling issue in filtered mode"

# Documentation
git commit -m "docs: update installation instructions"

# Breaking change (bumps major version: 0.1.0 → 1.0.0)
git commit -m "feat: redesign filter API

BREAKING CHANGE: Filter trait now requires async implementation"
```

## CI/CD Workflows

The project uses GitHub Actions for continuous integration and automated releases.

### CI Workflow (`.github/workflows/ci.yml`)

Runs on every push and pull request to the master branch.

**Jobs:**
- **Test**: Runs on Linux and macOS
  - Executes all tests with `cargo test`
  - Runs `cargo clippy` for linting
  - Checks code formatting with `cargo fmt`
- **Build**: Creates artifacts for all platforms
  - Linux x86_64
  - macOS x86_64 (Intel)
  - macOS aarch64 (Apple Silicon)

The CI must pass before PRs can be merged.

### Release PR Workflow (`.github/workflows/release-pr.yml`)

Runs automatically on every push to the master branch.

**Functionality:**
- Uses [release-please](https://github.com/googleapis/release-please) to automate releases
- Analyzes commit messages since the last release
- Creates or updates a release PR with:
  - Updated `CHANGELOG.md` with all changes
  - Version bump in `Cargo.toml` based on commit types
  - Generated release notes

**Version Bumping:**
- `feat:` commits → minor version bump (0.1.0 → 0.2.0)
- `fix:` commits → patch version bump (0.1.0 → 0.1.1)
- `BREAKING CHANGE:` → major version bump (0.1.0 → 1.0.0)

### Release Workflow (`.github/workflows/release.yml`)

Triggered when the release PR is merged (release is published).

**Jobs:**
- Builds optimized, stripped binaries for all platforms
- Compresses binaries as `.tar.gz` archives
- Uploads artifacts to the GitHub release:
  - `lazytail-linux-x86_64.tar.gz`
  - `lazytail-macos-x86_64.tar.gz`
  - `lazytail-macos-aarch64.tar.gz`

## Release Process

The release process is fully automated:

### 1. Make Commits

Commit your changes to the master branch using conventional commit messages:

```bash
git commit -m "feat: add new filtering mode"
git commit -m "fix: correct ANSI color parsing"
git push origin master
```

### 2. Release PR Created

After your commits are pushed:
- The release-please workflow automatically runs
- A release PR is created or updated (if one already exists)
- The PR includes:
  - All changes since the last release in `CHANGELOG.md`
  - Version bump in `Cargo.toml`
  - Generated release notes

### 3. Review and Merge

When ready to publish a release:
1. Review the auto-generated release PR
2. Verify the changelog and version bump are correct
3. Merge the release PR

### 4. Release Published

After merging the release PR:
- A new GitHub release is automatically created with a version tag
- The release workflow builds binaries for all platforms
- Binaries are uploaded to the release page
- Users can download the new release

## Pull Request Guidelines

1. **Create a feature branch**: Don't commit directly to master
   ```bash
   git checkout -b feature/your-feature-name
   ```

2. **Write conventional commits**: Follow the commit convention outlined above

3. **Add tests**: If adding new functionality, include tests

4. **Run checks locally**: Before pushing, run:
   ```bash
   cargo test
   cargo clippy
   cargo fmt
   ```

5. **Write clear PR descriptions**: Explain what the PR does and why

6. **Keep PRs focused**: One feature/fix per PR when possible

## Code Style

- Follow Rust standard formatting (`cargo fmt`)
- Address all clippy warnings (`cargo clippy`)
- Write clear, self-documenting code
- Add comments only when the logic isn't self-evident
- Keep functions focused and reasonably sized

## Testing

- Write unit tests for new functions and modules
- Test edge cases and error conditions
- Run the full test suite before submitting PRs
- Manually test with various log files when changing UI or filtering logic

## Architecture Guidelines

See `CLAUDE.md` for detailed architecture documentation including:
- Core event loop design
- State management patterns
- Viewport scrolling system
- Reader architecture
- Filter system
- File watching implementation

## Questions or Issues?

- Open an issue on GitHub for bugs or feature requests
- Check existing issues before creating new ones
- Provide detailed reproduction steps for bugs
- Include log samples when reporting filtering or parsing issues

## License

By contributing to LazyTail, you agree that your contributions will be licensed under the same license as the project.
