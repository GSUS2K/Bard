import "@/styles/globals.css";
import { Inter as FontSans } from "next/font/google";
import type { Metadata } from "next";

import { cn } from "@/lib/utils";

const fontSans = FontSans({ subsets: ["latin"], variable: "--font-sans" });

export const metadata: Metadata = {
  title: "Bard Account Linking",
  description: "Link your Spotify account to Bard",
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html className="dark h-full" lang="en" suppressHydrationWarning>
      <head>
      </head>
      <body
        className={cn(
          "h-full bg-background font-sans antialiased",
          fontSans.variable,
        )}
      >
        {children}
      </body>
    </html>
  );
}
