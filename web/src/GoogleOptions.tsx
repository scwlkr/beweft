import { Field } from "./components/shared";

export function GooglePhotoDescriptionNotice() {
  return (
    <div className="notice google-photo-notice" role="status">
      Photo descriptions stay in beweft and are not sent to Google. Other
      selected channels can use descriptions where supported.
    </div>
  );
}

function object(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {};
}

export function GoogleOptions({
  value,
  onChange,
}: {
  value: Record<string, unknown>;
  onChange: (value: Record<string, unknown>) => void;
}) {
  const topic = String(value.topic_type ?? "STANDARD");
  const event = object(value.event);
  const offer = object(value.offer);
  const action = object(value.call_to_action);
  const setOptional = (key: string, input: string) => {
    const next = { ...value };
    if (input) next[key] = input;
    else delete next[key];
    onChange(next);
  };
  const setNested = (key: "event" | "offer", field: string, input: string) => {
    const next = { ...object(value[key]) };
    if (input) next[field] = input;
    else delete next[field];
    const options = { ...value };
    if (key === "event" || Object.keys(next).length) options[key] = next;
    else delete options[key];
    onChange(options);
  };
  return (
    <section className="provider-options form-stack">
      <h3>Google Business Profile post</h3>
      <p className="muted">
        Posts can include up to 1,500 characters of text and at most one JPEG or
        PNG photo. Photos must be 10 KiB–5 MiB and at least 250 pixels on each
        side; Google needs a reachable public HTTPS URL to fetch them. Google
        project approval, account access, and location eligibility still apply;
        connecting a location does not verify permission to publish.
      </p>
      <Field label="Post type">
        <select
          value={topic}
          onChange={(event) => {
            const next: Record<string, unknown> = {
              ...value,
              topic_type: event.target.value,
            };
            if (event.target.value === "STANDARD") {
              delete next.event;
              delete next.offer;
            } else {
              next.event = object(next.event);
              if (event.target.value === "OFFER") delete next.call_to_action;
              else delete next.offer;
            }
            onChange(next);
          }}
        >
          <option value="STANDARD">Update</option>
          <option value="EVENT">Event</option>
          <option value="OFFER">Offer</option>
        </select>
      </Field>
      <Field
        label="Language code"
        hint="Optional language code, such as en-US."
      >
        <input
          value={String(value.language_code ?? "")}
          onChange={(event) => setOptional("language_code", event.target.value)}
        />
      </Field>
      {(topic === "EVENT" || topic === "OFFER") && (
        <>
          <p className="muted">
            Enter the event dates and times in the business location’s local
            time. These values are sent as entered, without converting from your
            browser’s timezone. All event fields are required, and the end must
            be after the start.
          </p>
          <Field label="Event title">
            <input
              required
              value={String(event.title ?? "")}
              onChange={(input) =>
                setNested("event", "title", input.target.value)
              }
            />
          </Field>
          {(["start", "end"] as const).map((boundary) => (
            <div key={boundary} className="form-stack">
              <Field label={`Event ${boundary} date`}>
                <input
                  type="date"
                  required
                  value={String(event[`${boundary}_date`] ?? "")}
                  onChange={(input) =>
                    setNested("event", `${boundary}_date`, input.target.value)
                  }
                />
              </Field>
              <Field label={`Event ${boundary} time`}>
                <input
                  type="time"
                  required
                  value={String(event[`${boundary}_time`] ?? "")}
                  onChange={(input) =>
                    setNested("event", `${boundary}_time`, input.target.value)
                  }
                />
              </Field>
            </div>
          ))}
        </>
      )}
      {topic !== "OFFER" && (
        <>
          <Field label="Call to action">
            <select
              value={String(action.type ?? "")}
              onChange={(event) => {
                const next = { ...value };
                const type = event.target.value;
                if (!type) delete next.call_to_action;
                else
                  next.call_to_action = {
                    type,
                    ...(type !== "CALL" && typeof action.url === "string"
                      ? { url: action.url }
                      : {}),
                  };
                onChange(next);
              }}
            >
              <option value="">No call to action</option>
              <option value="BOOK">Book</option>
              <option value="ORDER">Order</option>
              <option value="SHOP">Shop</option>
              <option value="LEARN_MORE">Learn more</option>
              <option value="SIGN_UP">Sign up</option>
              <option value="CALL">Call</option>
            </select>
          </Field>
          {action.type && action.type !== "CALL" && (
            <Field
              label="Call to action URL"
              hint="Required. Use an HTTP or HTTPS URL."
            >
              <input
                type="url"
                required
                value={String(action.url ?? "")}
                onChange={(event) =>
                  onChange({
                    ...value,
                    call_to_action: {
                      type: action.type,
                      url: event.target.value,
                    },
                  })
                }
              />
            </Field>
          )}
          {action.type === "CALL" && (
            <p className="muted">
              Google uses the location’s phone number for Call.
            </p>
          )}
        </>
      )}
      {topic === "OFFER" && (
        <>
          <p className="muted">
            Offer details are optional. Offers do not use a call to action.
          </p>
          <Field label="Coupon code">
            <input
              value={String(offer.coupon_code ?? "")}
              onChange={(event) =>
                setNested("offer", "coupon_code", event.target.value)
              }
            />
          </Field>
          <Field
            label="Online redemption URL"
            hint="Optional HTTP or HTTPS URL."
          >
            <input
              type="url"
              value={String(offer.redeem_online_url ?? "")}
              onChange={(event) =>
                setNested("offer", "redeem_online_url", event.target.value)
              }
            />
          </Field>
          <Field label="Offer terms and conditions">
            <textarea
              rows={2}
              value={String(offer.terms_conditions ?? "")}
              onChange={(event) =>
                setNested("offer", "terms_conditions", event.target.value)
              }
            />
          </Field>
        </>
      )}
    </section>
  );
}
