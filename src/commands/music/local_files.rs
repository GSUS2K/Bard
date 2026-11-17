use std::{
    collections::{HashMap, VecDeque},
    fs,
    path::{Component, Path, PathBuf},
    sync::OnceLock,
};

use anyhow::{bail, Context as _, Result};
use poise::CreateReply;
use serenity::all::{
    Attachment, ButtonStyle, ComponentInteractionCollector, ComponentInteractionDataKind,
    CreateActionRow, CreateButton, CreateEmbed, CreateEmbedFooter, CreateInteractionResponse,
    CreateInteractionResponseMessage, CreateQuickModal, CreateSelectMenu, CreateSelectMenuKind,
    CreateSelectMenuOption, Permissions,
};
use sha2::{Digest, Sha256};
use songbird::input::{codecs::get_codec_registry, codecs::get_probe, File as AudioFile, Input};
use spoticord_utils::discord::Colors;
use tokio::io::AsyncWriteExt;

use crate::bot::Context;

const AUDIO_EXTENSIONS: &[&str] = &["aac", "flac", "m4a", "mp3", "ogg", "wav"];
const MAX_UPLOAD_BYTES: u32 = 25 * 1024 * 1024;
static LOCAL_QUEUES: OnceLock<tokio::sync::Mutex<HashMap<u64, VecDeque<String>>>> = OnceLock::new();

