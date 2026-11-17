use anyhow::Result;
use log::{info, warn};
use poise::{Framework, FrameworkOptions};
use serenity::all::{Context as SerenityContext, Ready, UserId};
use spoticord_database::Database;

use crate::commands;

pub type Context<'a> = poise::Context<'a, Data, anyhow::Error>;
pub type FrameworkError<'a> = poise::FrameworkError<'a, Data, anyhow::Error>;

type Data = Database;

pub const OWNER_ID: u64 = 682_288_992_456_409_096;
pub const RESTART_NOTICE_PATH: &str = "/tmp/bard-restart-notice";

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

    send_restart_notice(ctx).await;

    Ok(database)
}

async fn send_restart_notice(ctx: &SerenityContext) {
    let action = match tokio::fs::read_to_string(RESTART_NOTICE_PATH).await {
        Ok(action) => action,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => {
            warn!("Could not read the pending restart notice: {error}");
            return;
        }
    };

    let message = match action.trim() {
        "update" => "✅ Bard is back online. The update finished and the new version started successfully. Use /status to check it.",
        _ => "✅ Bard has restarted and is back online. Use /status to check its uptime and server details.",
    };
    let result = async {
        let owner = ctx.http.get_user(UserId::new(OWNER_ID)).await?;
        let channel = owner.create_dm_channel(&ctx.http).await?;
        channel.say(&ctx.http, message).await?;
        tokio::fs::remove_file(RESTART_NOTICE_PATH).await?;
        Ok::<(), anyhow::Error>(())
    }
    .await;

    if let Err(error) = result {
        warn!("Could not deliver Bard's restart-complete notice: {error}");
    }
}

