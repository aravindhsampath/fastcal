# davcli Patterns and Learnings

This document captures key patterns from studying davcli source code that we'll adopt in fastcal.

## Source Analysis

**Repository**: https://git.sr.ht/~whynothugo/davcli
**Analyzed**: March 4, 2026
**Version**: Latest from main branch

## Key Files Analyzed

- `src/cli.rs` - CLI structure and commands
- `src/caldav.rs` - CalDAV operations and libdav usage
- `src/auth.rs` - Authentication patterns

## Pattern 1: Authentication (src/auth.rs)

### davcli Approach
```rust
pub(crate) fn from_env<S>(service: S) -> anyhow::Result<AddAuthorization<S>> {
    let username = std::env::var("DAVCLI_USERNAME")?;
    let password = std::env::var("DAVCLI_PASSWORD")?;
    Ok(AddAuthorization::new(service, username, password))
}
```

### What We'll Adopt
- Environment variable-based credentials
- Clean separation of auth from HTTP client
- Use `AddAuthorization` wrapper pattern

### What We'll Add
- Support for config file credentials (in addition to env vars)
- Environment variable precedence over config file
- Use `FASTCAL_*` prefix instead of `DAVCLI_*`

## Pattern 2: Client Initialization (src/caldav.rs:33-53)

### davcli Approach
```rust
async fn caldav_client(enable_discovery: bool) -> anyhow::Result<Client> {
    let base_url = std::env::var("DAVCLI_BASE_URL")
        .context("failed to determine base_url")?
        .try_into()
        .context("parsing DAVCLI_BASE_URL")?;

    let https = HttpsConnectorBuilder::new()
        .with_native_roots()?
        .https_or_http()
        .enable_http1()
        .build();

    let raw_client = HyperClient::builder(TokioExecutor::new()).build(https);
    let auth_client = from_env(raw_client)?;
    let webdav = WebDavClient::new(base_url, auth_client);

    let client = if enable_discovery {
        CalDavClient::bootstrap_via_service_discovery(webdav).await?
    } else {
        CalDavClient::new(webdav)
    };

    Ok(client)
}
```

### Key Insights
1. **HTTPS Connector Setup**: Use native roots, support http/https, enable http1
2. **Layered Client**: Raw HTTP → Auth → WebDAV → CalDAV
3. **Service Discovery**: Bootstrap automatically finds endpoints
4. **Error Context**: Use `.context()` for helpful error messages

### What We'll Adopt
- Same client initialization pattern
- Service discovery for initial setup
- Native TLS roots

### What We'll Add
- Read base_url from config file if not in env
- Connection pooling/reuse across commands
- Timeout configuration

## Pattern 3: Service Discovery (src/caldav.rs:79-122)

### davcli Discovery Flow
```rust
async fn discover(mut client: Client) -> anyhow::Result<()> {
    let service = caldav_service_for_url(client.base_url())?;
    println!("- Base url: {}", client.base_url());

    match find_context_url(&client, service).await {
        FindContextUrlResult::BaseUrl => {
            println!("- Base url is a valid context path.");
        }
        FindContextUrlResult::Found(uri) => {
            println!("- Resolved context path: {uri}");
            client.webdav_client.base_url = uri;
        }
        FindContextUrlResult::NoneFound => {
            println!("- Context path not found; using given URL. This might not work.");
        }
        FindContextUrlResult::Error(err) => {
            bail!("No usable context path found: {err}");
        }
    }

    match client.find_current_user_principal().await? {
        Some(principal) => {
            println!("- Current user principal: {principal}");
            let home_sets = client.request(FindCalendarHomeSet::new(&principal)).await?;
            if home_sets.home_sets.is_empty() {
                println!("- No calendar home set found.");
            } else {
                for collection in home_sets.home_sets {
                    println!("- Calendar home set: {collection}");
                }
            }
            // ... address set discovery
        }
        None => println!("- Current user principal not found."),
    }
    Ok(())
}
```

### Key Steps
1. **Context URL**: Find the CalDAV context path
2. **User Principal**: Identify the current user
3. **Calendar Home Set**: Find where calendars are stored
4. **Address Set**: Get user's email addresses

### What We'll Adopt
- Same discovery sequence
- Handle all result variants properly
- Store discovered URLs

### What We'll Add
- Save discovered URLs to config file
- JSON output instead of println
- List actual calendars (not just home set)

## Pattern 4: Create Event (src/caldav.rs:151-169)

### davcli Create Pattern
```rust
async fn create(client: Client, href: String) -> anyhow::Result<()> {
    let mut data = Vec::new();
    let mut stdin = std::io::stdin().lock();
    stdin.read_to_end(&mut data).context("reading from stdin")?;

    let data_str = String::from_utf8(data).context("parsing stdin as UTF-8")?;
    let response = client
        .request(PutResource::new(&href).create(data_str, "text/calendar"))
        .await
        .context("sending request to create resource")?;

    if let Some(etag) = response.etag {
        println!("Etag: {etag}");
    } else {
        println!("No etag");
    }

    Ok(())
}
```

### Key Insights
1. **PutResource**: Use libdav's `PutResource::new(&href).create(data, "text/calendar")`
2. **Content-Type**: Must be "text/calendar" for ICS
3. **Etag**: Server returns etag for version tracking
4. **Href**: Full URL to the event resource

### What We'll Adopt
- Use `PutResource` for creating events
- Capture and return etag
- Use proper content-type

