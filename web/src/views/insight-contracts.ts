import { z } from "zod";
export const MetricSchema = z.object({
  publication_id: z.string(),
  channel_id: z.string(),
  provider: z.string(),
  text: z.string(),
  external_url: z.string().nullable(),
  observed_at: z.string(),
  metrics: z.array(
    z.object({ name: z.string(), value: z.number().nullable() }),
  ),
});
export const ItemSchema = z.object({
  id: z.string(),
  channel_id: z.string(),
  channel_name: z.string(),
  provider: z.string(),
  item: z.object({
    id: z.string(),
    parent_id: z.string().nullable().optional(),
    author: z.string(),
    text: z.string(),
    url: z.string().nullable().optional(),
    created_at: z.string().nullable().optional(),
    reply_context: z
      .object({
        visibility: z.string(),
        recipients: z.array(z.object({ id: z.string(), acct: z.string() })),
      })
      .nullish(),
  }),
  read_at: z.string().nullable(),
  synced_at: z.string(),
  can_reply: z.boolean(),
});
export const Analytics = z.object({
  availability: z.string(),
  reason: z.string().optional(),
  summary: z.object({
    scheduled: z.number(),
    published: z.number(),
    attention: z.number(),
  }),
  metrics: z.array(MetricSchema),
});
export const Inbox = z.object({
  availability: z.string(),
  reason: z.string().optional(),
  items: z.array(ItemSchema),
});
export type Result = z.infer<typeof Analytics> | z.infer<typeof Inbox>;
