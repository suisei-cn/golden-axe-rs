# Golden Axe

Bot used in Suisei-CN related TG groups (No-nonsense and OT). Main purpose is to manage group member's title.

## Config

Configurations are passin in via environment variable. For better debugging experience, `.env` file is used.

### Variables

- `TELOXIDE_TOKEN` (Required) - Telegram bot token. This should be kept confidential.

- `BOT_MODE` (Default: `POLL`) - Telegram bot get update mode, case insensitive. Available values: `poll`, `webhook`

- `DOMAIN` - When using `webhook`, this is required to setup the webhook endpoint.

- `DEBUG_GROUP_ID` - Chat id of debugging telegram group. This should be kept confidential.

- `RUST_LOG` (Default: `INFO`) - Log level, case insensitive. Available values: `0` = OFF, `1` = ERROR, `2` = WARN, `3` = INFO, `4` = DEBUG, `5` = TRACE, `error`, `warn`, `info`, `debug`, `trace`, `off`.

## Develop

Use `cargo run` directly. `.env` is optional but may be useful.