### What We'll Add
- Generate ICS from command-line flags (not stdin)
- Return full event details in JSON
- Validate ICS before sending

## Pattern 5: Delete Event (src/caldav.rs:243-246)

### davcli Delete Pattern
```rust
async fn delete(client: &Client, href: String) -> anyhow::Result<()> {
    client.request(Delete::new(&href).force()).await?;
    Ok(())
}
```

### Key Insights
1. **Simple**: Just use `Delete::new(&href).force()`
2. **Force**: Skip etag check (davcli doesn't track etags)
3. **Clean**: No response needed

### What We'll Adopt
- Same pattern for deletion
- Use `.force()` since we don't track etags yet

### What We'll Add
- Confirmation prompt before delete (unless --force)
- Return success message in JSON
- Option to track etags in future

## Pattern 6: List Calendars (src/caldav.rs:198-228)

### davcli List Pattern
```rust
async fn list_collections(client: Client) -> anyhow::Result<()> {
    let urls = urls_for_finding_calendars(&client).await?;
    for url in urls {
        let response = client.request(FindCalendars::new(&url)).await?;
        for collection in response.calendars {
            println!("{}", collection.href);

            let name_response = client
                .request(GetProperty::new(&collection.href, &names::DISPLAY_NAME))
                .await;

            if let Ok(GetPropertyResponse { value: Some(name) }) = name_response {
                println!("- Name: {name}");
            }

            let components_response = client
                .request(GetSupportedComponents::new(&collection.href))
                .await;

            if let Ok(components) = components_response {
                if !components.components.is_empty() {
                    for component in components.components {
                        println!("- Component: {}", component.as_str());
                    }
                }
            }
        }
    }
    Ok(())
}
```

### Key Insights
1. **FindCalendars**: libdav request to list calendars
2. **GetProperty**: Fetch DISPLAY_NAME for friendly names
3. **GetSupportedComponents**: Know what each calendar supports (VEVENT, VTODO)
4. **Multiple Home Sets**: Handle multiple calendar home sets

### What We'll Adopt
- Use `FindCalendars` request
- Fetch display names
- Check supported components

### What We'll Add
- JSON output with structured calendar info
- Cache calendar list in config
- Filter calendars by type (our 3 specific ones)

## Pattern 7: List Events (src/caldav.rs:230-241)

### davcli List Pattern
```rust
async fn list_resources(client: &Client, href: String) -> anyhow::Result<()> {
    let response = client.request(ListCalendarResources::new(&href)).await?;
    if response.resources.is_empty() {
        info!("No items in collection");
    } else {
        for resource in response.resources {
            println!("{}", resource.href);
        }
    }
    Ok(())
}
```

### Key Insights
1. **ListCalendarResources**: Lists all resources in a calendar
2. **Returns hrefs only**: Doesn't fetch full event data
3. **Lightweight**: Good for listing

### What We'll Adopt
- Use `ListCalendarResources` for listing
- Separate list (hrefs) from get (full data)

### What We'll Add
- Use `GetCalendarResources` to fetch full event data
- Date range filtering
- Parse ICS to JSON for output
- Search/filter client-side

## Missing from davcli (What We Must Add)

### 1. UPDATE Events
davcli doesn't have an update command. To implement:

```rust
// Fetch existing event
let existing = client.request(GetCalendarResources::new(&collection)
    .with_hrefs(&[event_href])).await?;

// Parse existing ICS, modify fields
let mut event = parse_ics(&existing.data)?;
event.summary = new_summary;

// Put back with existing href (not .create())
let updated_ics = generate_ics(&event)?;
client.request(PutResource::new(&event_href)
    .update(updated_ics, "text/calendar", etag))
    .await?;
```

### 2. ICS ↔ JSON Conversion
davcli works with raw ICS. We need:
- Parse ICS to structured Event model
- Serialize Event to JSON
- Deserialize JSON to Event
- Generate ICS from Event model

Use `ical` crate for parsing/generation.

### 3. Date/Time Parsing
davcli expects ISO8601. We need:
- Multiple format support
- Timezone handling
- Relative dates (future enhancement)

Use `chrono` for this.

### 4. Search & Filter
davcli only lists all. We need:
- Client-side filtering by text
- Date range filtering
- Filter by attendees, location

### 5. Batch Operations
davcli operates one at a time. We need:
- Read JSON array
- Process multiple events
- Return results per event

## libdav API Patterns

### Request Pattern
All libdav operations follow:
```rust
let response = client.request(RequestType::new(args)).await?;
```

### Common Requests
- `FindCalendars::new(&url)` - List calendars
- `ListCalendarResources::new(&href)` - List events (hrefs only)
- `GetCalendarResources::new(&collection).with_hrefs(&[href])` - Get event data
- `PutResource::new(&href).create(data, content_type)` - Create
- `Delete::new(&href).force()` - Delete
- `GetProperty::new(&href, &property_name)` - Get property

### Error Handling
Use `anyhow::Context` for error context:
```rust
.context("description of what failed")?
```

## Action Items for Phase 1

Based on this analysis:

1. **Copy auth pattern** - Use env vars with fallback to config
2. **Copy client init** - Same HTTPS connector setup
3. **Implement discovery** - Use for `fastcal config init`
4. **Store discovered URLs** - Save to config file
5. **Test with Fastmail** - Verify discovery works

## References

- davcli source: `/tmp/davcli/`
- libdav docs: https://docs.rs/libdav/latest/libdav/
- Our plan: `DEVELOPMENT_PLAN.md`
