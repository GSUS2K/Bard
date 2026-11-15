export default function Home() {
  return (
    <main className="flex min-h-screen items-center justify-center px-6">
      <section className="max-w-lg space-y-4 text-center">
        <h1 className="text-3xl font-semibold">Bard account linking</h1>
        <p className="text-muted-foreground">
          To connect Spotify, use <code>/link</code> in Discord and open the
          private link the bot sends you.
        </p>
        <p className="text-muted-foreground text-sm">
          This page links your account for playback controls and track info. It
          does not stream audio into Discord.
        </p>
      </section>
    </main>
  );
}
