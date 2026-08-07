# rouratui-slack-bridge

One orchestrating rouratui bot on Slack, reachable from any channel it's
invited to (reply in-thread after an `@mention`) or by direct message,
restricted to a family allowlist. Built on the same `BuiltRuntime` the
interactive CLI and `chat-server` use.

## How it routes messages

- **DMs**: every message from an allowed user is handled; one continuous
  conversation per DM channel.
- **Channels**: a top-level message is only handled if it `@mentions` the
  bot — that starts a new Slack thread. Any reply inside that thread is
  then handled without needing to re-mention the bot. Messages in threads
  the bot isn't already part of, and channel messages that don't mention
  it, are ignored — so it doesn't talk over unrelated conversation in a
  shared family channel.
- Tool calls that need escalation beyond workspace-write pause and post an
  approval request into the thread; replying "approve" or "deny" in that
  same thread resolves it (identical mechanism to `chat-server`'s Open
  WebUI bridge).

## One-time Slack app setup (not scriptable — do this in api.slack.com)

1. Create an app at <https://api.slack.com/apps> (or reuse an existing
   Roura.io one) "From scratch", in your workspace.
2. **Socket Mode**: Settings → Socket Mode → enable it. This generates an
   app-level token — copy it (`xapp-...`, scope `connections:write`).
3. **OAuth & Permissions** → Bot Token Scopes, add:
   - `chat:write` (post replies)
   - `users:read.email` (resolve the family allowlist by email)
   - `im:history`, `channels:history`, `groups:history`, `mpim:history`
     (receive messages)
   Install the app to the workspace, copy the Bot User OAuth Token
   (`xoxb-...`).
4. **Event Subscriptions** → enable, then subscribe to bot events:
   `message.im`, `message.channels`, `message.groups`, `message.mpim`.
5. Invite the bot to whatever channels it should be reachable from
   (`/invite @rouratui` in each channel). DMs work automatically once the
   app is installed.

## Running

```
SLACK_BOT_TOKEN=xoxb-... \
SLACK_APP_TOKEN=xapp-... \
rouratui-slack-bridge
```

| Env var | Default | Purpose |
| --- | --- | --- |
| `SLACK_BOT_TOKEN` | — (required) | Bot token, `xoxb-...` |
| `SLACK_APP_TOKEN` | — (required) | App-level token, `xapp-...` |
| `SLACK_ALLOWED_EMAILS` | Chris, Bill, Carito, Susan's addresses | Comma-separated allowlist, resolved to Slack user IDs at startup |
| `ROURATUI_SLACK_MODEL` | `qwen3.6:27b-coding-bf16` | Ollama model tag |
| `OLLAMA_HOST` | `http://127.0.0.1:11434` | Set explicitly to be certain which Ollama instance is used |

Tokens are read from the environment only — never commit them. Refusing
to start with zero resolved allowlist emails is intentional: it prevents
an open bot from silently coming up unrestricted if the emails typo'd or
none of them have Slack accounts yet.

On any WebSocket error the process reconnects with backoff (2s, doubling,
capped at 60s) rather than exiting — matches `netwatch`'s "a long-running
background job shouldn't crash-loop on a blip" precedent elsewhere in this
repo.
