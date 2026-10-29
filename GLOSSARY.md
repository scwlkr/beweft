# Glossary

- **Brand:** A group of social channels and their associated content.
- **Channel:** An account or page connected to a social network.
- **Queue:** Scheduled channel-specific posts and their posting slots.
- **Provider:** A social network exposing capabilities through its own APIs.
- **Post:** Editable source content; it can have a different variant per channel.
- **Publication:** An immutable snapshot for one channel at one scheduled time.
- **Attempt:** One durable claim to perform a publication or reply operation.
- **Receipt:** A provider's response identifying a result; it is not fabricated
  from a local success message.
- **Uncertain:** A write may have reached the provider, but its outcome is not
  known. Reconcile it before permitting another attempt.
- **Manual handoff:** Prepared content the owner must finish on the network.
  Owner confirmation is recorded separately from a verified provider result.
- **Grant:** Encrypted authorization bound to a provider app and destination.
- **Fixture:** An isolated simulated provider used to test behavior. Passing a
  fixture does not establish live access, eligibility or publication.
