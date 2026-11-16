use std::{
    fs,
    path::{Component, Path, PathBuf},
};

use anyhow::{bail, Context as _, Result};
use poise::CreateReply;
use serenity::all::{Attachment, CreateEmbed, CreateEmbedFooter};
use songbird::input::{codecs::get_codec_registry, codecs::get_probe, File as AudioFile, Input};
use spoticord_utils::discord::Colors;
use tokio::io::AsyncWriteExt;

use crate::bot::Context;

const AUDIO_EXTENSIONS: &[&str] = &["aac", "flac", "m4a", "mp3", "ogg", "wav"];
const MAX_UPLOAD_BYTES: u32 = 25 * 1024 * 1024;

/// Save an attached audio file to the bot's music folder
#[poise::command(slash_command, guild_only)]
pub async fn uploadfile(
    ctx: Context<'_>,
    #[description = "Audio file to save"] file: Attachment,
    #[description = "Optional saved name; the original extension is added if omitted"]
    save_as: Option<String>,
) -> Result<()> {
    let filename = match upload_filename(&file.filename, save_as.as_deref()) {
        Ok(filename) => filename,
        Err(error) => return reply(ctx, error.to_string()).await,
    };
    if file.size > MAX_UPLOAD_BYTES {
        return reply(ctx, "That file is over the 25 MB upload limit.").await;
    }

    let root = PathBuf::from(spoticord_config::music_dir());
    if let Err(error) = tokio::fs::create_dir_all(&root).await {
        return reply(ctx, format!("I couldn't create the music folder: {error}")).await;
    }
    let root = match root.canonicalize() {
        Ok(root) => root,
        Err(error) => {
            return reply(ctx, format!("I couldn't open the music folder: {error}")).await
        }
    };
    let path = root.join(&filename);
    if path.exists() {
        return reply(
            ctx,
            format!(
                "`{}` already exists in the music folder. Rename the attachment and try again.",
                spoticord_utils::discord::escape(filename.clone())
            ),
        )
        .await;
    }

    let bytes = match file.download().await {
        Ok(bytes) if bytes.len() <= MAX_UPLOAD_BYTES as usize => bytes,
        Ok(_) => return reply(ctx, "That file is over the 25 MB upload limit.").await,
        Err(error) => {
            return reply(ctx, format!("I couldn't download that attachment: {error}")).await
        }
    };
    let mut output = match tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .await
    {
        Ok(output) => output,
        Err(error) => return reply(ctx, format!("I couldn't save that file: {error}")).await,
    };
    if let Err(error) = output.write_all(&bytes).await {
        let _ = tokio::fs::remove_file(&path).await;
        return reply(ctx, format!("I couldn't finish saving that file: {error}")).await;
    }

    reply(
        ctx,
        format!(
            "Saved `{}` to the bot's music folder. Use `/playfile` to play it in voice.",
            spoticord_utils::discord::escape(filename)
        ),
    )
    .await
}

/// Play a local audio file from the configured music folder in your voice channel
#[poise::command(slash_command, guild_only)]
pub async fn playfile(
    ctx: Context<'_>,
    #[autocomplete = "autocomplete_audio_file"]
    #[description = "File name from the music folder"]
    name: String,
) -> Result<()> {
    let path = match resolve_file(&name) {
        Ok(path) => path,
        Err(error) => return reply(ctx, error.to_string()).await,
    };

    let guild_id = ctx.guild_id().expect("guild_only command has a guild");
    let user_id = ctx.author().id;
    let channel_id = guild_id
        .to_guild_cached(ctx.serenity_context())
        .and_then(|guild| {
            guild
                .voice_states
                .get(&user_id)
                .and_then(|state| state.channel_id)
        });

    let Some(channel_id) = channel_id else {
        return reply(ctx, "Join a voice channel first, then run `/playfile`.").await;
    };

    let input = match Input::from(AudioFile::new(path))
        .make_playable_async(get_codec_registry(), get_probe())
        .await
    {
        Ok(input) => input,
        Err(error) => {
            return reply(ctx, format!("I couldn't read that audio file: {error}")).await;
        }
    };

    let Some(songbird) = songbird::get(ctx.serenity_context()).await else {
        return reply(ctx, "The voice player is unavailable right now.").await;
    };

    let call = match songbird.join(guild_id, channel_id).await {
        Ok(call) => call,
        Err(error) => {
            return reply(ctx, format!("I couldn't join your voice channel: {error}")).await;
        }
    };

    let mut call = call.lock().await;
    call.stop();
    call.play_input(input);
    drop(call);

    ctx.send(
        CreateReply::default()
            .embed(
                CreateEmbed::new()
                    .title("Playing local file")
                    .description(format!("`{}`", spoticord_utils::discord::escape(name)))
                    .footer(CreateEmbedFooter::new(
                        "Audio is played from the bot's music folder",
                    ))
                    .color(Colors::Info),
            )
            .ephemeral(true),
    )
    .await?;
    Ok(())
}

