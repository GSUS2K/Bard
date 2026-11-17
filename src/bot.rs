use anyhow::Result;
use log::info;
use poise::{Framework, FrameworkOptions};
use serenity::all::{Context as SerenityContext, Ready};
use spoticord_database::Database;

use crate::commands;

pub type Context<'a> = poise::Context<'a, Data, anyhow::Error>;
pub type FrameworkError<'a> = poise::FrameworkError<'a, Data, anyhow::Error>;

type Data = Database;

pub fn framework_opts() -> FrameworkOptions<Data, anyhow::Error> {
    poise::FrameworkOptions {
        commands: vec![
            commands::admin::status(),
            commands::admin::restart(),
            commands::admin::update(),
            #[cfg(debug_assertions)]
            commands::debug::ping(),
            commands::core::help(),
            commands::core::version(),
            commands::core::link(),
            commands::core::unlink(),
            commands::music::nowplaying(),
            commands::music::pause(),
            commands::music::resume(),
            commands::music::skip(),
            commands::music::previous(),
            commands::music::playlists(),
            commands::music::uploadfile(),
            commands::music::playfile(),
            commands::music::queuefile(),
            commands::music::renamefile(),
            commands::music::skiplocal(),
            commands::music::localqueue(),
            commands::music::bard(),
            commands::music::listfiles(),
            commands::music::deletefile(),
            commands::music::voicepanel(),
            commands::music::stopvoice(),
            commands::music::leavevoice(),
        ],
        ..Default::default()
    }
}

pub async fn setup(
    ctx: &SerenityContext,
    ready: &Ready,
    framework: &Framework<Data, anyhow::Error>,
    database: Database,
) -> Result<Data> {
    info!("Successfully logged in as {}", ready.user.name);
    ctx.set_activity(Some(serenity::all::ActivityData::listening(
        "/voicepanel · local music",
    )));

    #[cfg(debug_assertions)]
    poise::builtins::register_in_guild(
        ctx,
        &framework.options().commands,
        std::env::var("GUILD_ID")?.parse()?,
    )
    .await?;

    #[cfg(not(debug_assertions))]
    poise::builtins::register_globally(ctx, &framework.options().commands).await?;

    Ok(database)
}

