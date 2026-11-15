use anyhow::Result;
use poise::CreateReply;
use tokio::process::Command;

use crate::bot::Context;

const OWNER_ID: u64 = 682_288_992_456_409_096;

/// Owner-only bot administration
#[poise::command(
    slash_command,
    subcommands("status", "restart", "update"),
    subcommand_required
)]
pub async fn admin(_ctx: Context<'_>) -> Result<()> {
    Ok(())
}

/// Show the bot version and running commit
#[poise::command(slash_command)]
pub async fn status(ctx: Context<'_>) -> Result<()> {
    if !is_owner(&ctx) {
        return reply(ctx, "This command is restricted to the bot owner.").await;
    }

    let commit = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .await
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_else(|| "unknown".to_owned());
    reply(
        ctx,
        format!(
            "Bard {} · commit `{commit}`",
            spoticord_config::VERSION
        ),
    )
    .await
}

/// Restart the bot process (requires a supervisor such as systemd with Restart=always)
#[poise::command(slash_command)]
pub async fn restart(ctx: Context<'_>) -> Result<()> {
    if !is_owner(&ctx) {
        return reply(ctx, "This command is restricted to the bot owner.").await;
    }
    if !is_supervised() {
        return reply(
            ctx,
            "Restart is disabled: run the bot under a supervisor and set BOT_SUPERVISED=true.",
        )
        .await;
    }

    reply(ctx, "Restarting the bot now.").await?;
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    std::process::exit(0);
}

/// Fetch and build the configured GitHub branch, then restart the bot
#[poise::command(slash_command)]
pub async fn update(ctx: Context<'_>) -> Result<()> {
    if !is_owner(&ctx) {
        return reply(ctx, "This command is restricted to the bot owner.").await;
    }
    if !is_supervised() {
        return reply(
            ctx,
            "Update is disabled: run the bot under a supervisor and set BOT_SUPERVISED=true.",
        )
        .await;
    }

    let remote = match std::env::var("UPDATE_REMOTE") {
        Ok(remote) if valid_remote(&remote) => remote,
        _ => return reply(ctx, "Set UPDATE_REMOTE to the name of the Git remote for your own repository (for example, `origin`).").await,
    };
    let branch = std::env::var("UPDATE_BRANCH").unwrap_or_else(|_| "main".to_owned());
    if !valid_branch(&branch) {
        return reply(ctx, "UPDATE_BRANCH contains unsupported characters.").await;
    }

    if let Err(error) = ctx.defer_ephemeral().await {
        return Err(error.into());
    }

    let dirty = run("git", &["status", "--porcelain"]).await?;
    if !dirty.status.success() {
        return reply(
            ctx,
            "Couldn't inspect the Git working tree; update cancelled.",
        )
        .await;
    }
    if !String::from_utf8_lossy(&dirty.stdout).trim().is_empty() {
        return reply(ctx, "Update cancelled because the checkout has local changes. Commit or discard them before updating.").await;
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
                "Update stopped: fast-forward merge failed: {}",
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
                "The code updated, but the release build failed: {}",
                output_tail(&build.stderr)
            ),
        )
        .await;
    }

    reply(ctx, "Update built successfully. Restarting the bot now.").await?;
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    std::process::exit(0);
}

fn is_owner(ctx: &Context<'_>) -> bool {
    ctx.author().id.get() == OWNER_ID
}

fn is_supervised() -> bool {
    std::env::var("BOT_SUPERVISED").is_ok_and(|value| value.eq_ignore_ascii_case("true"))
}

fn valid_remote(remote: &str) -> bool {
    !remote.is_empty()
        && remote
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
}

fn valid_branch(branch: &str) -> bool {
    !branch.is_empty()
        && branch.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '/' | '.')
        })
}

async fn run(program: &str, args: &[&str]) -> Result<std::process::Output> {
    Ok(Command::new(program).args(args).output().await?)
}

fn output_tail(bytes: &[u8]) -> String {
    const MAX_CHARS: usize = 1200;
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
