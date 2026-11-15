use anyhow::Result;
use poise::CreateReply;
use reqwest::Method;

use crate::bot::Context;

/// Pause playback on your Spotify account
#[poise::command(slash_command)]
pub async fn pause(ctx: Context<'_>) -> Result<()> {
    action(ctx, Method::PUT, "pause", "Paused Spotify playback.").await
}

/// Resume playback on your Spotify account
#[poise::command(slash_command)]
pub async fn resume(ctx: Context<'_>) -> Result<()> {
    action(ctx, Method::PUT, "play", "Resumed Spotify playback.").await
}

/// Skip to the next item on your Spotify account
#[poise::command(slash_command)]
pub async fn skip(ctx: Context<'_>) -> Result<()> {
    action(
        ctx,
        Method::POST,
        "next",
        "Skipped to the next Spotify track.",
    )
    .await
}

/// Go back to the previous item on your Spotify account
#[poise::command(slash_command)]
pub async fn previous(ctx: Context<'_>) -> Result<()> {
    action(
        ctx,
        Method::POST,
        "previous",
        "Went back to the previous Spotify track.",
    )
    .await
}

async fn action(
    ctx: Context<'_>,
    method: Method,
    endpoint: &str,
    success_message: &str,
) -> Result<()> {
    let message = match super::api::request(&ctx, method, endpoint).await {
        Ok(_) => success_message.to_owned(),
        Err(error) => error.to_string(),
    };

    ctx.send(CreateReply::default().content(message).ephemeral(true))
        .await?;
    Ok(())
}
