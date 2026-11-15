use anyhow::Result;
use poise::CreateReply;
use reqwest::Method;

use crate::bot::Context;

/// List playlists in your linked Spotify library
#[poise::command(slash_command)]
pub async fn playlists(ctx: Context<'_>) -> Result<()> {
    let response = match super::api::request_path(&ctx, Method::GET, "me/playlists?limit=20").await
    {
        Ok(response) => response,
        Err(error) => return send(ctx, error.to_string()).await,
    };

    let body: serde_json::Value = match response.json().await {
        Ok(body) => body,
        Err(error) => return send(ctx, format!("Couldn't read Spotify's response: {error}")).await,
    };
    let items = body
        .get("items")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();

    if items.is_empty() {
        return send(ctx, "No playlists were returned for your Spotify account.").await;
    }

    let mut lines = Vec::new();
    for item in items {
        let Some(name) = item.get("name").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let url = item
            .get("external_urls")
            .and_then(|urls| urls.get("spotify"))
            .and_then(serde_json::Value::as_str);
        match url {
            Some(url) => lines.push(format!("• [{}]({url})", markdown_escape(name))),
            None => lines.push(format!("• {}", markdown_escape(name))),
        }
    }

    let mut message = format!("**Your Spotify playlists**\n{}", lines.join("\n"));
    if body.get("next").is_some_and(|next| !next.is_null()) {
        message.push_str("\n\nShowing the first 20 playlists.");
    }
    send(ctx, message).await
}

fn markdown_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('[', "\\[")
        .replace(']', "\\]")
}

async fn send(ctx: Context<'_>, message: impl Into<String>) -> Result<()> {
    ctx.send(
        CreateReply::default()
            .content(message.into())
            .ephemeral(true),
    )
    .await?;
    Ok(())
}
