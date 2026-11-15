use anyhow::Result;
use poise::CreateReply;
use serenity::all::{CreateEmbed, CreateEmbedAuthor};
use spoticord_config::VERSION;
use spoticord_utils::discord::Colors;

use crate::bot::Context;

const IMAGE_URL: &str = "https://cdn.discordapp.com/avatars/389786424142200835/6bfe3840b0aa6a1baf432bb251b70c9f.webp?size=128";

/// Shows the current active version of Bard
#[poise::command(slash_command)]
pub async fn version(ctx: Context<'_>) -> Result<()> {
    // Had to pull this from the builder as rustfmt refused to format the file
    let description = format!("Current version: {}\n\nBard is open source: [view it on GitHub](https://github.com/GSUS2K/Bard)", VERSION);

    ctx.send(
        CreateReply::default().embed(
            CreateEmbed::default()
                .title("Bard Version")
                .author(
                    CreateEmbedAuthor::new("Maintained by: DaXcess (@daxcess)")
                        .url("https://github.com/DaXcess")
                        .icon_url(IMAGE_URL),
                )
                .description(description)
                .color(Colors::Info),
        ),
    )
    .await?;

    Ok(())
}
