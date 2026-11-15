# Bard Account Linking

Bard Account Linking is a Next.js application that enables users to connect their Spotify accounts to Bard. It shares the bot's PostgreSQL database and stores the Spotify access and refresh tokens there.

The app requests permission to read the linked user's playback state and control playback on that user's Spotify devices. It does not stream audio.

## Environment

|             Variable             | Description                                                                                                                                                           |
| :------------------------------: | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
|    DISCORD_TOKEN<sup>1</sup>     | Any Discord bot token (can be different from the Bard bot), for resolving user information                                                                             |
|     DATABASE_URL<sup>2</sup>     | A PostgreSQL database URL, which must point to the same database as your Bard instance                                                                                 |
|        SPOTIFY_CLIENT_ID         | Your Spotify app Client ID                                                                                                                                            |
|      SPOTIFY_CLIENT_SECRET       | Your spotify app Client Secret                                                                                                                                        |
| SPOTIFY_REDIRECT_URI<sup>3</sup> | The full URI to redirect to after Spotify authentication (for local development: `http://127.0.0.1:3000/authorize`) |

### Important notes

- <sup>1</sup>: While not required, you may use a different Discord application than the one used for hosting the bot
- <sup>2</sup>: Make sure you use the same PostgreSQL database as your Bard instance
- <sup>3</sup>: Insecure redirect URIs are no longer allowed in Spotify, this includes localhost. You are still allowed to use 127.0.0.1 without https.

## Local development

Set the variables above in `.env.local`, install dependencies with `pnpm install`, then run `pnpm dev`. Set the bot's `LINK_URL` to `http://127.0.0.1:3000` while developing locally. The Spotify application must allow the exact redirect URI in its dashboard.

When changing requested Spotify permissions, users who already linked an account need to run `/unlink` and `/link` again to grant the new scopes.
