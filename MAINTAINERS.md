# Maintainers

## Current maintainer

- **Dan Klaussen** ([@digidude](https://github.com/digidude)) started the project.

## Looking for maintainers

This is a hobby project. It isn't a product, nobody pays for it, and the original author doesn't plan to keep it for the long term. **If you use Klipper and want this to keep existing, please take part.** You don't have to take over all of it: each area below can have its own maintainer.

| Area | Where | Skills |
|---|---|---|
| Language server | `src/` | Rust, LSP |
| Grammar and syntax extensions | `syntax/` (submodule, [klipper-syntax](https://github.com/digidude/klipper-syntax)) | tree-sitter, a little C (`scanner.c`), TextMate |
| VS Code extension | `editors/vscode-klipper-ls/` | TypeScript |
| Zed extension | `editors/zed-klipper-ls/` | Rust (wasm), tree-sitter queries |
| Docs and triage | issues, READMEs | Klipper knowledge is enough |

## What maintaining involves

Light when things are quiet:
- Triage issues and answer questions: an hour or two a week.
- Review pull requests.
- Cut a release when something worth shipping has landed. It is one tag: see [docs/maintaining.md](docs/maintaining.md).
- When Klipper changes its docs or adds commands, run the coverage tests and fix any regression. That is the main recurring work, and the tests tell you what broke.

## How to become a maintainer

1. Send a few pull requests or help triage issues. Any area counts, docs included.
2. Open an issue titled "Maintainer: <your name>" saying what you'd like to look after.
3. Triage rights come first. Write access follows after a few merged contributions.

## Handing the project over

If the current maintainer steps away, the plan is to use GitHub's **transfer repository** feature. It keeps issues, releases, stars and links, and old URLs redirect.

1. Give the new maintainer, or a new GitHub organisation, admin rights and transfer `digidude/klipper-ls`.
2. Update the repository URL where it is written down:
   - `Cargo.toml`
   - `editors/vscode-klipper-ls/package.json` (`repository`, `bugs`) and `src/binary.ts` (`RELEASES_API`)
   - `editors/zed-klipper-ls/extension.toml` (`repository`) and `src/lib.rs` (`GITHUB_REPO`)
   - `.gitmodules` (the `syntax/` submodule URL), and klipper-syntax's own repository URLs if it moves too
   - the READMEs
3. Anything tied to a person's account does **not** transfer automatically and needs a hand-off: a VS Code Marketplace publisher and its token, an Open VSX namespace, and the Zed registry entry (a pull request to `zed-industries/extensions` changing the repository).
4. Cut a release so the extensions download binaries from the new location.

## Licence and contributions

MIT. Contributions are accepted under the same licence (inbound = outbound). MIT was chosen because it is the easiest to hand off: MIT code can be absorbed by a GPL project such as Klipper's own, but not the other way round. Klipper's and Marlin's docs are GPL-3.0; this project reads them at runtime and **never** bundles or copies them.
