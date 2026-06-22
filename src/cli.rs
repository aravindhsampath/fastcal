// Copyright 2026 Aravindh Sampath Kumar
// SPDX-License-Identifier: MIT OR Apache-2.0

use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::Shell;

#[derive(Clone, Copy, ValueEnum)]
pub enum OutputFormat {
    Json,
    Ics,
    Text,
}

#[derive(Parser)]
#[command(
    name = "fastcal",
    version,
    author,
    about = "AI-friendly CalDAV CLI for Fastmail calendar management",
    long_about = None
)]
pub struct Cli {
    /// Custom config file path
    #[arg(short, long, global = true)]
    pub config: Option<String>,

    /// Output format (default: from config preferences.output_format, or "text")
    #[arg(short, long, global = true, value_enum)]
    pub format: Option<OutputFormat>,

    /// Target calendar name (as configured in config.toml)
    #[arg(long, global = true)]
    pub calendar: Option<String>,

    /// Verbose logging
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Dry-run: parse and validate without sending mutations to the server
    #[arg(long, global = true)]
    pub dry_run: bool,

    /// IANA timezone for interpreting and displaying times this invocation
    /// (e.g. "America/New_York"). Overrides preferences.default_timezone.
    #[arg(long, global = true)]
    pub timezone: Option<String>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Configuration management
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },

    /// Calendar operations
    Calendars {
        #[command(subcommand)]
        command: CalendarCommands,
    },

    /// Event operations
    Events {
        #[command(subcommand)]
        command: EventCommands,
    },

    /// Batch operations
    Batch {
        #[command(subcommand)]
        command: BatchCommands,
    },

    /// Generate shell completion scripts
    Completions {
        /// Shell to generate completions for
        shell: Shell,
    },
}

#[derive(Subcommand)]
pub enum ConfigCommands {
    /// Initialize config (discover + save)
    Init,

    /// Display current config
    Show,

    /// Set config value
    Set { key: String, value: String },

    /// Test connection to Fastmail
    Test,
}

#[derive(Subcommand)]
pub enum CalendarCommands {
    /// List all calendars
    List,

    /// Show calendar details
    Info { calendar: String },
}

#[derive(Subcommand)]
pub enum EventCommands {
    /// List events
    List {
        /// Start date (default: today)
        #[arg(long)]
        from: Option<String>,

        /// End date (default: +30 days)
        #[arg(long)]
        to: Option<String>,
    },

    /// Get event details
    Get { event_id: String },

    /// Create new event
    Create {
        /// Event title (required unless --from-json is used)
        #[arg(long)]
        summary: Option<String>,

        /// Start time (required unless --from-json is used)
        #[arg(long)]
        start: Option<String>,

        /// End time
        #[arg(long)]
        end: Option<String>,

        /// Duration in minutes
        #[arg(long)]
        duration: Option<u32>,

        /// Location
        #[arg(long)]
        location: Option<String>,

        /// Description
        #[arg(long)]
        description: Option<String>,

        /// Comma-separated attendee emails
        #[arg(long)]
        attendees: Option<String>,

        /// Set a DISPLAY reminder N minutes before the event start.
        /// Repeat with larger numbers for hours / days
        /// (e.g. 60 for 1 hour, 1440 for 1 day). Omit for no reminder.
        #[arg(long)]
        reminder_minutes: Option<u32>,

        /// Create event from JSON file (fields can be overridden by other flags)
        #[arg(long)]
        from_json: Option<String>,
    },

    /// Update existing event
    Update {
        event_id: String,

        /// New title
        #[arg(long)]
        summary: Option<String>,

        /// New start time
        #[arg(long)]
        start: Option<String>,

        /// New end time
        #[arg(long)]
        end: Option<String>,

        /// New location
        #[arg(long)]
        location: Option<String>,

        /// New description
        #[arg(long)]
        description: Option<String>,

        /// New attendees (comma-separated emails)
        #[arg(long)]
        attendees: Option<String>,

        /// Replace all existing VALARMs with a single DISPLAY reminder
        /// N minutes before start. Omit to leave the event's existing
        /// reminders untouched; pass `--no-reminders` to strip them.
        #[arg(long, conflicts_with = "no_reminders")]
        reminder_minutes: Option<u32>,

        /// Strip all VALARMs from the event. Mutually exclusive with
        /// `--reminder-minutes`.
        #[arg(long)]
        no_reminders: bool,
    },

    /// Delete event
    Delete {
        event_id: String,

        /// Skip confirmation
        #[arg(long)]
        force: bool,
    },

    /// Search events
    Search {
        query: String,

        /// Start date
        #[arg(long)]
        from: Option<String>,

        /// End date
        #[arg(long)]
        to: Option<String>,
    },

    /// Check for scheduling conflicts
    Conflicts {
        /// Proposed start time
        #[arg(long)]
        start: String,

        /// Proposed end time
        #[arg(long)]
        end: String,
    },
}

#[derive(Subcommand)]
pub enum BatchCommands {
    /// Create multiple events from JSON
    Create { json_file: String },

