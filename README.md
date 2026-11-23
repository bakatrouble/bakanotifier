# bakanotifier

Small HTTP service that sends text or templated rich messages to Telegram through
the Bot API.

## Build and run

```sh
cargo build --release
cp config.example.toml config.toml
cargo run --release -- --config config.toml
```

`--config` defaults to `config.toml`. Logging is configured through `RUST_LOG`
(for example `RUST_LOG=debug`); the default level is `info`.

## Configuration

The configuration file is TOML and rejects unknown fields. It is loaded with
clapfig; strict-mode errors name the file and key.

| Key                 | Required | Description                                                        |
| ------------------- | -------- | ------------------------------------------------------------------ |
| `listen`            | no       | Address to bind; CLI default `0.0.0.0:8080`.                        |
| `bot_token`         | yes*     | Telegram bot token. Never logged. TOML or `--bot-token`.           |
| `default_chat_id`   | no       | Chat id used when `/notify` does not provide `chat_id`.            |
| `templates_dir`     | no       | Template directory; default `templates`; must exist.               |
| `telegram_api_url`  | no       | Telegram API base URL, default `https://api.telegram.org`.         |
| `api_secrets`       | yes      | Non-empty list of bearer tokens accepted by `/notify`.             |
| `otlp`              | no       | OTLP trace export settings; see below.                             |

`bot_token` and `templates_dir` have no built-in defaults; set them in TOML or
with their flags. Startup fails if `bot_token` is empty, `api_secrets` is empty or contains an
empty secret, `telegram_api_url` is empty, or `templates_dir` is not a directory.

Example:

```toml
listen = "0.0.0.0:8080"
bot_token = "123456:ABC-DEF"
default_chat_id = 123456789
templates_dir = "templates"
telegram_api_url = "https://api.telegram.org"
api_secrets = ["change-me"]

# [otlp]
# endpoint = "http://collector:4318"
# service_name = "bakanotifier"
# sampling_ratio = 1.0
```

## Configuration precedence

The configuration file is TOML and rejects unknown top-level keys. Values can
also come from environment variables and flags. Precedence is:

1. CLI flag
2. Environment variable
3. TOML file
4. Built-in default

| Flag                 | Environment variable | TOML key            | Built-in default             |
| -------------------- | -------------------- | ------------------- | ---------------------------- |
| `--config`           | none                 | not stored in TOML  | `config.toml`                |
| `--listen`           | `BN__LISTEN`         | `listen`            | `0.0.0.0:8080`               |
| `--bot-token`        | `BN__BOT_TOKEN`      | `bot_token`         | none                         |
| `--default-chat-id`  | `BN__DEFAULT_CHAT_ID`| `default_chat_id`   | none                         |
| `--templates-dir`    | `BN__TEMPLATES_DIR`  | `templates_dir`     | `templates`                  |
| `--telegram-api-url` | `BN__TELEGRAM_API_URL`| `telegram_api_url`  | `https://api.telegram.org`   |

`api_secrets` and the `otlp` section are TOML-only, but nested OTLP keys also
work through the double-underscore env mapping, for example
`BN__OTLP__ENDPOINT` or `BN__OTLP__SAMPLING_RATIO`. There is no environment
variable for `--config`; pass the file path explicitly.

`BN__BOT_TOKEN` is the recommended way to supply the token: it avoids shell
history and process listings. The TOML file also works; passing `--bot-token`
puts the token in shell history. The value is hidden from `--help` and is never
logged.

Example:

```sh
BN__LISTEN=127.0.0.1:9000 cargo run -- --config config.example.toml
```

### YAML parsing

Template files, request bodies and values files are YAML/JSON and use
`serde-saphyr`. Duplicate keys and non-finite floats are
errors. Leading-zero and underscored numeric spellings such as `0755` and
`1_000` deliberately stay strings in untyped values, and null is not converted
to a string (`null_as_string` remains false). Booleans are strict YAML 1.2:
only `true`/`false` are booleans, while `yes`/`no`/`on`/`off` stay strings.
Unsupported tags are rejected. Mapping keys are converted to strings when parsed
into JSON: `1:` becomes `"1"` and `true:` becomes `"true"`. A `null:` key is
rejected. If both `1:` and `"1":` appear in one mapping, YAML treats them as
distinct keys while JSON sees one key; the later value wins. Duplicate keys with
the same YAML identity are still an error.

## Config schema

`schemas/config.schema.json` is generated from the config types in
`src/config.rs` during every build. It is written only when its content
changes, embedded as `bakanotifier::schemas::CONFIG_SCHEMA`, and checked by a
drift test. `config.example.toml` and `config.docker.example.toml` reference it
with `#:schema ./schemas/config.schema.json`. The library embeds the source-tree
file at compile time, so a published package must include
`schemas/config.schema.json`.

