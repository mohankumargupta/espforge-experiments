# Design: `examples` materializes editable projects instead of printing a file

Status: agreed. Revised once (see "Relationship between `examples` and `build`")
after rethinking what the two commands are for.

## Problem

`espforge examples` today is an interactive picker that prints the chosen
example's `example.espf.txt` verbatim to stdout. An example is more than that
file — the embedded tree holds `example.espf.txt`, `app/rust/app.rs`,
`diagram.json`, `chip.json`, wokwi `chip/` sources and a prebuilt `chip.wasm`
(see `espforge-examples/src/examples/...`). None of that ever reaches the user,
and nothing from the examples can be turned into firmware without manual
shuffling.

We want `examples` to hand the user a real folder they can modify and then
`espforge build` — the same way `build` hands them a folder they can
`cargo build`.

## Decisions (agreed)

- **Output = raw example folder.** An exact, self-contained copy of the embedded
  example tree — the *source material* stage, not a generated cargo project.
- **Scope of this change:** layer the foundation (library + low-level CLI).
  Reshaping the interactive command comes after.
- **CLI shape:** nested subcommands (`espforge examples list`,
  `espforge examples get ...`) — recommended, still reversible; bare
  `espforge examples` stays the interactive porcelain entry point.
- **App-logic layout:** `build` learns to find the example layout — auto-detect
  `src/app.rs` first, fall back to `app/rust/app.rs` (keep it if `app/rust/`
  anticipates other languages later, and prefer an explicit `--app` flag if
  ambiguity ever becomes a problem).
- **No desugaring by default.** A materialize+generate shortcut
  (`examples get ... --build`) can be added once the two stages are proven to
  work chained manually.
- **Architecture:** git-style split — plumbing (library + low-level CLI) vs
  porcelain (human-friendly command).

## Relationship between `examples` and `build`

Not a desugar: the two commands are **adjacent stages of one pipeline**.

```
espforge examples get 01.Basics/button --out mydir   (stage 1: fetch — embedded tree → editable copy, destination chosen by the user)
        │
        │  you edit:  button/example.espf.txt, button/src/app.rs
        ▼
espforge build button/example.espf.txt        (stage 2: generate — source → cargo project)
        │
        ▼
cd button/build && cargo build                (stage 3: compile — the toolchain's job)
```

- **`examples` = import.** Creates *modifiable source material* in the user's
  workspace. Never runs cargo. Promise: "here is a starting point you own".
- **`build` = derivation.** Takes source material and regenerates the cargo
  project. Its existing clobber contract (scaffold never overwrites existing
  files, `src/app.rs` copied verbatim on every run) is exactly what makes
  iteration safe: edit the `.espf`, re-run build any number of times.
- The pipeline's value is the **modification window** between stage 1 and 3 —
  which is why `examples` must not collapse into `build`. (`git clone` does not
  run `make`.)
- Sugar is allowed but explicit: a later flags-on porcelain opt-in
  (`examples get ... --build`) may chain stages 1+2; it is sugar stacked on top
  of both commands, not a redefinition of either.

## Terminology

- **Plumbing** — the library API and the non-interactive CLI vars. Scriptable;
  output for scripts, not humans; never prompts.
- **Porcelain** — the human-facing command(s). Owns all picking, defaults, and
  printed next-steps.

## Layers

### Layer 0 — plumbing library (`espforge-examples` crate, reworked)

Reframe the crate: today it is "the data dump for the examples command"; the
goal is a **read-only catalog of the embedded example tree**.

- Keep `categories()` / `Category` (empty categories are still dropped).
- Rework `Example` to expose **all** embedded files as `(relative_path,
  contents)`, alongside the derived metadata it already has (`title`, the espf
  document). Currently it keeps only `example.espf.txt` and discards the rest.
- `find("01.Basics/button") -> Option<&Example>` — one addressing scheme
  (`<category>/<name>`) used by every layer and any future command.
- `materialize(&Example, dest: &Path) -> Result<Vec<PathBuf>>` — write the
  embedded tree verbatim under `dest`.
  - **Never clobbers:** fails if any target file already exists (same contract
    as `scaffold` in `espforge-emit`). No partial writes.
  - Returns the list of written paths for the caller to print.
  - No `println!` inside, no UI imports, `&Path` in / `Result` out.

Nothing here knows about cliclack, stdout formatting, or CLI args.

### Layer 1 — plumbing CLI

Non-interactive subcommands of `examples`:

```
espforge examples list [category]
    One line per example: `category/name` plus title. Plain text, no ANSI.
    Nonzero exit when the tree is empty or the category is unknown.

espforge examples get <category>/<name> --out <dir>
    Materialize the example folder to disk, into the destination **the user
    chooses**. `--out` is required (no default): `examples` never assumes
    where a project should live in the user's workspace.
    Refuses to overwrite anything that exists, with a clear error.
```

Thin wiring over Layer 0. Exit codes carry failures (not-found, conflict);
nothing ever prompts.

### Layer 2 — porcelain (deferred)

Bare `espforge examples` keeps today's behavior (pickers + print) until this
follow-up reshapes it: same pickers, then a cliclack **Input** prompt asking for
the destination folder, pre-filled with a proposed default (the example's own
directory name, e.g. `button`) — Enter accepts it, the user can retype it. Then
delegate to `materialize` and print `build`-style next-steps for the full
pipeline:

    next: cd <dest>  → edit the example → espforge build <dest>/example.espf.txt

Because the picker only ever calls Layer 0, reshaping it later cannot break the
plumbing commands or scripts built on them.

### The `build` change (stage 2, small but separate)

Teach `espforge build` where app logic lives: prefer `src/app.rs` next to the
`.espf` file (today's contract), fall back to `app/rust/app.rs` (the embedded
examples' layout). This is what makes the stage-1 → stage-2 handover work with
zero glue and no migration of the embedded examples.

## Invariants that make the split stick

1. Plumbing functions: no stdout, no prompts, `&Path` in / `Result` out.
2. Plumbing CLI: no prompts, plain output, documented exit codes.
3. Porcelain formats output and picks defaults; plumbing never does.
4. All knowledge of *what's inside an example* lives in Layer 0 — no other
   crate or command may reach into `include_dir` directly.
5. `examples` never invokes `build`; chaining is the user's choice (or later,
   explicit sugar).

## Code change outline

- `espforge-examples/src/lib.rs`: extend the data model, add `find` +
  `materialize`; keep the existing public API compatible for
  `espforge/src/commands/examples.rs`.
- `espforge/src/main.rs`: `Examples` gains an optional nested subcommand enum;
  no subcommand → today's interactive flow.
- `espforge/src/commands/examples.rs`: split into `interactive()` (existing
  logic, untouched) plus thin `list()` / `get()` wrappers.
- `espforge/src/commands/build.rs`: app.rs discovery — `src/app.rs` first, then
  `app/rust/app.rs` (and while there, reword the error to name both layouts).

## Tests

- `materialize` into a tempdir: happy path (all files land, relative paths
  preserved), clobber refusal, missing-`example.espf.txt` handling.
- Plumbing CLI: `list` output format and exit codes, `get` happy path and
  conflict exit code, unknown `category/name`, missing/empty `--out` rejected
  by the CLI parser (usage error, not a runtime surprise).
- App-layout discovery: `.espf` with only `src/app.rs`, only `app/rust/app.rs`,
  and both (must prefer `src/app.rs`).
- Later, Layer 2: with stdin scripted, the picker materializes the selected
  example exactly as `get` does.
