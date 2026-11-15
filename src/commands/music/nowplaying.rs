use anyhow::Result;
use poise::CreateReply;
use reqwest::Method;
use serenity::all::{CreateEmbed, CreateEmbedFooter};
use spoticord_utils::discord::Colors;

use crate::bot::Context;

/// Show the track currently playing on your Spotify account
#[poise::command(slash_command)]
pub async fn nowplaying(ctx: Context<'_>) -> Result<()> {
    let response = match super::api::request(&ctx, Method::GET, "currently-playing").await {
        Ok(response) => response,
        Err(error) => return send_error(ctx, error.to_string()).await,
    };

    if response.status() == reqwest::StatusCode::NO_CONTENT {
        ctx.send(
            CreateReply::default()
                .content("Spotify isn't playing anything right now.")
                .ephemeral(true),
        )
        .await?;
        return Ok(());
    }

    let body: serde_json::Value = response.json().await?;
    let Some(item) = body.get("item").filter(|item| !item.is_null()) else {
        ctx.send(
            CreateReply::default()
                .content("Spotify didn't return a current track.")
                .ephemeral(true),
        )
        .await?;
        return Ok(());
    };

    let name = item
        .get("name")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("Unknown title");
    let artists = item
        .get("artists")
        .and_then(serde_json::Value::as_array)
        .map(|artists| {
            artists
                .iter()
                .filter_map(|artist| artist.get("name").and_then(serde_json::Value::as_str))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .filter(|artists| !artists.is_empty())
        .unwrap_or_else(|| {
            item.get("show")
                .and_then(|show| show.get("name"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("Spotify")
                .to_owned()
        });
    let item_type = item
        .get("type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("track");
    let spotify_url = item
        .get("external_urls")
        .and_then(|urls| urls.get("spotify"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .or_else(|| {
            item.get("id")
                .and_then(serde_json::Value::as_str)
                .map(|id| match item_type {
                    "episode" => format!("https://open.spotify.com/episode/{id}"),
                    _ => format!("https://open.spotify.com/track/{id}"),
                })
        });

    let mut embed = CreateEmbed::new()
        .title(name)
        .description(format!("by {artists}"))
        .footer(CreateEmbedFooter::new("Now playing on Spotify"))
        .color(Colors::Info);

    if let Some(url) = spotify_url {
        embed = embed.url(url);
    }
    if let Some(album) = item
        .get("album")
        .and_then(|album| album.get("name"))
        .and_then(serde_json::Value::as_str)
    {
        embed = embed.field("Album", album, true);
    } else if let Some(show) = item
        .get("show")
        .and_then(|show| show.get("name"))
        .and_then(serde_json::Value::as_str)
    {
        embed = embed.field("Show", show, true);
    }
    if let Some(image) = item
        .get("album")
        .and_then(|album| album.get("images"))
        .and_then(serde_json::Value::as_array)
        .and_then(|images| images.first())
        .and_then(|image| image.get("url"))
        .and_then(serde_json::Value::as_str)
    {
        embed = embed.thumbnail(image);
    }

    let playing = body
        .get("is_playing")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    ctx.send(CreateReply::default().embed(embed).content(if playing {
        "▶ Playing"
    } else {
        "⏸ Paused"
    }))
    .await?;
    Ok(())
}

async fn send_error(ctx: Context<'_>, message: String) -> Result<()> {
    ctx.send(CreateReply::default().content(message).ephemeral(true))
        .await?;
    Ok(())
}
