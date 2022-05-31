# Golden Axe

Bot used in Suisei-CN related TG groups (No-nonsense and OT). Main purpose is to manage group member's title.

## Config

Configurations are passin in via environment variable. For better debugging experience, `.env` file is used.

### `GOLDEN_AXE_LOG`

Log level, case insensitive

**Type**: `String`

**Required**: `true`

**Possible values**: `0` = `OFF`, `1` = `ERROR`, `2` = `WARN`, `3` = `INFO`, `4` = `DEBUG`, `5` = `TRACE`, `error`, `warn`, `info`, `debug`, `trace`, `off`

**Default value**: `info`

### `GOLDEN_AXE_TOKEN`

Telegram bot token. This should be kept confidential.

**Type**: `String`

**Required**: `true`

### `GOLDEN_AXE_DEBUG_CHAT`

Chat id of debugging telegram group. This should be kept confidential.

**Type**: `i64`

**Required**: `false`

## Develop

- `nightly` version of rustc is required.
- Use `cargo run` to start in debug mode directly.
- `.env` is optional but may be useful for debugging.

## Deploy

The bot is being deployed onto `fly.io` on each push to master
