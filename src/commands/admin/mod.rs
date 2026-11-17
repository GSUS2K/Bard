use std::{path::Path, sync::OnceLock, time::Instant};

use anyhow::Result;
use poise::CreateReply;
use serenity::all::{CreateEmbed, CreateEmbedFooter};
use tokio::process::Command;

use crate::bot::Context;

static STARTED_AT: OnceLock<Instant> = OnceLock::new();

pub fn initialize() {
    STARTED_AT.get_or_init(Instant::now);
}

/// Show bot, deployment, server, and music storage status (owner only).
#[poise::command(slash_command)]
pub async fn status(ctx: Context<'_>) -> Result<()> {
    if !is_owner(&ctx) {
        return reply(ctx, "This command is restricted to the bot owner.").await;
    }

    let running = git_output(&["rev-parse", "--short", "HEAD"]).await;
    let latest = latest_remote_commit().await;
    let uptime = format_duration(STARTED_AT.get_or_init(Instant::now).elapsed().as_secs());
    let music_path = Path::new(spoticord_config::music_dir());
    let (music_files, music_bytes) = library_size(music_path).await;
    let disk = disk_status(music_path).await;
    let host = command_text("hostname", &[])
        .await
        .unwrap_or_else(|| "unknown".into());
    let os = command_text("uname", &["-sr"])
        .await
        .unwrap_or_else(|| "unknown".into());
    let cpu_count = command_text("nproc", &[])
        .await
        .unwrap_or_else(|| "unknown".into());
    let memory = memory_status()
        .await
        .unwrap_or_else(|| "unavailable".into());
    let guild_count = ctx.serenity_context().cache.guilds().len();
    let update_state = match (&running, &latest) {
        (Some(current), Some(remote)) if current == remote => "up to date".to_owned(),
        (Some(current), Some(remote)) => format!("update available (`{current}` → `{remote}`)"),
        (Some(current), None) => format!("running `{current}`; remote unavailable"),
        _ => "commit unavailable".to_owned(),
    };
    let disk_text = disk.unwrap_or_else(|| "unavailable".to_owned());
    let embed = CreateEmbed::new()
        .title("Bard · System status")
        .description(format!(
            "🟢 Online · up {uptime}\nRelease `{}` · {update_state}",
            spoticord_config::VERSION
        ))
        .field("Server", format!("`{host}` · `{os}`\n{cpu_count} vCPU · {memory}\nConnected to {guild_count} server(s)"), true)
        .field(
            "Music library",
            format!("{music_files} file(s) · {}", human_bytes(music_bytes)),
            true,
        )
        .field("EC2 root disk · OS, tools, apps, files", disk_text, false)
        .footer(CreateEmbedFooter::new(
            "Use /update to fetch and build the configured GitHub branch",
        ));
    ctx.send(CreateReply::default().embed(embed).ephemeral(true))
        .await?;
    Ok(())
}

/// Restart the bot process (owner only; requires systemd or another supervisor).
#[poise::command(slash_command)]
pub async fn restart(ctx: Context<'_>) -> Result<()> {
    if !is_owner(&ctx) {
        return reply(ctx, "This command is restricted to the bot owner.").await;
    }
    if !is_supervised() {
        return reply(
            ctx,
            "Restart is disabled: run Bard under systemd and set BOT_SUPERVISED=true.",
        )
        .await;
    }
    schedule_restart_notice("restart").await?;
    if let Err(error) = reply(ctx, "Restarting Bard now. I’ll DM you when it’s back online.").await {
        let _ = tokio::fs::remove_file(crate::bot::RESTART_NOTICE_PATH).await;
        return Err(error);
    }
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    std::process::exit(0);
}

/// Fetch and build the configured GitHub branch, then restart Bard (owner only).
#[poise::command(slash_command)]
pub async fn update(ctx: Context<'_>) -> Result<()> {
    if !is_owner(&ctx) {
        return reply(ctx, "This command is restricted to the bot owner.").await;
    }
    if !is_supervised() {
        return reply(
            ctx,
            "Update is disabled: run Bard under systemd and set BOT_SUPERVISED=true.",
        )
        .await;
    }

    let remote = match std::env::var("UPDATE_REMOTE") {
        Ok(remote) if valid_remote(&remote) => remote,
        _ => {
            return reply(
                ctx,
                "Set UPDATE_REMOTE to your Git remote name (usually `origin`).",
            )
            .await
        }
    };
    let branch = std::env::var("UPDATE_BRANCH").unwrap_or_else(|_| "main".to_owned());
    if !valid_branch(&branch) {
        return reply(ctx, "UPDATE_BRANCH contains unsupported characters.").await;
    }
    ctx.defer_ephemeral().await?;

    let dirty = run("git", &["status", "--porcelain"]).await?;
    if !dirty.status.success() {
        return reply(
            ctx,
            "Couldn't inspect the Git working tree; update cancelled.",
        )
        .await;
    }
    if !String::from_utf8_lossy(&dirty.stdout).trim().is_empty() {
        return reply(ctx, "Update cancelled: the checkout has local changes. Save them to GitHub or discard them before updating.").await;
    }
    let fetch = run("git", &["fetch", "--no-tags", &remote, &branch]).await?;
    if !fetch.status.success() {
        return reply(
            ctx,
            format!("Git fetch failed: {}", output_tail(&fetch.stderr)),
        )
        .await;
    }
    let merge = run("git", &["merge", "--ff-only", "FETCH_HEAD"]).await?;
    if !merge.status.success() {
        return reply(
            ctx,
            format!(
                "Update stopped: fast-forward failed: {}",
                output_tail(&merge.stderr)
            ),
        )
        .await;
    }
    let build = run("cargo", &["build", "--release"]).await?;
    if !build.status.success() {
        return reply(
            ctx,
            format!(
                "Code fetched, but the release build failed: {}",
                output_tail(&build.stderr)
            ),
        )
        .await;
    }
    let restore_lock = run("git", &["restore", "--", "Cargo.lock"]).await?;
    if !restore_lock.status.success() {
        return reply(
            ctx,
            "The build succeeded, but I couldn't restore Cargo.lock; Bard was not restarted.",
        )
        .await;
    }
    schedule_restart_notice("update").await?;
    if let Err(error) = reply(ctx, "Update built successfully. Restarting Bard now; I’ll DM you when it’s back online.").await {
        let _ = tokio::fs::remove_file(crate::bot::RESTART_NOTICE_PATH).await;
        return Err(error);
    }
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    std::process::exit(0);
}

