# fastcal

An AI-friendly CalDAV CLI for managing Fastmail calendars. Built with Rust for speed and reliability.

[![Tests](https://img.shields.io/badge/tests-69%2F69-success)](https://github.com/yourusername/fastcal)
[![Rust](https://img.shields.io/badge/rust-1.70%2B-orange)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](LICENSE)

## Features

- 📅 **Full CalDAV Support** - Create, read, update, delete events
- 🤖 **AI-Friendly** - JSON output for easy parsing by AI assistants
- 🚀 **Fast** - Written in Rust with concurrent HTTP requests
- 🔍 **Search** - Find events by text, date range, or calendar
- ⚡ **Batch Operations** - Create or delete multiple events at once
- 🔄 **Conflict Detection** - Check for scheduling conflicts before booking
- 📝 **Text & JSON Output** - Human-readable or machine-parseable
- 🔐 **Secure** - Uses app-specific passwords, never stores credentials

## Project Status

**Current**: Phase 9 Complete - Production Ready ✅

- ✅ Full CRUD operations on events
- ✅ Search and conflict detection
- ✅ Batch create/delete
- ✅ 69/69 tests passing (100%)
- ✅ Zero warnings (compilation + clippy)
- ✅ Comprehensive API documentation

See [DEVELOPMENT_PLAN.md](DEVELOPMENT_PLAN.md) for detailed progress.

## Quick Start

### Installation

#### From Source (Requires Rust 1.70+)

```bash
git clone https://github.com/yourusername/fastcal.git
cd fastcal
cargo install --path .
```

#### Using Cargo

```bash
cargo install fastcal
```

### Initial Setup

1. **Create a Fastmail app password:**
   - Go to https://www.fastmail.com/settings/security/password
   - Click "New App Password"
   - Name it "fastcal" and generate
   - Copy the password

2. **Set environment variables:**
   ```bash
   export FASTCAL_USERNAME="your-email@fastmail.com"
   export FASTCAL_PASSWORD="your-app-password"
   ```

3. **Initialize configuration:**
   ```bash
   fastcal config init
   ```

4. **Test the connection:**
   ```bash
   fastcal config test
   ```

### Basic Usage

#### List Today's Events

```bash
# Human-readable format (default)
fastcal events list --from today --to today

# JSON format for scripting/AI
fastcal events list --from today --to today --format json
```

#### Create an Event

```bash
fastcal events create \
  --summary "Team Meeting" \
  --start "2026-03-10T14:00:00-08:00" \
  --duration 60 \
  --location "Conference Room A"
```

#### Search Events

```bash
fastcal events search "dentist"
```

#### Check for Conflicts

```bash
fastcal events conflicts \
  --start "2026-03-10T14:00:00-08:00" \
  --end "2026-03-10T15:00:00-08:00"
```

#### Update an Event

```bash
EVENT_ID=$(fastcal events search "dentist" --format json | jq -r '.events[0].id')
fastcal events update $EVENT_ID --start "2026-03-15T10:00:00-08:00"
```

#### Batch Operations

```bash
fastcal batch create events.json
```

## Command Reference

### Global Options

```
-c, --config <PATH>        Custom config file path
-f, --format <FORMAT>      Output format: text|json [default: text, or config preference]
    --calendar <NAME>      Target calendar
    --dry-run              Preview mutations without sending to server
-v, --verbose              Enable verbose logging
-h, --help                 Print help
```

### Commands

- `config init|show|set|test` - Configuration management
- `calendars list|info` - Calendar operations
- `events list|get|create|update|delete|search|conflicts` - Event operations
- `batch create|delete` - Batch operations
- `completions <shell>` - Generate shell completions (bash, zsh, fish, etc.)

## Configuration

Config file: `~/.config/fastcal/config.toml`

```toml
[server]
url = "https://fastmail.com"
username = "user@fastmail.com"

[calendars]
Personal = "https://caldav.fastmail.com/.../personal-uuid/"
Work = "https://caldav.fastmail.com/.../work-uuid/"

[preferences]
default_calendar = "Personal"
default_timezone = "America/Los_Angeles"
output_format = "text"
```

### Environment Variables

- `FASTCAL_USERNAME` - Your Fastmail email
- `FASTCAL_PASSWORD` - Your app-specific password
- `FASTCAL_BASE_URL` - CalDAV server URL

## Output Formats

### Text (Human-Readable)

```
📅 Team Meeting
   2026-03-10 14:00:00 PST
   📍 Conference Room A
   ⏱️  60 min
```

### JSON (AI/Script-Friendly)

```json
{
  "events": [{
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "summary": "Team Meeting",
    "start": "2026-03-10T14:00:00-08:00",
    "duration_minutes": 60,
    "location": "Conference Room A"
  }]
}
```

## AI Assistant Integration

See [examples/ai_assistant_usage.md](examples/ai_assistant_usage.md) and [docs/API.md](docs/API.md) for integration patterns.

## DateTime Format

ISO 8601 with timezone: `YYYY-MM-DDTHH:MM:SS±HH:MM`

Examples:
- `2026-03-10T14:00:00-08:00` (Pacific)
- `2026-03-10T22:00:00Z` (UTC)

## Troubleshooting

### Authentication Errors

Check your credentials and ensure you're using an **app-specific password**.

```bash
fastcal config test  # Verify connection
```

### Verbose Logging

```bash
fastcal -v events list  # Enable debug output
```

## Development

### Building from Source

```bash
cargo build --release
```

### Running Tests

```bash
cargo test  # 69/69 tests passing
```

### Code Quality

```bash
cargo clippy --all-targets -- -D warnings  # Zero warnings
cargo fmt  # Format code
```

## Documentation

- [API Documentation](docs/API.md) - Complete API reference
- [Development Plan](DEVELOPMENT_PLAN.md) - Implementation roadmap
- [AI Integration Guide](examples/ai_assistant_usage.md) - For AI assistants
- [Fastmail Setup](docs/FASTMAIL_SETUP.md) - Configuration guide

## Contributing

Contributions welcome! Please:

1. Fork the repository
2. Create a feature branch
3. Add tests for new functionality
4. Ensure all tests pass
5. Submit a pull request

## License

Dual-licensed under MIT OR Apache-2.0.

## Acknowledgments

- Built with [libdav](https://git.sr.ht/~whynothugo/libdav)
- Inspired by [davcli](https://git.sr.ht/~whynothugo/davcli)
- Thanks to the Rust community

---

Built with ❤️ and Rust