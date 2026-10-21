[Documentation](../README.md)

# Writing documentation

Update documentation in the same change as the behavior it describes. Follow
the repository's [working agreements](../../AGENTS.md#writing-documentation).

## Choose a location

The root [README](../../README.md) introduces the product. The
[documentation index](../README.md) routes readers to a guide or reference.

| Content | Location |
| --- | --- |
| First launch and basic use | `docs/getting-started.md` |
| Build, test, asset generation, profiling, and packaging procedures | `docs/development/` |
| Ownership, lifecycle, invariants, and design explanations | `docs/architecture/` |
| Command-line arguments, environment variables, and file locations | `docs/reference/` |
| Component-specific build, installation, and configuration | `components/<component>/README.md` |
| Asset formats, inventories, schemas, and notices | `assets/<collection>/README.md` |
| Dependency provenance and local patch records | `vendor/README.md`, `LOCAL_CHANGES.md`, or `PATCHES.md` beside the dependency |

Keep each fact in one page and link to it from elsewhere. Preserve upstream
dependency manuals and changelogs; record project-specific patches separately.

Historical measurements belong with the profiling procedure. Identify their
date, hardware, workload, and limits. Describe implemented behavior in architecture
pages and label any unimplemented work explicitly.

## Structure a page

Start central pages with a relative link to the documentation index and one
level-one title. Add an introduction only when it gives information beyond the
title. Use sentence case for headings without skipping levels.

Use descriptive titles such as “Build from source” or “Settings and session
files.” Use numbered lists for procedures, bullets for parallel choices, and
tables for platform differences and option mappings. Choose sections for the
subject; omit automatic summaries and related-topic lists. Put links where they
help the reader follow an instruction or understand a detail.

## Write direct instructions

- Address the reader as “you” when needed. Use active verbs: “Run,” “Open,”
  “Set,” and “Build.”
- State prerequisites before the commands that need them. Say where to run a
  command and identify the shell when syntax differs by platform.
- Explain the expected artifact or observable result after a procedure.
- Use exact names for UI labels, arguments, paths, and identifiers. Format UI
  labels in **bold** and code identifiers and paths in backticks.
- Avoid filler such as “just,” “simply,” and “obviously.” Support performance
  claims with measurements and describe their limits.
- Avoid time-dependent wording such as “new” and “currently.” State the behavior,
  version, status, or measurement date instead.

For example:

| Avoid | Prefer |
| --- | --- |
| Simply run the packaging helper. | Run `bash scripts/package-macos.sh dist/package` from the repository root. |
| The new renderer is faster. | The dated benchmark reports GPU and CPU timings for the listed workload. |
| This builds the thing you need. | The build produces `target/release/fragile-notepad.exe`. |

## Format commands and links

Use fenced code blocks with `powershell`, `sh`, `python`, or `text` as appropriate.
Keep copyable commands free of shell prompt markers. Use `python` on Windows and
`python3` on Unix when showing separate commands.

Use `-j 1` in Cargo build and test examples. Include `-- --test-threads=1` for
graphics and memory-intensive tests. Full validation procedures should set
`CARGO_BUILD_JOBS=1` and `RUST_TEST_THREADS=1`.

Use relative links inside the repository and descriptive link text. Link to
specific sections when useful. When moving a page or renaming a heading, update
incoming links, including links in local dependency patch notes. Do not link to
an ignored generated artifact as though it were tracked documentation.

## Review a documentation change

1. Check commands, defaults, file paths, and status claims against the relevant
   source or workflow.
2. Check every changed relative link and heading anchor. Confirm the index links
   to new pages and other links lead to the page that owns the topic.
3. Confirm that moved material preserves configuration constraints, provenance,
   notices, and benchmark limitations.
4. Review Markdown headings, tables, fenced blocks, and screenshots in the
   repository renderer. Run `git diff --check` for whitespace errors.
