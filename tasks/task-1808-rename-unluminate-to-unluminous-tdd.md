# task-1808 — Renaming Unluminate to Unluminous

> This document is deliberately exempt from its own substitution: it is about a rename, so the old
> name has to survive in it or it stops saying anything.

## What this is

The product is renamed a second time. `Quill → Unluminate` happened five days ago under task-1800;
`Unluminate → Unluminous` is the same operation with the same surfaces, so this document says what is
different rather than restating the parts that already have a proven procedure.

Three cases, and only three exist anywhere in the tree:

| from | to | occurrences in the product repo |
|---|---|---|
| `Unluminate` | `Unluminous` | 3,702 |
| `unluminate` | `unluminous` | 4,810 |
| `UNLUMINATE` | `UNLUMINOUS` | 165 |

**8,701 across 406 files** in the product repo, 222 across 16 tracked files in ai-service, 21 across 4
in claude-settings, 83 across 11 in blackrainbowlabs.

## The one thing that is genuinely easier this time

Both names are **ten characters**. `a_module_path_is_read_back_to_its_keyword` asserts byte offsets
that count the letters of the crate name — `unluminate_core` and `unluminous_core` are the same
length, so `stem: 21..25` does not move. That is a reason to expect fewer surprises, not a reason to
skip the suite: two of the three assertions task-1800 tripped over are length-independent.

- The **truncation** assertion still breaks. `typing_in_find_in_files_leaves_the_document_alone` types
  the name and asserts what is left after a backspace — the old expectation was nine of the old ten
  letters, which is not the word and which no substitution on the word can see. It becomes
  `unluminou`.
- The **deliberate typo** still breaks. `an_unresolved_segment_answers_nothing_rather_than_everything`
  imports a misspelling of the core crate to prove an unknown segment resolves to nothing. Also not
  the word.

Both are found by running the suite, which is the only way to find them. The plan is: substitute,
build, run `cargo test --workspace`, and fix what fails — not grep for these two by hand and assume
that is all of them.

## The rest of the traps, carried forward verbatim

1. **`cargo clean --release` is mandatory after moving the checkout.** Last time, 72 generated files
   under `target/release/build` carried a `file!()` path from the old directory, cargo did not
   consider them changed, and the old absolute path shipped inside the released executable. The
   release had to be redone as 0.34.1. So: rename, move, `cargo clean --release`, rebuild, and
   **grep the built `unluminous.exe` for the old byte string** before the release goes out. That grep
   is the acceptance test for this item, not the clean command.
2. **`%APPDATA%\Unluminate` and `%LOCALAPPDATA%\Unluminate` are copied, never moved.** 9.2 MB and
   137.5 MB respectively — trivially affordable against 1,230 GB free, so the adapters subtree gets
   copied like everything else. Paths and ids inside are rewritten: the stored theme is
   `unluminate/dark → unluminous/dark`, and `debug.lldb` points into `%LOCALAPPDATA%`. A renamed
   binary reading a folder that does not exist comes up factory-fresh and silently loses the font,
   the 0.83 background opacity, the pane sizes, the recent projects, the saved chat conversations and
   the debug-adapter paths.
3. **A new Inno `AppId` GUID.** The old one was `{520016FC-ED64-4B3C-AAAC-D75ABC651C30}` and the
   installed row recorded `...\Local\Programs\Unluminate`. Keeping the GUID makes Inno install
   `unluminous.exe` back into that folder — the one folder name this ticket exists to be rid of. So a
   fresh GUID, and **the old product is uninstalled by its own uninstaller first**, which is what
   clears the Add-or-Remove-Programs row, the PATH entry, `App Paths\unluminate.exe`, the desktop
   shortcut, the context-menu verbs and the `OpenWithProgids` values. Each is verified gone.
4. **The custom URI scheme is the browser pane's project origin**, 27 occurrences, and wry rewrites it
   to `http://<scheme>.<origin>/`. Both halves move together or a local page loads blank with no error
   and nothing in the log.
