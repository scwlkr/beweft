# REA investigation tooling

Checked 2026-10-29. REA is research tooling, not a beweft runtime dependency.

## Installation and verification

Installed official [`rea-agents` 6.8.0](https://www.npmjs.com/package/rea-agents/v/6.8.0), linked by the [official repository](https://github.com/morluto/rea). Reused an existing Node 26.12.1 runtime: the default Node 26.5.0 does not satisfy this release's engines. No runtime download, native analysis engine, browser download, or optional Frida installation was needed. Package lifecycle scripts were disabled.

The official setup registered Codex and installed its matching workflow skill. Its launcher was pinned to the compatible runtime. Invoking the installed REA entry script with the existing Node 26.12.1 executable and `doctor --client codex --skill --json` returned healthy, with aligned registration and skill. Comparison with the setup recovery snapshot confirmed unrelated Codex configuration was unchanged. New MCP registrations require a client restart; the equivalent CLI works immediately. Machine-specific paths and configuration remain outside this repository. Only the MCP launcher is runtime-pinned: a bare shell `rea` command still inherits PATH and needs a compatible Node runtime. The setup recovery snapshot was removed after unrelated configuration equality was verified.

## Browser boundary

[REA's browser provider](https://github.com/morluto/rea/blob/main/docs/browser-observation.md) passively attaches to an already-running Chrome-family browser through CDP. It does not navigate, click, publish, or close the browser. Browser tools must drive authorized interactions separately.

The existing owner Chrome process initially exposed neither a CDP listener nor a `DevToolsActivePort` file. The owner subsequently explicitly authorized permanent remote debugging. It was enabled through Chrome's own settings without relaunching Chrome or changing its profile. REA then successfully discovered and passively inspected the authenticated Buffer tab using an exact `https://publish.buffer.com` origin filter. Text, scripts, storage keys/fingerprints, console text and body-shape capture stayed off. The initial home inspection observed 1,715 DOM nodes with privacy-policy filtering and explicit attach limitations. Public unauthenticated requests to Buffer's marketing and publishing entry pages had returned HTTP 403; no bypass was attempted.

Chrome exposes this setting at `chrome://inspect/#remote-debugging`. Use `list-browser-targets` with an exact Buffer origin filter and `inspect-web-page` with the selected target. Do not collect cookies, tokens, raw response bodies, private post text, or unrelated origins. Keep source and screenshot collection off unless specifically needed. The setting remains enabled per the owner's explicit instruction; analysis attachments themselves are bounded and detach on completion.

A subsequent 120-second `observe-web-session` covered navigation through ideas,
calendar, queue, drafts, sent history, the untouched multi-network composer,
community, insights, channel settings and preferences. REA returned 13 timeline
entries, no dropped events and no out-of-policy destination. Its coverage is
attach-limited navigation metadata; it does not prove write behavior. Results
were reduced in memory to counts and limitations, without retaining a raw session
bundle. The accompanying UI inventory records the visible interaction details.

## Completed static investigation

REA analyzed the [Postiz YouTube adapter at revision `64f00fd`](https://github.com/gitroomhq/postiz-app/blob/64f00fd6d0e9dfd55d202ff83983e7576e08bc2e/libraries/nestjs-libraries/src/integrations/social/youtube.provider.ts) as inert TypeScript. The selected 33,615-byte source parsed successfully: one source, 3,609 AST nodes, 46 findings, no parse failure. REA retained request, promise, resource, import, and local call relationships. Imported dependencies were outside the slice; application coverage was partial and 720 semantic unknowns remained. These findings establish static structure only, not a successful live connection or publication.

Source inspection alongside the graph identifies useful design lessons: OAuth requests offline access; refresh retains the previous refresh token when Google omits a replacement; upload work has a resumable session probe and separate pending/finalization phases; quota and revoked-token failures receive distinct messages. These are patterns to implement independently, subject to Google's current documented API. No source was incorporated into beweft.

The downloaded source slice and 3.2 MB REA evidence bundle were deleted after extracting this summary. No Buffer session captures or account data were saved.
