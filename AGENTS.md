# Project instructions

Read `CLAUDE.md` for repository, build and data-preservation rules.

Read and follow `DESIGN.md` before changing any UI or interaction. The console
Rust port must preserve the established Go/iPod components and flows. Backend
rewrites do not authorize redesigns. Verify affected console screens against
the Go source and native-size reference captures. Explicit user instructions
take precedence.

Keep console design separate from the Mac companion's requested native macOS
design. Do not move desktop controls or Liquid Glass into the console.

Use English for code, comments, repository documents and commits. Preserve
unrelated work and user data. Keep generated builds, local test-run data and
captures out of commits unless the user requests those artifacts. Small,
intentional test fixtures may be included when needed for reproducible tests.
