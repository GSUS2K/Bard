# Bard

Bard is a Discord music bot with local voice playback and optional Spotify account controls. Slash commands can read a linked user's current track and control playback on that user's Spotify devices. Local audio files can be uploaded from Discord and played in a voice channel. Spotify audio itself is not relayed into Discord voice.

The bot is based on archived Spoticord source and its code is licensed under AGPL-3.0. The existing MIT license from the Bard repository is preserved separately as `LICENSE-MIT`. Previous Spotify voice player code remains in the repository as reference material, but is no longer part of the Cargo workspace. Spotify commands use the documented Web API.

## Discord commands

- `/link` — connect your Spotify account.
- `/nowplaying` — show your current track or episode, with a link to Spotify.
- `/pause`, `/resume`, `/skip`, `/previous` — control your Spotify playback.
- `/playlists` — show your Spotify playlists with links.
- `/unlink` — remove your Spotify account from Bard.
- `/listfiles` — list local audio files available to the voice player.
- `/uploadfile` — save an attached audio file from Discord into the bot's music folder (up to 25 MB); optionally provide `save_as` to rename it while uploading (the original extension is added if omitted).
- `/playfile` — choose a local audio file from autocomplete, or type its exact name, to play it in your current voice channel.
- `/voicepanel` — open a private control panel with file selection, a queue preview, playback controls, and rename/delete actions.
- `/renamefile` — rename a library file; `/uploadfile` also accepts `save_as` to choose a name during upload.
- `/deletefile` — uploaders can delete their own files; members with Manage Server or Administrator can delete any file.
- `/stopvoice` and `/leavevoice` — stop local audio or disconnect the voice player.
- `/status`, `/restart`, and `/update` — top-level, owner-only status, restart, and fast-forward update commands. Restart/update DM the owner after Bard reconnects.

For local voice playback, add audio files you own or are licensed to share to the folder in `MUSIC_DIR` (default: `music`). The bot must run on a computer that can access those files. `/playfile` joins your voice channel and replaces any current local audio.

Playback controls require Spotify Premium, an active Spotify device, and authorization for the playback scopes requested by the link app. Playlist access uses your Spotify library permissions. If an account was linked before these scopes were added, run `/unlink` and `/link` again.

## Run locally

You need Rust 1.83 or newer, PostgreSQL, a Discord bot application, and a Spotify developer application. Set up the account-linking web app in [`link-app`](link-app/README.md) first; it shares the PostgreSQL database with the bot. On Windows, install Visual Studio Build Tools and CMake for Songbird's Opus dependency.

Create a `.env` file in the repository root:

```env
DISCORD_TOKEN=your-discord-bot-token
DATABASE_URL=postgresql://postgres:password@localhost/spoticord
LINK_URL=http://127.0.0.1:3000
SPOTIFY_CLIENT_ID=your-spotify-client-id
SPOTIFY_CLIENT_SECRET=your-spotify-client-secret
GUILD_ID=your-test-discord-server-id
MUSIC_DIR=music
```

`GUILD_ID` is used for fast slash-command registration in debug builds. In production, commands are registered globally. Configure the same Spotify client ID and secret in the link app, and allow its `SPOTIFY_REDIRECT_URI` in the Spotify developer dashboard.
`MUSIC_DIR` can be an absolute path, such as `C:\\Users\\you\\Music\\Discord`; use `music` to keep files in a folder beside the bot.

For owner restart and updates, run the bot under a supervisor (for example, systemd) that restarts it when it exits. Set `BOT_SUPERVISED=true`, `UPDATE_REMOTE` to your Bard GitHub remote name, and `UPDATE_BRANCH` (default `main`). `/update` refuses to change a dirty checkout, fetches and fast-forwards that branch, builds the release binary, then exits so the supervisor starts the new binary. The only allowed admin user is Discord ID `682288992456409096`. See [`deploy/bard.service`](deploy/bard.service) for a systemd example. This source checkout workflow is for a VPS running the bot from its Git checkout; it does not update a prebuilt Docker image.

Start PostgreSQL and the link app, then run the bot:

```powershell
cargo run
```

The first run creates the database tables. Invite the bot with application commands permissions, then use `/link` in Discord to connect your Spotify account.

## Account linking

The Next.js account-linking app lives in [`link-app`](link-app/README.md). It must use the same PostgreSQL database as the bot and request these Spotify scopes:

- `user-read-private`
- `user-read-playback-state`
- `user-read-currently-playing`
- `user-modify-playback-state`
- `playlist-read-private`
- `playlist-read-collaborative`

The link app stores the Spotify access and refresh tokens in the `account` table. Keep the database private and do not commit `.env` files or tokens.