    /// Delete multiple events from JSON
    Delete { json_file: String },
}

impl Cli {
    pub async fn execute(self) -> anyhow::Result<()> {
        let Cli {
            config,
            format,
            calendar,
            dry_run,
            timezone,
            command,
            ..
        } = self;

        // Load config once (best-effort): it feeds both the default output
        // format and the timezone-resolution precedence. Commands that
        // require config re-load and surface a proper error themselves.
        let loaded_config = if let Some(ref path) = config {
            crate::config::Config::load_from(&std::path::PathBuf::from(path)).ok()
        } else {
            crate::config::Config::load().ok()
        };

        // Resolve effective format: CLI flag > config preference > default (text)
        let effective_format = format.unwrap_or_else(|| {
            match loaded_config
                .as_ref()
                .map(|c| c.preferences.output_format.as_str())
            {
                Some("json") => OutputFormat::Json,
                Some("ics") => OutputFormat::Ics,
                _ => OutputFormat::Text,
            }
        });

        // Resolve the one timezone for this invocation (flag > config >
        // system > UTC). An explicitly-set-but-invalid zone fails fast here.
        let tz = crate::timezone::resolve(
            timezone.as_deref(),
            loaded_config
                .as_ref()
                .map(|c| c.preferences.default_timezone.as_str()),
        )?;

        let ctx = crate::commands::context::CommandContext::new(
            config,
            effective_format,
            calendar,
            dry_run,
            tz,
            loaded_config,
        );

        match command {
            Commands::Config { command } => {
                use crate::commands::config;

                match command {
                    ConfigCommands::Init => config::init(&ctx).await,
                    ConfigCommands::Show => config::show(&ctx).await,
                    ConfigCommands::Set { key, value } => config::set(&ctx, key, value).await,
                    ConfigCommands::Test => config::test(&ctx).await,
                }
            }
            Commands::Calendars { command } => {
                use crate::commands::calendars;

                match command {
                    CalendarCommands::List => calendars::list(&ctx).await,
                    CalendarCommands::Info { calendar } => calendars::info(&ctx, calendar).await,
                }
            }
            Commands::Events { command } => {
                use crate::commands::events;

                match command {
                    EventCommands::List { from, to } => {
                        // Parse dates in the resolved zone; error on invalid input.
                        // `--to` is the exclusive end of a half-open range, so a
                        // date-only value covers through the end of that local day.
                        let from_dt = from
                            .as_ref()
                            .map(|s| crate::parsers::datetime::parse_datetime(s, tz))
                            .transpose()?;
                        let to_dt = to
                            .as_ref()
                            .map(|s| crate::parsers::datetime::parse_range_end(s, tz))
                            .transpose()?;

                        events::list(&ctx, from_dt, to_dt).await
                    }
                    EventCommands::Get { event_id } => events::get(&ctx, event_id).await,
                    EventCommands::Create {
                        summary,
                        start,
                        end,
                        duration,
                        location,
                        description,
                        attendees,
                        reminder_minutes,
                        from_json,
                    } => {
                        events::create(
                            &ctx,
                            events::EventCreateOverrides {
                                summary,
                                start,
                                end,
                                duration,
                                location,
                                description,
                                attendees,
                                reminder_minutes,
                            },
                            from_json,
                        )
                        .await
                    }
                    EventCommands::Update {
                        event_id,
                        summary,
                        start,
                        end,
                        location,
                        description,
                        attendees,
                        reminder_minutes,
                        no_reminders,
                    } => {
                        events::update(
                            &ctx,
                            event_id,
                            events::EventUpdatePatch {
                                summary,
                                start,
                                end,
                                location,
                                description,
                                attendees,
                                reminder_minutes,
                                no_reminders,
                            },
                        )
                        .await
                    }
                    EventCommands::Delete { event_id, force } => {
                        events::delete(&ctx, event_id, force).await
                    }
                    EventCommands::Search { query, from, to } => {
                        // Parse dates in the resolved zone; `--to` is the
                        // exclusive end of a half-open range.
                        let from_dt = from
                            .as_ref()
                            .map(|s| crate::parsers::datetime::parse_datetime(s, tz))
                            .transpose()?;
                        let to_dt = to
                            .as_ref()
                            .map(|s| crate::parsers::datetime::parse_range_end(s, tz))
                            .transpose()?;

                        events::search(&ctx, query, from_dt, to_dt).await
                    }
                    EventCommands::Conflicts { start, end } => {
                        events::conflicts(&ctx, start, end).await
                    }
                }
            }
            Commands::Batch { command } => {
                use crate::commands::batch;

                match command {
                    BatchCommands::Create { json_file } => batch::create(&ctx, json_file).await,
                    BatchCommands::Delete { json_file } => batch::delete(&ctx, json_file).await,
                }
            }
            Commands::Completions { shell } => {
                clap_complete::generate(
                    shell,
                    &mut Cli::command(),
                    "fastcal",
                    &mut std::io::stdout(),
                );
                Ok(())
            }
        }
    }
}