5. **Per-project state folders** exist at the repo root, `sample/`, `sample-diagrams/` and — still —
   `C:\jason\dev\ai-service\--version\`, the stray directory a mistyped flag created. Copied to the
   new name, originals parked under `_agent_output/task-1808-rename/` rather than deleted.
6. **Something will be holding files in the checkout.** Last time three orphaned `conhost.exe` and an
   abandoned agent terminal blocked the move. An agent terminal is closed through the terminal
   daemon's own `kill` control message, never by killing the process, so the session stays resumable.

## Out of scope, as decisions rather than oversights

- **`C:\jason\dev\unluminate-site` and unluminous.com** — task-1806 owns the new product site, the
  domain, its Cloudflare zone, its proxy route and its Service Manager registration. That directory
  does not exist on disk yet; if it appears mid-run it is still not mine to touch.
- **task-1806's own title, description and TDD.** It is `in_progress` with an agent on it. Editing the
  instructions underneath a running ticket is worse than leaving a stale product name in them.
- **task-1800's title and its TDD.** "Rename quill to unluminate" is a true statement about a rename
  that produced Unluminate. Rewriting it to say Unluminous would make it false.
- **The 269 task comments.** They are a transcript — QA rejections in Jason's words, measurements
  quoted at the time, and the previous rename's own thread. A record edited to agree with the present
  is not a record.
- **`_agent_output/` in the product repo**, `installer/dist/`, git history, and existing tag names.
  The single carve-out is `_agent_output/task-1793-…-video/`, because the site README names that
  folder as the only way to regenerate the product video.
- **The bytes inside pre-rename installers.** They really were called what they are called. Assets on
  GitHub get renamed; their contents are a fact about a historical build.

## Order of work

1. Product repo substitution → crates, CLI, binaries, installer, project-state folders.
2. `cargo clean --release`, full build, `cargo test --workspace`, fix the assertions the suite finds.
3. Move the checkout, clean again, rebuild, grep the binary.
4. GitHub rename in place (`PATCH /repos/...` keeps the redirect alive), remote repointed, 63 releases
   retitled and their assets renamed.
5. ai-service, claude-settings, blackrainbowlabs — then rebuild and redeploy both sites.
6. Notebook notes through `POST /notebook/tdd` with the existing `noteId`, so the search vector and
   the RAG index are rebuilt rather than left answering with the old words and the deep links already
   pasted into task comments keep resolving.
7. Board: 43 rows carrying the old `agent_project_id`, and the titles and descriptions of tickets that
   name the product. Terminal daemon state repointed at the new checkout.
8. Uninstall, install, drive the installed app for real, release `-Part minor`.

## Done means

`unluminous-cli instances` finds a window, `unluminous-cli status --json` answers `Unluminous 0.37.0`,
`Unluminous → About Unluminous` says the same, one row in Add or Remove Programs, `cargo test
--workspace` green, the built executable free of the old byte string, both sites verified on prod, and
the release published with the installer attached.

---

## What actually happened

Released as **Unluminous 0.37.0** — https://github.com/jasonmcaffee/unluminous/releases/tag/v0.37.0

**The numbers.** 8,701 occurrences across 406 files and 89 renamed paths in the product repo; 222
across 16 files in ai-service; 21 across 4 in claude-settings; 83 across 11 in blackrainbowlabs. Only
three casings existed anywhere, which is why a scripted substitution was the right tool for the prose.

**The three assertions, and how each one behaved.**

1. `typing_in_find_in_files_leaves_the_document_alone` **failed**, exactly as predicted:
   `left: "unluminou"`, `right: "unluminat"`. Fixed.
2. `an_unresolved_segment_answers_nothing_rather_than_everything` **passed while still being wrong**,
   which the last rename did not have to deal with. It asserts that `unluminat_core` resolves to
   `None`, and a stale misspelling of a *renamed* crate still resolves to `None` — so the assertion
   held while no longer testing what it was written to test. The suite could not catch it in either
   direction. It was found by grepping for near-misses of the name once the suite had proved that
   shape of test existed, and is now `unluminou_core`. **A test that cannot fail is not evidence**, and
   this is the one place where "run the suite" was not enough.
3. `a_module_path_is_read_back_to_its_keyword` **passed untouched**. `stem: 21..25` counts the letters
   of `unluminous_core`, which is the same length as `unluminate_core`. The ten-character coincidence
   was real and it paid.

A fourth test, `the_case_of_a_search_can_be_insisted_on`, failed once with kittest's
`Harness::run exceeded max_steps (4)` and passed on the two runs either side of it — the known repaint
flake, not the rename.

**The snapshots.** 163 of the 165 failures were images: the menu bar and the project folder are drawn
in nearly every screenshot. Regenerated with `UPDATE_SNAPSHOTS=1`, then re-run **without** it —
because an update run cannot fail an image test, so only the second run is evidence. **2,779 pass, 0
fail.**

**The binary grep came back clean this time.** `cargo clean --release` removed 7,199 files / 4.4 GiB
before the rebuild, and both `unluminous.exe` and `unluminous-cli.exe` — built and installed — contain
zero occurrences of the old name, with no generated file under `target/release/build` carrying an old
path.

**The settings migration held.** The installed 0.37.0 came up with the stored font, background
opacity, pane sizes, both terminal tabs, all three plugins and `debug.lldb` pointing into the copied
adapter tree — not factory-fresh. The WebView2 `browser/` profile was deliberately not copied: it is
Chromium's own leveldb keyed to the old origin, it cannot be rewritten, and the pane rebuilds it.

**One surprise worth writing down.** Copying `%APPDATA%\Unluminate\plugins` into a destination that
already had a `plugins` directory — the test runs had created one under the new name hours earlier —
produced `Unluminous\plugins\plugins\…` rather than merging. `Copy-Item -Recurse <dir> <existing dir>`
nests instead of merging on Windows. Caught by the file count (13 before, 13 after) once flattened.