Template `values` declarations remain JSON Schema documents used for runtime
validation; they do not generate editor schema files.

## OTLP tracing

Spans are sent with OTLP/HTTP using protobuf payloads over the reqwest client.
No exporter is created unless an endpoint is configured, so running without
the `otlp` section and without OTLP environment variables leaves behavior
unchanged. The standard `tracing-subscriber` log output stays enabled.

The `otlp` section supports:

| Key              | Required | Description                                                  |
| ---------------- | -------- | ------------------------------------------------------------ |
| `endpoint`       | no       | OTLP/HTTP base URL, for example `http://collector:4318`.      |
| `service_name`   | no       | Resource service name, default `bakanotifier`.               |
| `sampling_ratio` | no       | Root trace sampling ratio from `0.0` to `1.0`, default `1.0`. |

Environment variables and precedence:

| Variable                        | Meaning                                                        |
| ------------------------------- | -------------------------------------------------------------- |
| `OTEL_EXPORTER_OTLP_ENDPOINT`   | Enables export and overrides `otlp.endpoint`.                  |
| `OTEL_EXPORTER_OTLP_PROTOCOL`   | Must be unset or `http/protobuf`; other values fail startup.    |
| `OTEL_SERVICE_NAME`             | Overrides `otlp.service_name`.                                 |

Precedence is: `OTEL_EXPORTER_OTLP_ENDPOINT` over `otlp.endpoint`;
`OTEL_SERVICE_NAME` over `otlp.service_name` over the `bakanotifier` default.
The sampling ratio is read from the config only. An empty endpoint or service
name environment variable is treated as unset.

Both `POST /notify` and `GET /health` HTTP requests produce a server span
(`http.server`) with method, route and status. Notify handling adds a `notify`
span with `chat_id` and `template` attributes, a `template.render` span, and a
`telegram.send_message` or `telegram.send_rich_message` child span. Message
text, template values, the bot token and API secrets are never recorded on
spans. W3C `traceparent` headers are extracted from incoming
requests. Spans are flushed and the tracer provider is shut down on ctrl_c or
SIGTERM.

## Docker

The image uses Alpine at runtime and runs as a non-root user. The binary is
built on musl, and the image contains only `ca-certificates` in addition to the
binary, templates and an example config. It exposes port `8080`.

```sh
docker build -t bakanotifier:local .
```

The container reads `/etc/bakanotifier/config.toml` by default, and templates
are expected at `/etc/bakanotifier/templates` (see
`config.docker.example.toml`):

```sh
cp config.docker.example.toml config.toml
# edit config.toml with a placeholder token and secret
docker run --rm -p 8080:8080 \
  -v "$PWD/config.toml:/etc/bakanotifier/config.toml:ro" \
  -v "$PWD/templates:/etc/bakanotifier/templates:ro" \
  bakanotifier:local
```

The image healthcheck runs `bakanotifier healthcheck --address
127.0.0.1:8080`, which calls the local `/health` endpoint without needing curl.
Because `/health` checks Telegram connectivity, the container is reported
unhealthy while Telegram is unreachable.

## Docker Compose

Compose starts bakanotifier, an OpenTelemetry Collector and Jaeger. The app
reads `./config.toml` and `./templates`; the collector receives OTLP on ports
4317 and 4318 inside the Compose network and exports traces to Jaeger.

```sh
cp config.docker.example.toml config.toml
# edit config.toml with a placeholder token and secret
docker compose up --build
```

Then open the Jaeger UI at <http://localhost:16686> and call the service on
<http://localhost:8080/notify>. Only port 8080 (service) and 16686 (Jaeger UI)
are published. Secrets stay in `config.toml`, not in the Compose file.

## HTTP API

### `GET /health`

This handler does not call Telegram. A background task calls `getMe` immediately
on startup and then every 60 seconds, and records the result in shared state.
The handler reports:

- `200` with `"status": "ok"` when the latest check succeeded and the last
  success is no older than two intervals (120 seconds).
- `503` with `"status": "starting"` when no check has completed yet.
- `503` with `"status": "down"` after a failed check or when the last success is
  stale.

Response shape:

```json
{
  "status": "ok",
  "telegram": {
    "reachable": true,
    "last_checked_unix": 0,
    "last_ok_unix": 0,
    "latency_ms": 12,
    "last_error": null
  }
}
```

The bot token is redacted from `last_error`. Unknown routes return
`404 {"error":"not found"}`.

### `POST /notify`

