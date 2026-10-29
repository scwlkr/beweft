import { ErrorNotice, Time } from "../components/shared";

export function isOwnerAttestation(attempt: Record<string, unknown>) {
  const receipt = attempt.receipt;
  return (
    typeof receipt === "object" &&
    receipt !== null &&
    "verification" in receipt &&
    receipt.verification === "owner_attestation"
  );
}

export function AttemptHistory({
  attempts,
  error,
}: {
  attempts: Record<string, unknown>[];
  error: string;
}) {
  return (
    <section className="detail-section">
      <h3>Attempt history</h3>
      {error ? (
        <ErrorNotice message={error} />
      ) : attempts.length === 0 ? (
        <p className="muted">No attempts yet.</p>
      ) : (
        <ol className="attempts">
          {attempts.map((attempt, index) => {
            const receipt = attempt.receipt as Record<string, unknown> | null;
            const detail = receipt?.detail as Record<string, unknown> | null;
            const owner = isOwnerAttestation(attempt);
            return (
              <li key={String(attempt.id ?? index)}>
                <strong>
                  {owner ? "Owner attestation" : "Delivery attempt"} ·{" "}
                  {String(attempt.state ?? "unknown")}
                </strong>
                {typeof attempt.started_at === "string" && (
                  <small>
                    <Time value={attempt.started_at} />
                  </small>
                )}
                {owner && (
                  <p>
                    This outcome was recorded by the owner; it is not provider
                    confirmation.
                  </p>
                )}
                {typeof detail?.note === "string" && (
                  <p>Reconciliation / completion note: {detail.note}</p>
                )}
                {attempt.error != null && <p>{String(attempt.error)}</p>}
                {receipt && (
                  <details>
                    <summary>
                      {owner
                        ? "Attestation record"
                        : "Recorded provider receipt"}
                    </summary>
                    <pre className="receipt-json">
                      {JSON.stringify(receipt, null, 2)}
                    </pre>
                  </details>
                )}
              </li>
            );
          })}
        </ol>
      )}
    </section>
  );
}
