# psu-functional-bot

Bot to handle operations on my PSU functional programming server

## Setup

1. Create a bot in the discord developer portal and copy the token
2. Invite with bot scope and these permissions: **Manage Roles**,
   **Add Reactions**, **Read Message History**, **View Channel**,
   **Manage Messages** (that last one is for clearing stray reactions)
3. In server settings, drag the bot's own role above every role it hands out.
4. Turn on Developer Mode in Discord (Settings -> Advanced) so you can
   right-click to copy message and role ids.
5. Fill in `roles.txt`: one `<channel_id> <message_id> <emoji> <role_id>`
   per line. On startup the bot adds each emoji to its message itself, so
   members only have to click.

## Run

```powershell
cargo run
cargo test
```

The first run asks for the token in the console. Paste it, press Enter, and it
is saved to `token.txt` and reused after that. Pasted the wrong one? Delete
`token.txt` and run again.

`token.txt` is plain text and is gitignored, whoever has that token controls
the bot, so don't commit or share it.

`roles.txt` is read once at startup, so restart the bot after editing it.

Once an hour the bot clears any reaction on those messages that no rule
lists, so the only things members can click are the ones that hand out a
role.
