import {
  BarChart3,
  CalendarDays,
  FileText,
  Image,
  Inbox,
  Layers3,
  Lightbulb,
  Plug,
  Settings2,
} from "lucide-react";
export type Route =
  | "queue"
  | "calendar"
  | "ideas"
  | "drafts"
  | "media"
  | "analytics"
  | "inbox"
  | "connections"
  | "settings";
export const routes: {
  id: Route;
  name: string;
  icon: typeof Layers3;
  group?: boolean;
}[] = [
  { id: "queue", name: "Publishing", icon: Layers3 },
  { id: "calendar", name: "Calendar", icon: CalendarDays },
  { id: "ideas", name: "Ideas", icon: Lightbulb },
  { id: "drafts", name: "Drafts", icon: FileText },
  { id: "media", name: "Media library", icon: Image },
  { id: "analytics", name: "Analytics", icon: BarChart3, group: true },
  { id: "inbox", name: "Inbox", icon: Inbox },
  { id: "connections", name: "Connections", icon: Plug, group: true },
  { id: "settings", name: "Settings", icon: Settings2 },
];
export const titles: Record<Route, [string, string]> = {
  queue: [
    "Your publishing rhythm",
    "A clear view of what’s next, and what needs a little care.",
  ],
  calendar: ["The bigger picture", "Make room for your next great idea."],
  ideas: [
    "A place for every spark",
    "Catch it now. Shape it when you’re ready.",
  ],
  drafts: ["Work in progress", "Good things take a few drafts."],
  media: [
    "Your creative collection",
    "Keep your photos and videos ready for their next moment.",
  ],
  analytics: [
    "Understand your impact",
    "Network performance, when your connections support it.",
  ],
  inbox: [
    "Keep the conversation going",
    "Bring supported conversations into one place.",
  ],
  connections: [
    "Your channels, connected",
    "Bring your accounts. Keep control of your credentials.",
  ],
  settings: [
    "Make it your own",
    "A little setup for a smoother publishing day.",
  ],
};
export const routeFromHash = (): Route =>
  routes.find((r) => r.id === location.hash.slice(1))?.id ?? "queue";
export const zone = Intl.DateTimeFormat().resolvedOptions().timeZone;