Requires `Authorization: Bearer <secret>` with one of `api_secrets`. Secrets are
compared in constant time. Missing or invalid credentials return `401`.

The request body only selects a template and supplies its values:

```json
{
  "chat_id": 123,
  "template": "text",
  "values": {"message": "hello"}
}
```

| Field      | Required | Description                                                     |
| ---------- | -------- | --------------------------------------------------------------- |
| `chat_id`  | no       | Falls back to `default_chat_id`; `400` if neither is available. |
| `template` | yes      | Template name without extension; must match `^[A-Za-z0-9_-]+$`. |
| `values`   | no       | Object passed to the template; default `{}`.                     |

Unknown fields return `400`. `values` that is not an object returns `400`. The
template's `mode` decides whether Telegram receives `sendMessage` or
`sendRichMessage`; the request has no `parse_mode` or `message` field.

The body may be JSON or YAML, selected by `Content-Type`:

- `application/json` and `application/*+json` are parsed as JSON.
- `application/yaml`, `application/x-yaml`, `text/yaml` and
  `application/*+yaml` are parsed as YAML. The same YAML restrictions as
  templates apply: YAML tags return `400`, and mapping keys are coerced to
  strings the same way as templates.
- A missing `Content-Type` returns `400` with a clear message.
- Any other `Content-Type` returns `415`.

Parse errors return `400` with the parser message. The request body limit is
1 MiB.

Success response:

```json
{"ok": true, "message_id": 1, "chat_id": 123}
```

Error response:

```json
{"ok": false, "error": "template not found"}
```

Status codes:

- `400` invalid request body, missing `template`, invalid template name, YAML
  syntax error, render error, or a rendered document that does not match the
  expected template shape.
- `401` missing or invalid bearer token.
- `404` template file does not exist.
- `415` unsupported request `Content-Type`.
- `500` template file cannot be read.
- `502` Telegram API error; the response carries the Telegram error description
  with the bot token redacted if present.

### Examples

```sh
curl -H "Authorization: Bearer change-me" \
  -H "Content-Type: application/json" \
  -d '{"template": "text", "values": {"message": "hello"}}' \
  http://localhost:8080/notify
```

```sh
curl -H "Authorization: Bearer change-me" \
  -H "Content-Type: application/json" \
  -d '{"template": "html", "values": {"message": "<b>hi</b>"}}' \
  http://localhost:8080/notify
```

```sh
curl -H "Authorization: Bearer change-me" \
  -H "Content-Type: application/yaml" \
  --data-binary 'template: text
values:
  message: hello' \
  http://localhost:8080/notify
```

```sh
curl -H "Authorization: Bearer change-me" \
  -H "Content-Type: application/json" \
  -d '{"chat_id": 123, "template": "example", "values": {"title": "Disk alert", "items": ["sda 97%", "sdb 12%"], "urgent": true}}' \
  http://localhost:8080/notify
```

## Rich templates