fn local_queues() -> &'static tokio::sync::Mutex<HashMap<u64, VecDeque<String>>> {
    LOCAL_QUEUES.get_or_init(|| tokio::sync::Mutex::new(HashMap::new()))
}

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
    // Attachment downloads can exceed Discord's three-second initial response window.
    ctx.defer_ephemeral().await?;

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

    let owners_dir = root.join(".bard-owners");
    if let Err(error) = tokio::fs::create_dir_all(&owners_dir).await {
        let _ = tokio::fs::remove_file(&path).await;
        return reply(ctx, format!("I couldn't save file ownership data: {error}")).await;
    }
    if let Err(error) = tokio::fs::write(
        owner_metadata_path(&root, &filename),
        ctx.author().id.get().to_string(),
    )
    .await
    {
        let _ = tokio::fs::remove_file(&path).await;
        return reply(ctx, format!("I couldn't save file ownership data: {error}")).await;
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
    let guild_id = ctx.guild_id().expect("guild_only command has a guild");
    if let Err(error) = enqueue_for_user(ctx, guild_id, ctx.author().id, &name, true).await {
        return reply(ctx, error.to_string()).await;
    }

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

/// Add a local audio file to the end of this server's playback queue
#[poise::command(slash_command, guild_only)]
pub async fn queuefile(
    ctx: Context<'_>,
    #[autocomplete = "autocomplete_audio_file"]
    #[description = "Audio file to add to the queue"]
    name: String,
) -> Result<()> {
    let guild_id = ctx.guild_id().expect("guild_only command has a guild");
    match enqueue_for_user(ctx, guild_id, ctx.author().id, &name, false).await {
        Ok(()) => {
            reply(
                ctx,
                format!(
                    "Added `{}` to the voice queue.",
                    spoticord_utils::discord::escape(name)
                ),
            )
            .await
        }
        Err(error) => reply(ctx, error.to_string()).await,
    }
}

/// Show how many local tracks are queued in this server
#[poise::command(slash_command, guild_only)]
pub async fn localqueue(ctx: Context<'_>) -> Result<()> {
    let guild_id = ctx.guild_id().expect("guild_only command has a guild");
    let Some(manager) = songbird::get(ctx.serenity_context()).await else {
        return reply(ctx, "The voice player is unavailable right now.").await;
    };
    let Some(call) = manager.get(guild_id) else {
        return reply(
            ctx,
            "Bard isn't connected to voice. The local queue is empty.",
        )
        .await;
    };
    let call = call.lock().await;
    let count = call.queue().len();
    let mut queues = local_queues().lock().await;
    let names = queues.entry(guild_id.get()).or_default();
    while names.len() > count {
        names.pop_front();
    }
    let list = names
        .iter()
        .enumerate()
        .map(|(i, name)| {
            format!(
                "{} `{}`",
                if i == 0 { "▶" } else { "•" },
                spoticord_utils::discord::escape(name.clone())
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    reply(
        ctx,
        if count == 0 {
            "The local queue is empty.".to_owned()
        } else {
            format!(
                "🎵 **{} track(s)**\n{}\n\nUse `/skiplocal` or `/voicepanel` to control playback.",
                count,
                if list.is_empty() {
                    "Track names are unavailable for items queued before this restart.".to_owned()
                } else {
                    list
                }
            )
        },
    )
    .await
}

/// Skip the current local track and continue the queue
#[poise::command(slash_command, guild_only)]
pub async fn skiplocal(ctx: Context<'_>) -> Result<()> {
    let guild_id = ctx.guild_id().expect("guild_only command has a guild");
    let Some(manager) = songbird::get(ctx.serenity_context()).await else {
        return reply(ctx, "The voice player is unavailable right now.").await;
    };
    let Some(call) = manager.get(guild_id) else {
        return reply(ctx, "Bard isn't connected to voice in this server.").await;
    };
    let call = call.lock().await;
    if call.queue().is_empty() {
        return reply(ctx, "Nothing is playing right now.").await;
    }
    call.queue().skip()?;
    local_queues()
        .lock()
        .await
        .entry(guild_id.get())
        .or_default()
        .pop_front();
    reply(ctx, "Skipped the current local track.").await
}

/// Bard's tiny hidden fortune
#[poise::command(slash_command)]
pub async fn bard(ctx: Context<'_>) -> Result<()> {
    let fortunes = [
        "The Bard whispers: the next track is the one you meant to play.",
        "A quiet queue is just a song waiting for its cue.",
        "Hidden verse unlocked: `BARD-PLAYBACK-42` (it does absolutely nothing).",
        "The lyre hums: upload locally, listen together.",
    ];
    let index = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as usize
        % fortunes.len();
    reply(ctx, fortunes[index]).await
}

/// Rename a file you uploaded (server managers can rename any library file)
#[poise::command(slash_command, guild_only)]
pub async fn renamefile(
    ctx: Context<'_>,
    #[autocomplete = "autocomplete_audio_file"]
    #[description = "File to rename"]
    name: String,
    #[description = "New file name; keep or add an audio extension"] new_name: String,
) -> Result<()> {
    let old_path = match resolve_file(&name) {
        Ok(path) => path,
        Err(error) => return reply(ctx, error.to_string()).await,
    };
    let root = music_root()?;
    let new_name = match upload_filename(&name, Some(&new_name)) {
        Ok(name) => name,
        Err(error) => return reply(ctx, error.to_string()).await,
    };
    let new_path = root.join(&new_name);
    if new_path.exists() {
        return reply(ctx, "A file with that name already exists.").await;
    }
    let old_metadata = owner_metadata_path(&root, &name);
    let uploader = tokio::fs::read_to_string(&old_metadata)
        .await
        .ok()
        .and_then(|id| id.trim().parse::<u64>().ok());
    let manager = ctx.author_member().await.is_some_and(|member| {
        member
            .permissions(ctx.serenity_context().cache.as_ref())
            .is_ok_and(|p| {
                p.contains(Permissions::ADMINISTRATOR) || p.contains(Permissions::MANAGE_GUILD)
            })
    });
    if uploader != Some(ctx.author().id.get())
        && !manager
        && ctx.author().id.get() != 682_288_992_456_409_096
    {
        return reply(
            ctx,
            "You can rename files you uploaded. A server manager can rename any file.",
        )
        .await;
    }
    if let Err(error) = tokio::fs::rename(&old_path, &new_path).await {
        return reply(ctx, format!("I couldn't rename that file: {error}")).await;
    }
    if let Some(id) = uploader {
        let _ = tokio::fs::write(owner_metadata_path(&root, &new_name), id.to_string()).await;
        let _ = tokio::fs::remove_file(old_metadata).await;
    }
    reply(
        ctx,
        format!(
            "Renamed `{}` to `{}`.",
            spoticord_utils::discord::escape(name),
            spoticord_utils::discord::escape(new_name)
        ),
    )
    .await
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

/// Delete your audio; server managers can delete any library file
#[poise::command(slash_command, guild_only)]
pub async fn deletefile(
    ctx: Context<'_>,
    #[autocomplete = "autocomplete_audio_file"]
    #[description = "Audio file to delete"]
    name: String,
) -> Result<()> {
    let path = match resolve_file(&name) {
        Ok(path) => path,
        Err(error) => return reply(ctx, error.to_string()).await,
    };
    let root = music_root()?;
    let metadata = owner_metadata_path(&root, &name);
    let uploader = tokio::fs::read_to_string(&metadata)
        .await
        .ok()
        .and_then(|id| id.trim().parse::<u64>().ok());
    let is_uploader = uploader == Some(ctx.author().id.get());
    let is_bot_owner = ctx.author().id.get() == 682_288_992_456_409_096;
    let is_server_manager = ctx.author_member().await.is_some_and(|member| {
        member
            .permissions(ctx.serenity_context().cache.as_ref())
            .is_ok_and(|permissions| {
                permissions.contains(Permissions::ADMINISTRATOR)
                    || permissions.contains(Permissions::MANAGE_GUILD)
            })
    });

    if !is_uploader && !is_server_manager && !is_bot_owner {
        return reply(
            ctx,
            "You can delete files you uploaded. A server manager can delete anyone's files.",
        )
        .await;
    }

    match tokio::fs::remove_file(&path).await {
        Ok(()) => {
            let _ = tokio::fs::remove_file(metadata).await;
            reply(
                ctx,
                format!("Deleted `{}`.", spoticord_utils::discord::escape(name)),
            )
            .await
        }
        Err(error) => reply(ctx, format!("I couldn't delete that file: {error}")).await,
    }
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

    let mut call = call.lock().await;
    call.stop();
    call.queue().stop();
    local_queues().lock().await.remove(&guild_id.get());
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
        Ok(()) => {
            local_queues().lock().await.remove(&guild_id.get());
            reply(ctx, "Left the voice channel.").await
        }
        Err(error) => reply(ctx, format!("I couldn't leave the voice channel: {error}")).await,
    }
}

/// Open an interactive control panel for local voice playback
#[poise::command(slash_command, guild_only)]
pub async fn voicepanel(ctx: Context<'_>) -> Result<()> {
    let guild_id = ctx.guild_id().expect("guild_only command has a guild");
    let files = music_root()
        .ok()
        .and_then(|root| audio_files(&root).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|name| name.chars().count() <= 100)
        .take(25)
        .collect::<Vec<_>>();
    let queue_text = {
        let queues = local_queues().lock().await;
        queues
            .get(&guild_id.get())
            .map(|q| {
                q.iter()
                    .take(8)
                    .enumerate()
                    .map(|(i, n)| {
                        format!(
                            "{} `{}`",
                            if i == 0 { "▶" } else { "•" },
                            spoticord_utils::discord::escape(n.clone())
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "Nothing queued yet".to_owned())
    };
    let mut selected_file: Option<String> = None;

    let mut components = Vec::new();
    if !files.is_empty() {
        let options = files
            .iter()
            .map(|name| CreateSelectMenuOption::new(name.clone(), name.clone()))
            .collect();
        components.push(CreateActionRow::SelectMenu(
            CreateSelectMenu::new("bard:select-file", CreateSelectMenuKind::String { options })
                .placeholder("▶ Play a local audio file"),
        ));
        let manage_options = files
            .iter()
            .map(|name| CreateSelectMenuOption::new(name.clone(), name.clone()))
            .collect();
        components.push(CreateActionRow::SelectMenu(
            CreateSelectMenu::new(
                "bard:manage-file",
                CreateSelectMenuKind::String {
                    options: manage_options,
                },
            )
            .placeholder("✎ Select a file to rename or delete"),
        ));
    }
    components.push(CreateActionRow::Buttons(vec![
        CreateButton::new("bard:skip")
            .label("Skip")
            .style(ButtonStyle::Secondary),
        CreateButton::new("bard:pause")
            .label("Pause")
            .style(ButtonStyle::Secondary),
        CreateButton::new("bard:resume")
            .label("Resume")
            .style(ButtonStyle::Primary),
        CreateButton::new("bard:stop")
            .label("Stop")
            .style(ButtonStyle::Danger),
        CreateButton::new("bard:leave")
            .label("Disconnect")
            .style(ButtonStyle::Secondary),
    ]));
    components.push(CreateActionRow::Buttons(vec![
        CreateButton::new("bard:rename-file")
            .label("Rename selected")
            .style(ButtonStyle::Secondary),
        CreateButton::new("bard:delete-file")
            .label("Delete selected")
            .style(ButtonStyle::Danger),
    ]));

    let panel = ctx
        .send(
            CreateReply::default()
                .content(format!("🎼 **Bard · Music desk**\nChoose a file to play now; use `/queuefile` to add without interrupting.\n\n**Queue**\n{queue_text}\n\nSelect a library file below to rename or delete it."))
                .components(components)
                .ephemeral(true),
        )
        .await?;
    let message_id = panel.message().await?.id;

    while let Some(interaction) = ComponentInteractionCollector::new(ctx.serenity_context())
        .author_id(ctx.author().id)
        .channel_id(ctx.channel_id())
        .guild_id(guild_id)
        .message_id(message_id)
        .timeout(std::time::Duration::from_secs(300))
        .await
    {
        if interaction.data.custom_id == "bard:rename-file" {
            let Some(name) = selected_file.as_deref() else {
                interaction
                    .create_response(
                        ctx.serenity_context(),
                        CreateInteractionResponse::Message(
                            CreateInteractionResponseMessage::new()
                                .content("Select a library file first.")
                                .ephemeral(true),
                        ),
                    )
                    .await?;
                continue;
            };
            let modal = CreateQuickModal::new("Rename audio file")
                .short_field("New file name (extension optional)")
                .timeout(std::time::Duration::from_secs(180));
            let response = interaction
                .quick_modal(ctx.serenity_context(), modal)
                .await?;
            let message = match response.and_then(|r| r.inputs.first().cloned()) {
                Some(new_name) => match rename_library_file(ctx, name, &new_name).await {
                    Ok(message) => message,
                    Err(error) => error.to_string(),
                },
                None => "Rename cancelled.".to_owned(),
            };
            ctx.send(CreateReply::default().content(message).ephemeral(true))
                .await?;
            continue;
        }
        let result = match interaction.data.custom_id.as_str() {
            "bard:pause" | "bard:resume" | "bard:stop" | "bard:leave" | "bard:skip" => {
                voice_action(ctx, guild_id, interaction.data.custom_id.as_str()).await
            }
            "bard:manage-file" => {
                selected_file = match &interaction.data.kind {
                    ComponentInteractionDataKind::StringSelect { values } => {
                        values.first().cloned()
                    }
                    _ => None,
                };
                let content = selected_file
                    .as_deref()
                    .map(|n| {
                        format!(
                            "Selected `{}` for library actions.",
                            spoticord_utils::discord::escape(n.to_owned())
                        )
                    })
                    .unwrap_or_else(|| "Select a library file first.".to_owned());
                interaction
                    .create_response(
                        ctx.serenity_context(),
                        CreateInteractionResponse::Message(
                            CreateInteractionResponseMessage::new()
                                .content(content)
                                .ephemeral(true),
                        ),
                    )
                    .await?;
                continue;
            }
            "bard:delete-file" => match selected_file.as_deref() {
                Some(name) => delete_library_file(ctx, name).await,
                None => Ok("Select a library file first.".to_owned()),
            },
            "bard:select-file" => {
                let selected = match &interaction.data.kind {
                    ComponentInteractionDataKind::StringSelect { values } => values.first(),
                    _ => None,
                };
                match selected {
                    Some(name) => play_for_user(ctx, guild_id, ctx.author().id, name).await,
                    None => bail!("Choose a file from the menu."),
                }
            }
            _ => continue,
        };
        let message = match result {
            Ok(message) => message,
            Err(error) => error.to_string(),
        };
        interaction
            .create_response(
                ctx.serenity_context(),
                CreateInteractionResponse::Message(
                    CreateInteractionResponseMessage::new()
                        .content(message)
                        .ephemeral(true),
                ),
            )
            .await?;
    }

    Ok(())
}

async fn can_manage_library_file(ctx: Context<'_>, name: &str) -> Result<()> {
    let root = music_root()?;
    let metadata = owner_metadata_path(&root, name);
    let uploader = tokio::fs::read_to_string(metadata)
        .await
        .ok()
        .and_then(|id| id.trim().parse::<u64>().ok());
    let manager = ctx.author_member().await.is_some_and(|member| {
        member
            .permissions(ctx.serenity_context().cache.as_ref())
            .is_ok_and(|p| {
                p.contains(Permissions::ADMINISTRATOR) || p.contains(Permissions::MANAGE_GUILD)
            })
    });
    if uploader != Some(ctx.author().id.get())
        && !manager
        && ctx.author().id.get() != 682_288_992_456_409_096
    {
        bail!("You can manage files you uploaded. A server manager can manage any library file.");
    }
    Ok(())
}

async fn delete_library_file(ctx: Context<'_>, name: &str) -> Result<String> {
    can_manage_library_file(ctx, name).await?;
    let path = resolve_file(name)?;
    tokio::fs::remove_file(path).await?;
    let root = music_root()?;
    let _ = tokio::fs::remove_file(owner_metadata_path(&root, name)).await;
    Ok(format!(
        "Deleted `{}`.",
        spoticord_utils::discord::escape(name.to_owned())
    ))
}

async fn rename_library_file(ctx: Context<'_>, old: &str, requested: &str) -> Result<String> {
    can_manage_library_file(ctx, old).await?;
    let source = resolve_file(old)?;
    let new_name = upload_filename(old, Some(requested))?;
    let root = music_root()?;
    let target = root.join(&new_name);
    if target.exists() {
        bail!("A file with that name already exists.");
    }
    tokio::fs::rename(source, &target).await?;
    let old_meta = owner_metadata_path(&root, old);
    if let Ok(uploader) = tokio::fs::read_to_string(&old_meta).await {
        tokio::fs::write(owner_metadata_path(&root, &new_name), uploader).await?;
        let _ = tokio::fs::remove_file(old_meta).await;
    }
    Ok(format!(
        "Renamed `{}` to `{}`.",
        spoticord_utils::discord::escape(old.to_owned()),
        spoticord_utils::discord::escape(new_name)
    ))
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
    let scan = async {
        let mut entries = tokio::fs::read_dir(&root).await.ok()?;
        let mut names = Vec::new();
        while let Ok(Some(entry)) = entries.next_entry().await {
            if !entry.file_type().await.is_ok_and(|kind| kind.is_file()) {
                continue;
            }
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if supported_audio(&path)
                && name.chars().count() <= 100
                && name.to_lowercase().contains(&query)
            {
                names.push(name);
            }
        }
        Some(names)
    };
    let mut files = tokio::time::timeout(std::time::Duration::from_millis(700), scan)
        .await
        .ok()
        .flatten()
        .unwrap_or_default();
    files.sort_unstable_by_key(|name| {
        let lower = name.to_lowercase();
        (!lower.starts_with(&query), lower)
    });
    files.truncate(25);
    files
}

fn owner_metadata_path(root: &Path, filename: &str) -> PathBuf {
    let digest = Sha256::digest(filename.as_bytes());
    let key = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    root.join(".bard-owners").join(format!("{key}.owner"))
}

async fn voice_action(
    ctx: Context<'_>,
    guild_id: serenity::all::GuildId,
    action: &str,
) -> Result<String> {
    let manager = songbird::get(ctx.serenity_context())
        .await
        .context("The voice player is unavailable right now.")?;

    if action == "leave" {
        manager.leave(guild_id).await?;
        local_queues().lock().await.remove(&guild_id.get());
        return Ok("Disconnected from voice.".to_owned());
    }

    let call = manager
        .get(guild_id)
        .context("Bard isn't connected to voice in this server.")?;
    let mut call = call.lock().await;
    match action {
        "pause" => {
            call.queue().pause()?;
            Ok("Paused local playback.".to_owned())
        }
        "resume" => {
            call.queue().resume()?;
            Ok("Resumed local playback.".to_owned())
        }
        "stop" => {
            call.stop();
            call.queue().stop();
            local_queues().lock().await.remove(&guild_id.get());
            Ok("Stopped local playback.".to_owned())
        }
        "skip" => {
            if call.queue().is_empty() {
                bail!("Nothing is playing right now.");
            }
            call.queue().skip()?;
            local_queues()
                .lock()
                .await
                .entry(guild_id.get())
                .or_default()
                .pop_front();
            Ok("Skipped the current local track.".to_owned())
        }
        _ => bail!("Unknown playback control."),
    }
}

async fn play_for_user(
    ctx: Context<'_>,
    guild_id: serenity::all::GuildId,
    user_id: serenity::all::UserId,
    name: &str,
) -> Result<String> {
    enqueue_for_user(ctx, guild_id, user_id, name, true).await?;
    Ok(format!(
        "Playing `{}`.",
        spoticord_utils::discord::escape(name)
    ))
}

async fn enqueue_for_user(
    ctx: Context<'_>,
    guild_id: serenity::all::GuildId,
    user_id: serenity::all::UserId,
    name: &str,
    replace: bool,
) -> Result<()> {
    let path = resolve_file(name)?;
    let channel_id = guild_id
        .to_guild_cached(ctx.serenity_context())
        .and_then(|guild| {
            guild
                .voice_states
                .get(&user_id)
                .and_then(|state| state.channel_id)
        })
        .context("Join a voice channel first, then choose a file.")?;
    let input = Input::from(AudioFile::new(path))
        .make_playable_async(get_codec_registry(), get_probe())
        .await?;
    let manager = songbird::get(ctx.serenity_context())
        .await
        .context("The voice player is unavailable right now.")?;
    let call = manager.join(guild_id, channel_id).await?;
    let mut call = call.lock().await;
    if replace {
        call.stop();
        call.queue().stop();
    }
    call.enqueue_input(input).await;
    drop(call);
    {
        let mut queues = local_queues().lock().await;
        let queue = queues.entry(guild_id.get()).or_default();
        if replace {
            queue.clear();
        }
        queue.push_back(name.to_owned());
    }
    ctx.serenity_context()
        .set_activity(Some(serenity::all::ActivityData::listening(
            name.chars().take(80).collect::<String>(),
        )));
    Ok(())
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

