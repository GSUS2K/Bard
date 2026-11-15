mod env;

use rspotify::{AuthCodeSpotify, Config, Credentials, OAuth, Token};
use serenity::all::GatewayIntents;

#[cfg(not(debug_assertions))]
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(debug_assertions)]
pub const VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), "-dev");

pub fn discord_token() -> &'static str {
    &env::DISCORD_TOKEN
}

pub fn discord_intents() -> GatewayIntents {
    GatewayIntents::GUILDS | GatewayIntents::GUILD_VOICE_STATES
}

pub fn database_url() -> &'static str {
    &env::DATABASE_URL
}

pub fn link_url() -> &'static str {
    &env::LINK_URL
}

pub fn music_dir() -> &'static str {
    &env::MUSIC_DIR
}

pub fn get_spotify(token: Token) -> AuthCodeSpotify {
    AuthCodeSpotify::from_token_with_config(
        token,
        Credentials {
            id: env::SPOTIFY_CLIENT_ID.to_string(),
            secret: Some(env::SPOTIFY_CLIENT_SECRET.to_string()),
        },
        OAuth::default(),
        Config::default(),
    )
}