/// List audio files available in the configured music folder
#[poise::command(slash_command)]
pub async fn listfiles(ctx: Context<'_>) -> Result<()> {
    let root = match music_root() {
        Ok(root) => root,
        Err(error) => return reply(ctx, error.to_string()).await,
    };
    let files = match audio_files(&root) {
        Ok(files) => files,
        Err(error) => {
            return reply(
                ctx,
                format!(
                    "Could not read the music folder `{}`: {error}",
                    root.display()
                ),
            )
            .await;
        }
    };
    let message = if files.is_empty() {
        format!("No supported audio files found in `{}`.", root.display())
    } else {
        let names = files
            .iter()
            .map(|name| format!("• {}", spoticord_utils::discord::escape(name.clone())))
            .collect::<Vec<_>>()
            .join("\n");
        format!("**Audio files**\n{names}\n\nUse `/playfile` with a file name.")
    };

    ctx.send(CreateReply::default().content(message).ephemeral(true))
        .await?;
    Ok(())
}

/// Stop local audio playing in this server's voice channel
#[poise::command(slash_command, guild_only)]
pub async fn stopvoice(ctx: Context<'_>) -> Result<()> {
    let guild_id = ctx.guild_id().expect("guild_only command has a guild");
    let Some(songbird) = songbird::get(ctx.serenity_context()).await else {
        return reply(ctx, "The voice player is unavailable right now.").await;
    };
    let Some(call) = songbird.get(guild_id) else {
        return reply(ctx, "The bot isn't connected to voice in this server.").await;
    };

    call.lock().await.stop();
    reply(ctx, "Stopped local audio playback.").await
}

/// Disconnect the bot from this server's voice channel
#[poise::command(slash_command, guild_only)]
pub async fn leavevoice(ctx: Context<'_>) -> Result<()> {
    let guild_id = ctx.guild_id().expect("guild_only command has a guild");
    let Some(songbird) = songbird::get(ctx.serenity_context()).await else {
        return reply(ctx, "The voice player is unavailable right now.").await;
    };

    match songbird.leave(guild_id).await {
        Ok(()) => reply(ctx, "Left the voice channel.").await,
        Err(error) => reply(ctx, format!("I couldn't leave the voice channel: {error}")).await,
    }
}

fn music_root() -> Result<PathBuf> {
    PathBuf::from(spoticord_config::music_dir())
        .canonicalize()
        .with_context(|| {
            format!(
                "Music folder `{}` does not exist. Create it and add audio files.",
                spoticord_config::music_dir()
            )
        })
}

fn upload_filename(original: &str, requested: Option<&str>) -> Result<String> {
    let Some(requested) = requested.map(str::trim).filter(|name| !name.is_empty()) else {
        if supported_audio(Path::new(original)) {
            return Ok(original.to_owned());
        }
        bail!("Supported file types are AAC, FLAC, M4A, MP3, OGG, and WAV.");
    };

    if requested.contains('/') || requested.contains('\\') {
        bail!("Use a plain file name without folders.");
    }

    let mut filename = requested.to_owned();
    if Path::new(&filename).extension().is_none() {
        if let Some(extension) = Path::new(original).extension().and_then(|ext| ext.to_str()) {
            filename.push('.');
            filename.push_str(extension);
        }
    }

    let path = Path::new(&filename);
    if path.components().count() != 1
        || !matches!(path.components().next(), Some(Component::Normal(_)))
    {
        bail!("Use a plain file name without folders.");
    }
    if !supported_audio(path) {
        bail!("Use an AAC, FLAC, M4A, MP3, OGG, or WAV file name.");
    }

    Ok(filename)
}

fn audio_files(root: &Path) -> std::io::Result<Vec<String>> {
    let entries = fs::read_dir(root)?;
    let mut files = entries
        .filter_map(std::result::Result::ok)
        .filter(|entry| {
            entry
                .file_type()
                .map(|kind| kind.is_file())
                .unwrap_or(false)
        })
        .filter_map(|entry| {
            let path = entry.path();
            supported_audio(&path).then(|| entry.file_name().to_string_lossy().into_owned())
        })
        .collect::<Vec<_>>();
    files.sort_unstable_by_key(|name| name.to_lowercase());
    Ok(files)
}

async fn autocomplete_audio_file(_ctx: Context<'_>, partial: &str) -> Vec<String> {
    let query = partial.trim().to_lowercase();
    let root = PathBuf::from(spoticord_config::music_dir());
    let mut files = audio_files(&root)
        .unwrap_or_default()
        .into_iter()
        .filter(|name| name.chars().count() <= 100 && name.to_lowercase().contains(&query))
        .collect::<Vec<_>>();
    files.sort_unstable_by_key(|name| {
        let lower = name.to_lowercase();
        (!lower.starts_with(&query), lower)
    });
    files.truncate(25);
    files
}

fn resolve_file(name: &str) -> Result<PathBuf> {
    let requested = Path::new(name);
    if requested.components().count() != 1
        || !matches!(requested.components().next(), Some(Component::Normal(_)))
    {
        bail!("Use a file name from the music folder, not a path.");
    }

    if !supported_audio(requested) {
        bail!("Supported file types are AAC, FLAC, M4A, MP3, OGG, and WAV.");
    }

    let root = music_root()?;
    let path = root
        .join(requested)
        .canonicalize()
        .with_context(|| format!("Audio file `{name}` was not found in the music folder."))?;
    if !path.starts_with(&root) || !path.is_file() {
        bail!("That file isn't available in the music folder.");
    }

    Ok(path)
}

fn supported_audio(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            let extension = extension.to_ascii_lowercase();
            AUDIO_EXTENSIONS.contains(&extension.as_str())
        })
}

async fn reply(ctx: Context<'_>, message: impl Into<String>) -> Result<()> {
    ctx.send(
        CreateReply::default()
            .content(message.into())
            .ephemeral(true),
    )
    .await?;
    Ok(())
}

