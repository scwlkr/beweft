export function Unicorn({
  size = 40,
  className = "",
}: {
  size?: number;
  className?: string;
}) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 64 64"
      className={className}
      fill="none"
      aria-hidden="true"
    >
      <path d="M35 9 44 2l-3 17" fill="currentColor" opacity=".6" />
      <path
        d="M19 56c-4-11-4-24 5-33l1-12 10 8c9-2 14 2 16 9l6 7-5 9-14-2c-3 4-3 9-2 14H19Z"
        fill="currentColor"
      />
      <path
        d="M24 23C14 25 8 33 9 43c1 7 4 11 7 14-2-11 1-21 9-27"
        fill="currentColor"
        opacity=".45"
      />
      <path
        d="M23 33c-3 8-2 15 1 21"
        stroke="var(--surface)"
        strokeWidth="2"
        strokeLinecap="round"
      />
      <circle cx="42" cy="29" r="2" fill="var(--surface)" />
      <path
        d="m49 39 4-2"
        stroke="var(--surface)"
        strokeWidth="2"
        strokeLinecap="round"
      />
    </svg>
  );
}
export function Network({ provider }: { provider: string }) {
  const labels: Record<string, string> = {
    youtube: "▶",
    x: "𝕏",
    twitter: "𝕏",
    facebook: "f",
    instagram: "◎",
    threads: "@",
    tiktok: "♪",
    linkedin: "in",
    bluesky: "🦋",
    mastodon: "m",
    pinterest: "P",
    manual: "↗",
    fixture: "T",
  };
  return (
    <span className={`network network-${provider}`} aria-label={provider}>
      {labels[provider] ?? provider.slice(0, 1).toUpperCase()}
    </span>
  );
}