fn is_owner(ctx: &Context<'_>) -> bool {
    ctx.author().id.get() == crate::bot::OWNER_ID
}
async fn schedule_restart_notice(action: &str) -> Result<()> {
    tokio::fs::write(crate::bot::RESTART_NOTICE_PATH, action).await?;
    Ok(())
}
fn is_supervised() -> bool {
    std::env::var("BOT_SUPERVISED").is_ok_and(|v| v.eq_ignore_ascii_case("true"))
}
fn valid_remote(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
}
fn valid_branch(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '/' | '.'))
}

async fn run(program: &str, args: &[&str]) -> Result<std::process::Output> {
    Ok(Command::new(program).args(args).output().await?)
}
async fn command_text(program: &str, args: &[&str]) -> Option<String> {
    let output = tokio::time::timeout(std::time::Duration::from_secs(4), run(program, args))
        .await
        .ok()?
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}
async fn git_output(args: &[&str]) -> Option<String> {
    command_text("git", args).await
}
async fn latest_remote_commit() -> Option<String> {
    let remote = std::env::var("UPDATE_REMOTE").ok()?;
    let branch = std::env::var("UPDATE_BRANCH").unwrap_or_else(|_| "main".to_owned());
    if !valid_remote(&remote) || !valid_branch(&branch) {
        return None;
    }
    let output = command_text(
        "git",
        &["ls-remote", &remote, &format!("refs/heads/{branch}")],
    )
    .await?;
    output
        .split_whitespace()
        .next()
        .map(|sha| sha.chars().take(7).collect())
}
async fn library_size(root: &Path) -> (usize, u64) {
    let mut dirs = vec![root.to_path_buf()];
    let mut count = 0;
    let mut bytes = 0;
    while let Some(dir) = dirs.pop() {
        let Ok(mut entries) = tokio::fs::read_dir(dir).await else {
            continue;
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            let Ok(kind) = entry.file_type().await else {
                continue;
            };
            if kind.is_dir() {
                dirs.push(entry.path());
            } else if kind.is_file()
                && entry
                    .path()
                    .extension()
                    .and_then(|x| x.to_str())
                    .is_some_and(|x| {
                        ["aac", "flac", "m4a", "mp3", "ogg", "wav"]
                            .contains(&x.to_ascii_lowercase().as_str())
                    })
            {
                count += 1;
                bytes += entry.metadata().await.map(|m| m.len()).unwrap_or(0);
            }
        }
    }
    (count, bytes)
}
async fn disk_status(path: &Path) -> Option<String> {
    let output = command_text("df", &["-Pk", path.to_str()?]).await?;
    let row = output.lines().last()?;
    let columns = row.split_whitespace().collect::<Vec<_>>();
    if columns.len() < 6 {
        return None;
    }
    let total = columns[1].parse::<u64>().ok()?.saturating_mul(1024);
    let used = columns[2].parse::<u64>().ok()?.saturating_mul(1024);
    let free = columns[3].parse::<u64>().ok()?.saturating_mul(1024);
    Some(format!(
        "{} · used {} · free {}",
        columns[5],
        human_bytes(used.min(total)),
        human_bytes(free)
    ))
}
async fn memory_status() -> Option<String> {
    let meminfo = tokio::fs::read_to_string("/proc/meminfo").await.ok()?;
    let value_kb = |key: &str| -> Option<u64> {
        meminfo.lines().find_map(|line| {
            let (name, rest) = line.split_once(':')?;
            (name == key)
                .then(|| rest.split_whitespace().next()?.parse::<u64>().ok())
                .flatten()
        })
    };
    let total = value_kb("MemTotal")?.saturating_mul(1024);
    let available = value_kb("MemAvailable")?.saturating_mul(1024);
    Some(format!(
        "RAM {} total · {} free",
        human_bytes(total),
        human_bytes(available)
    ))
}
fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}
fn format_duration(seconds: u64) -> String {
    let days = seconds / 86400;
    let hours = seconds / 3600 % 24;
    let minutes = seconds / 60 % 60;
    let seconds = seconds % 60;
    if days > 0 {
        format!("{days}d {hours}h {minutes}m")
    } else if hours > 0 {
        format!("{hours}h {minutes}m {seconds}s")
    } else {
        format!("{minutes}m {seconds}s")
    }
}
fn output_tail(bytes: &[u8]) -> String {
    const MAX_CHARS: usize = 1100;
    let text = String::from_utf8_lossy(bytes);
    let tail = text.trim();
    if tail.chars().count() > MAX_CHARS {
        format!(
            "…{}",
            tail.chars()
                .rev()
                .take(MAX_CHARS)
                .collect::<String>()
                .chars()
                .rev()
                .collect::<String>()
        )
    } else {
        tail.to_owned()
    }
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

