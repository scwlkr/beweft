import { z } from "zod";
const Permissions = z.object({
  permissions: z.object({
    source: z.string(),
    reported_scopes: z.array(z.string()),
    actions: z.record(
      z.string(),
      z.object({
        status: z.enum(["granted", "missing", "unknown"]),
        required_scopes: z.array(z.string()),
        missing_scopes: z.array(z.string()).nullable(),
      }),
    ),
  }),
});
export function ChannelPermissions({
  capabilities,
  showScopes = false,
}: {
  capabilities: unknown;
  showScopes?: boolean;
}) {
  const parsed = Permissions.safeParse(capabilities);
  if (!parsed.success)
    return (
      <small className="muted">Permission grants have not been reported.</small>
    );
  const permission = parsed.data.permissions;
  return (
    <details className="channel-permissions">
      <summary>Account permissions</summary>
      <p className="muted">
        Granted means the reported scopes are present; provider approval and
        live behavior remain separate. Unknown grants are not verified.
      </p>
      <dl>
        {Object.entries(permission.actions).map(([action, grant]) => (
          <div key={action}>
            <dt>{action}</dt>
            <dd>
              {grant.status === "unknown"
                ? "Unknown · Provider has not confirmed this permission"
                : grant.status}
              {!!grant.missing_scopes?.length &&
                ` · Reconnect for: ${grant.missing_scopes.join(", ")}`}
            </dd>
          </div>
        ))}
      </dl>
      <small className="muted">Scope source: {permission.source}</small>
      {showScopes && (
        <p className="muted">
          Reported scopes:{" "}
          {permission.reported_scopes.length
            ? permission.reported_scopes.join(", ")
            : "none reported"}
          .
        </p>
      )}
    </details>
  );
}
