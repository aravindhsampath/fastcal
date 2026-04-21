# Fastmail CalDAV Setup Guide

This guide walks you through setting up `fastcal` to work with your Fastmail account.

## Prerequisites

- A Fastmail account (any tier)
- Rust 1.70+ installed (for building from source)
- Basic familiarity with the command line

## Step 1: Create an App-Specific Password

For security, never use your main Fastmail password with third-party apps. Instead, create an app-specific password:

1. **Log in to Fastmail** at https://www.fastmail.com

2. **Navigate to Security Settings**
   - Click your profile icon (top right)
   - Go to **Settings** → **Password & Security**
   - Or visit directly: https://www.fastmail.com/settings/security/password

3. **Create New App Password**
   - Scroll to the "App Passwords" section
   - Click **"New App Password"**
   - Name it **"fastcal"** (or any memorable name)
   - Click **"Generate Password"**

4. **Copy the Password**
   - The password will be shown **only once**
   - Copy it immediately and save it securely
   - It will look like: `xxxx-xxxx-xxxx-xxxx`

## Step 2: Set Environment Variables

Set your credentials as environment variables. This keeps them out of your command history and config files.

### Option A: Temporary (Current Session Only)

```bash
export FASTCAL_USERNAME="your-email@fastmail.com"
export FASTCAL_PASSWORD="xxxx-xxxx-xxxx-xxxx"  # Your app password
```

### Option B: Permanent (Add to Shell Profile)

Add these lines to your shell profile file:

**For Bash** (~/.bashrc or ~/.bash_profile):
```bash
echo 'export FASTCAL_USERNAME="your-email@fastmail.com"' >> ~/.bashrc
echo 'export FASTCAL_PASSWORD="xxxx-xxxx-xxxx-xxxx"' >> ~/.bashrc
source ~/.bashrc
```

**For Zsh** (~/.zshrc):
```bash
echo 'export FASTCAL_USERNAME="your-email@fastmail.com"' >> ~/.zshrc
echo 'export FASTCAL_PASSWORD="xxxx-xxxx-xxxx-xxxx"' >> ~/.zshrc
source ~/.zshrc
```

**For Fish** (~/.config/fish/config.fish):
```fish
set -Ux FASTCAL_USERNAME "your-email@fastmail.com"
set -Ux FASTCAL_PASSWORD "xxxx-xxxx-xxxx-xxxx"
```

## Step 3: Initialize Configuration

Run the initialization command to discover your calendars and create the config file:

```bash
fastcal config init
```

This command will:
- Connect to Fastmail's CalDAV server
- Discover all your calendars automatically
- Create `~/.config/fastcal/config.toml` with your calendar URLs
- Set reasonable defaults (timezone, default calendar, etc.)

### What Gets Created

The config file at `~/.config/fastcal/config.toml` will look like:

```toml
[server]
url = "https://caldav.fastmail.com"
username = "your-email@fastmail.com"

[calendars]
Personal = "https://caldav.fastmail.com/dav/calendars/user/your-email@fastmail.com/abc123-def456/"
Work = "https://caldav.fastmail.com/dav/calendars/user/your-email@fastmail.com/xyz789-uvw012/"

[preferences]
default_calendar = "Personal"
default_timezone = "America/Los_Angeles"
output_format = "text"
```

**Note**: Calendar UUIDs are unique identifiers assigned by Fastmail. You don't need to know or modify them.

## Step 4: Test the Connection

Verify everything is working:

```bash
fastcal config test
```

You should see:
```
✓ Connected to Fastmail CalDAV server
✓ Authenticated as your-email@fastmail.com
✓ Found 2 calendars
```

If you see errors, check:
- Environment variables are set correctly
- App password is correct (regenerate if needed)
- Internet connection is working

## Step 5: List Your Calendars

Confirm which calendars are available:

```bash
fastcal calendars list
```

Output:
```
📅 Personal
   https://caldav.fastmail.com/dav/calendars/user/...

📅 Work
   https://caldav.fastmail.com/dav/calendars/user/...
```

## Step 6: Try Creating an Event

Create a test event to confirm everything works:

```bash
fastcal events create \
  --summary "Test Event" \
  --start "2026-03-10T10:00:00-08:00" \
  --duration 30
```

Then list today's events to see it:

```bash
fastcal events list --from today --to today
```

## Customizing Your Configuration

### Change Default Calendar

Edit `~/.config/fastcal/config.toml`:

```toml
[preferences]
default_calendar = "Work"  # Use Work calendar by default
```

### Change Default Timezone

```toml
[preferences]
default_timezone = "America/New_York"  # Or any IANA timezone
```

### Change Default Output Format

```toml
[preferences]
output_format = "json"  # Use JSON output by default
```

## Multiple Accounts

To use multiple Fastmail accounts, create separate config files:

```bash
# Personal account
export FASTCAL_USERNAME="personal@fastmail.com"
export FASTCAL_PASSWORD="xxxx-xxxx-xxxx-xxxx"
fastcal config init -c ~/.config/fastcal/personal.toml

# Work account
export FASTCAL_USERNAME="work@company.com"
export FASTCAL_PASSWORD="yyyy-yyyy-yyyy-yyyy"
fastcal config init -c ~/.config/fastcal/work.toml
```

Then use `-c` flag to specify which config:

```bash
fastcal -c ~/.config/fastcal/work.toml events list
```

## Troubleshooting

### "Authentication failed"

- **Cause**: Wrong username or password
- **Fix**:
  1. Verify `FASTCAL_USERNAME` matches your Fastmail email
  2. Regenerate app password at https://www.fastmail.com/settings/security/password
  3. Update `FASTCAL_PASSWORD` with new password
  4. Run `fastcal config test` to verify

### "Connection refused" or "Could not connect"

- **Cause**: Network issue or firewall blocking CalDAV
- **Fix**:
  1. Check internet connection
  2. Try `curl https://caldav.fastmail.com` to test connectivity
  3. Check if corporate firewall blocks CalDAV (port 443)

### "Calendar not found"

- **Cause**: Calendar was deleted or renamed in Fastmail
- **Fix**: Run `fastcal config init` to re-discover calendars

### "Invalid datetime format"

- **Cause**: Incorrect datetime string
- **Fix**: Use ISO 8601 format: `YYYY-MM-DDTHH:MM:SS±HH:MM`
  - Example: `2026-03-10T14:00:00-08:00`
  - Or natural format: `2026-03-10 2pm`

### Enable Verbose Logging

For debugging, enable verbose output:

```bash
fastcal -v events list
```

This shows all HTTP requests and responses.

## Security Best Practices

1. **Use App Passwords**: Never use your main Fastmail password
2. **Revoke Unused Passwords**: Delete old app passwords at https://www.fastmail.com/settings/security/password
3. **Don't Commit Credentials**: Never add `.env` files or credentials to git
4. **Use Environment Variables**: Keep credentials out of config files and command history
5. **Rotate Passwords**: Regenerate app passwords periodically

## Getting Help

- **Documentation**: See [README.md](../README.md) for full CLI reference
- **API Reference**: See [API.md](API.md) for integration details
- **Issues**: Report bugs at https://github.com/yourusername/fastcal/issues

## Next Steps

Now that you're set up, check out:

- [AI Integration Guide](../examples/ai_assistant_usage.md) - Use with AI assistants
- [API Documentation](API.md) - Complete command reference
- [README.md](../README.md) - Full feature overview
