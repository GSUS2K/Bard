import BlurFade from "@/components/magicui/blur-fade";
import Particles from "@/components/magicui/particles";
import ShineBorder from "@/components/magicui/shine-border";
import { db } from "@/db";
import { linkRequests } from "@/db/schema";
import { getUserInfo } from "@/lib/discord";
import { CDN } from "@discordjs/rest";
import { eq } from "drizzle-orm";
import { notFound } from "next/navigation";
import { AnimatedUserIcon } from "./components";
import {
  Card,
  CardContent,
  CardFooter,
  CardHeader,
} from "@/components/ui/card";
import { AlertTriangle, ExternalLinkIcon } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";

type Params = Promise<{ request: string }>;

export default async function LinkAccountPage({ params }: { params: Params }) {
  const { request } = await params;

  const entry = await db.query.linkRequests.findFirst({
    where: eq(linkRequests.token, request),
  });

  if (!entry || entry.expires < new Date()) {
    notFound();
  }

  const cdn = new CDN();
  const user = await getUserInfo(entry.user_id);
  const avatar = user.avatar
    ? cdn.avatar(user.id, user.avatar)
    : cdn.defaultAvatar(
        user.discriminator === "0000"
          ? Number((BigInt(user.id) >> 22n) % 6n)
          : Number(user.discriminator) % 5,
      );

  const searchParams = new URLSearchParams({
    response_type: "code",
    client_id: process.env.SPOTIFY_CLIENT_ID!,
    redirect_uri: process.env.SPOTIFY_REDIRECT_URI!,
    scope: [
      "user-read-private",
      "user-read-playback-state",
      "user-read-currently-playing",
      "user-modify-playback-state",
      "playlist-read-private",
      "playlist-read-collaborative",
    ].join(" "),
    state: request,
    show_dialog: "true",
  });
  const authorizeUrl = `https://accounts.spotify.com/authorize?${searchParams.toString()}`;

  return (
    <div className="relative flex h-full items-center justify-center">
      <BlurFade inView yOffset={12}>
        <ShineBorder
          className="contents p-0 md:grid [&>div:first-child]:hidden md:[&>div:first-child]:block"
          color={["#444444", "#444444"]}
        >
          <Card className="flex min-h-full w-full flex-col rounded-none md:min-h-auto md:max-w-md md:rounded-md">
            <CardHeader className="bg-muted flex items-center gap-4 p-6">
              {/* Images */}
              <AnimatedUserIcon image={avatar} />

              <div className="w-full">
                <div className="text-base font-medium md:text-lg">
                  Link your Spotify account
                </div>
                <p className="text-muted-foreground text-xs md:text-sm">
                  Connect your Spotify account to check and control playback
                  from Discord.
                </p>
              </div>
            </CardHeader>
            <CardContent className="flex flex-col justify-between space-y-4 p-6">
              <div className="space-y-4">
                <div>
                  <div className="text-sm font-medium md:text-base">
                    We need some permissions first
                  </div>
                  <p className="text-muted-foreground mt-2 text-sm md:text-base">
                    These permissions let Bard show your current track and
                    control playback on your Spotify devices.
                  </p>
                </div>

                <Alert className="">
                  <AlertTriangle className="size-[1em] translate-y-0.5 text-yellow-300!" />
                  <AlertTitle className="text-sm">Playback stays on Spotify</AlertTitle>
                  <AlertDescription className="text-xs">
                    Music plays on your Spotify device. Bard does not send
                    Spotify audio into Discord voice channels.
                  </AlertDescription>
                </Alert>
              </div>
            </CardContent>

            <CardFooter className="relative flex-1 items-end overflow-hidden">
              <div className="flex w-full items-end">
                <Button variant="outline" className="mr-0 ml-auto" asChild>
                  <a href={authorizeUrl}>
                    <ExternalLinkIcon className="mr-2 size-4" />
                    Authenticate with Spotify
                  </a>
                </Button>
              </div>
            </CardFooter>
          </Card>
        </ShineBorder>
      </BlurFade>

      <Particles
        className="absolute inset-0 -z-10 hidden md:block"
        quantity={200}
        ease={80}
        color="#ffffff"
        refresh
      />
    </div>
  );
}