Rich templates live in `templates_dir/<name>.yaml` (`.yml` is also accepted) and
are [JSON-e](https://json-e.js.org) templates written in YAML. The file is
parsed into a JSON value, then JSON-e renders the whole document. The result
must be a JSON object with a `mode`, a `text` body, and optionally
`skip_entity_detection`:

```yaml
mode: rich_html
text: "<b>Disk alert</b>"
skip_entity_detection: true
```

`mode` is required and selects the Telegram method:

| Mode            | Telegram call     | Formatting                         |
| --------------- | ----------------- | ---------------------------------- |
| `text`          | `sendMessage`     | no parse mode                      |
| `html`          | `sendMessage`     | `ParseMode::Html`                  |
| `markdown`      | `sendMessage`     | `ParseMode::MarkdownV2`            |
| `rich_html`     | `sendRichMessage` | `InputRichMessage.html`            |
| `rich_markdown` | `sendRichMessage` | `InputRichMessage.markdown`        |
| `rich_blocks`   | `sendRichMessage` | `InputRichMessage.blocks`          |

`text` is required for all modes except `rich_blocks`. `rich_blocks` requires
`blocks` and must not set `text` or `skip_entity_detection`; its blocks are
validated against frankenstein's `InputRichBlock` types at render time.
`skip_entity_detection` is optional and only valid for rich modes. `is_rtl` is
optional and only valid for rich modes. Unknown fields are rejected.
`sendMessage` does not set link-preview options; there is no template field for
that.

Shipped default templates take a required `values.message` string and render it
unchanged:

- `templates/text.yaml` with mode `text`
- `templates/html.yaml` with mode `html`
- `templates/markdown.yaml` with mode `markdown`

```yaml
mode: text
text: "${message}"
```

Because JSON-e is strict, missing `values.message` returns `400`.

Template files are YAML, but request `values` and the files under
`examples/values/` stay JSON because `/notify` accepts JSON. YAML syntax errors
return `400`, and missing templates return `404`.

Rich messages are sent with `sendRichMessage`, not `sendMessage`. See the
Telegram documentation:

- <https://core.telegram.org/bots/api#rich-messages>
- <https://core.telegram.org/bots/api/rich-messages.md>
- <https://core.telegram.org/bots/api/methods/sendrichmessage.md>
- <https://core.telegram.org/bots/api/types/inputrichmessage.md>

Rendering is structural, so a value containing quotes, backslashes or newlines
stays a plain string in the output; there is no textual JSON escaping step.
The request `values` object is the JSON-e context. Undefined references and
operator errors produce a `400`.

JSON-e operators available in templates include:

- `${expr}` interpolation inside strings (for example `"<b>${title}</b>"`).
- `$eval` to evaluate an expression and produce any JSON value.
- `$if` to include a value only when a condition holds, optionally with `then`
  and `else`.
- `$map` to transform a list, `$flatten` to flatten nested lists.
- `$merge` and `$mergeDeep` to merge objects or arrays.
- `$switch` for multi-branch selection.
- `$let`, `$json`, `$fromNow`, and other operators documented at
  <https://json-e.js.org>.
- `$$` escapes a literal `$` in strings (for example `"$$5"` renders `"$5"`).

Example `templates/example.yaml`:

```yaml
mode: rich_html
text:
  $let:
    header: "<b>${title}</b>"
    lines:
      $map:
        $eval: items
      each(item): "- ${item}"
  in:
    $if: len(lines)
    then:
      $eval: "header + '\n' + join(lines, '\n')"
    else:
      $eval: header
skip_entity_detection:
  $if: urgent
  then: true
```

### Declaring values

A template can declare the values object it accepts with a top-level `values`
key containing a JSON Schema (draft 2020-12) object:

```yaml
values:
  type: object
  properties:
    message:
      type: string
  required:
    - message
  additionalProperties: false
```

`values` is a JSON Schema document used for runtime validation; it is not a
generated artifact. It is metadata: it is removed before JSON-e rendering and
never reaches the rendered output. If it is absent, the template accepts any object. `/notify`
and the local `send_template` example validate the request values against this
schema after loading the template and before rendering. Validation failures
return `400` with the instance path and validator message for each violation.

### Kitchen-sink templates

Two templates exercise most rich formatting features:

- `templates/rich-html-kitchen-sink.yaml` with mode `rich_html` and
  `examples/values/rich-html-kitchen-sink.json`.
- `templates/rich-markdown-kitchen-sink.yaml` with mode `rich_markdown` and
  `examples/values/rich-markdown-kitchen-sink.json`.

The small `templates/rich-blocks-example.yaml` demonstrates `rich_blocks` with a
heading, paragraph, list built from `values.items`, optional footer and divider;
its values are in `examples/values/rich-blocks-example.json`.

Render any of them locally with the example binary:

```sh
cargo run --example send_template -- \
  --template rich-html-kitchen-sink \
  --values examples/values/rich-html-kitchen-sink.json
```

### Telegram limits

Telegram enforces these rich message limits; bakanotifier does not validate them
locally:

- up to 32768 UTF-8 characters, including custom emoji alternative text and
  formula source;
- up to 500 blocks, including nested blocks, list items, table rows, quotation
  blocks and details blocks;
- up to 16 levels of nested formatting and blocks;
- up to 50 media attachments;
- up to 20 columns in a table.

## Sending a test template

The `send_template` example renders a template exactly as the server does. With
`--dry-run` it prints the Telegram method and payload without contacting
Telegram or needing a token:

```sh
cargo run --example send_template -- \
  --config config.example.toml \
  --template example \
  --values examples/values/example.json \
  --dry-run
```

Without `--dry-run` it sends directly to Telegram using the bot token,
`telegram_api_url` and `default_chat_id` from the config or `BN__` environment
variables (`BN__BOT_TOKEN`, `BN__DEFAULT_CHAT_ID`, `BN__TEMPLATES_DIR`,
`BN__TELEGRAM_API_URL`). `--chat-id` overrides the default chat id and
`--templates-dir` overrides the configured templates directory. The token is
never printed. Dry-run does not require a config file or token.

```sh
cargo run --example send_template -- \
  --config config.example.toml \
  --template example \
  --values examples/values/example.yaml
```

`--values` takes an existing file path. `.json` files are parsed as JSON,
`.yaml`/`.yml` as YAML, and other extensions try JSON then YAML. Values must be
an object. The same YAML restrictions as `/notify` apply.
