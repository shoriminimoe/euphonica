# Genres view — implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a top-level **Genres** entry to the sidebar that browses the library by genre, splitting compound single-value tags so that atomic genres appear as separate tiles.

**Architecture:** New `GenreView` widget mirroring `ArtistView`'s shape — a two-page `adw::NavigationView` (genre grid → per-genre album grid). Cross-view album navigation reuses the existing `goto_album` flow already used by `ArtistContentView`. Genre tag splitting uses two new Aho-Corasick automatons in `utils.rs` (mirroring the existing artist ones), with a hybrid rule: trust MPD multi-value responses, split single compound strings.

**Tech Stack:** Rust 2024 edition, GTK4 + libadwaita via the `gtk-rs` bindings, `mpd` crate (htkhiem fork), `aho-corasick`, `rustc-hash`. Build via Meson driving Cargo.

**Spec reference:** `docs/superpowers/specs/2026-04-30-genres-view-design.md`.

---

## Pre-flight notes for the implementer

This codebase **has no automated test harness** — there is no `cargo test` target hooked into the build, no test files under `tests/`, no CI runs tests. The TDD discipline still applies for one specific area: `parse_genre_tag` and `parse_genre_values` are pure-logic functions and lend themselves to ordinary `#[cfg(test)] mod tests` blocks compiled by `cargo test`. Use real unit tests for that module. For everything else (UI widgets, GObject subclasses, MPD wiring, GSettings) the verification steps are: (a) the build itself catches a great deal because of GObject template macros and the borrow checker, and (b) a manual smoke-test pass at the end of the plan exercises end-to-end behaviour.

**Build commands you will use repeatedly:**

```bash
# Initial setup (once per fresh checkout / worktree):
meson setup build --buildtype=debug

# After any code change:
meson compile -C build

# After any change to data/io.github.htkhiem.Euphonica.gschema.xml or new .ui files:
meson install -C build --destdir "$PWD/build/install-root"
```

The `--destdir` form lets you verify schema/resource bundles compile without needing root. To actually run the binary against a fully-installed schema, use `meson install -C build` (root) or run via the Flatpak manifest from the README. The plan assumes the implementer has a working MPD instance to point Euphonica at for the final smoke-test.

**Cargo unit tests** for the splitter alone: `cargo test --manifest-path Cargo.toml common::genre::tests` will work without going through Meson once `cargo` can resolve `config.rs` (which Meson generates on first `compile`). If Cargo can't find `config.rs`, run `meson compile -C build` first to populate it, then re-run the tests.

**Commit cadence:** commit after each task. The codebase convention based on `git log` is short imperative commit messages without a Conventional-Commits prefix; just plain titles. Co-authored-by trailers are fine but not required.

---

## File map

**New files:**

- `src/common/genre.rs` — `Genre` GObject + `parse_genre_tag` + `parse_genre_values` + unit tests.
- `src/library/genre_cell.rs` — text-only tile widget for the grid.
- `src/library/genre_view.rs` — top-level grid widget.
- `src/library/genre_content_view.rs` — per-genre album grid widget.
- `src/gtk/library/genre-cell.ui` — template for `GenreCell`.
- `src/gtk/library/genre-view.ui` — template for `GenreView`.
- `src/gtk/library/genre-content-view.ui` — template for `GenreContentView`.
- `src/gtk/icons/genre-symbolic.svg` — sidebar icon.

**Modified files:**

- `data/io.github.htkhiem.Euphonica.gschema.xml` — new keys + `state.genreview` schema.
- `src/common/tags.rs` — add `GENRE` constant.
- `src/common/mod.rs` — export `Genre`, `parse_genre_tag`, `parse_genre_values`.
- `src/common/song.rs` — add `genres: Vec<String>` field to `SongInfo` + populate in `from_mpd`.
- `src/utils.rs` — add genre Aho-Corasick automatons + rebuild functions.
- `src/client/wrapper.rs` — add `get_genres` method.
- `src/library/controller.rs` — add `genres` ListStore, `init_genres`, `get_genre_albums`.
- `src/library/mod.rs` — declare and re-export new modules.
- `src/sidebar/sidebar.rs` — add `genres_btn` template child + handler.
- `src/gtk/sidebar.ui` — add the Genres `SidebarButton`.
- `src/window.rs` — register `GenreView` template child + setup + `album-clicked` signal handler.
- `src/window.ui` — add genre stack page.
- `src/preferences/library.rs` + `src/gtk/preferences/library.ui` — genre delimiter UI.
- `src/euphonica.gresource.xml` — register new `.ui` and `.svg` files.

---

## Task 1: GSettings additions

**Files:**
- Modify: `data/io.github.htkhiem.Euphonica.gschema.xml`

- [ ] **Step 1: Add the two new `library` keys**

In `data/io.github.htkhiem.Euphonica.gschema.xml`, find the `library` schema (the line that starts `<schema id="io.github.htkhiem.Euphonica.library"`). Right after the existing `artist-tag-delim-exceptions` key (around line 107), add:

```xml
		<key name="genre-tag-delims" type="as">
			<default>[",", ";", "/"]</default>
		</key>
		<key name="genre-tag-delim-exceptions" type="as">
			<default>[]</default>
		</key>
```

- [ ] **Step 2: Add the `state.genreview` child reference**

Find the `state` schema (around line 306). After the existing `artistview` child line (around line 308), add:

```xml
		<child schema="io.github.htkhiem.Euphonica.state.genreview" name="genreview"/>
```

- [ ] **Step 3: Add the `state.genreview` schema definition**

Right after the existing `state.artistview` schema (around line 353), add:

```xml
	<schema id="io.github.htkhiem.Euphonica.state.genreview" path="/io/github/htkhiem/Euphonica/state/genreview/">
		<!-- Genre view state -->
		<key name="sort-direction" enum='io.github.htkhiem.Euphonica.sortdir'>
			<default>'asc'</default>
			<summary>Genre View sort direction</summary>
		</key>
	</schema>
```

- [ ] **Step 4: Verify the schema compiles**

```bash
meson install -C build --destdir "$PWD/build/install-root"
```

Expected: install completes without `glib-compile-schemas` errors. Search for `genre` in the compiled bundle to confirm:

```bash
grep -l "genre-tag-delims" build/install-root/usr/local/share/glib-2.0/schemas/gschemas.compiled
```

Expected: matches the file (binary so grep emits the filename, not content). If `gschemas.compiled` is not present, run `glib-compile-schemas build/install-root/usr/local/share/glib-2.0/schemas/` manually and retry.

- [ ] **Step 5: Commit**

```bash
git add data/io.github.htkhiem.Euphonica.gschema.xml
git commit -m "Add gschema keys for genre tag splitting and genre view state"
```

---

## Task 2: GENRE tag constant

**Files:**
- Modify: `src/common/tags.rs:14`

- [ ] **Step 1: Append the GENRE constant**

Open `src/common/tags.rs`. After the existing `ALBUMARTIST_MBID` line, add:

```rust
pub const GENRE: &str = "genre";
```

- [ ] **Step 2: Verify it compiles**

```bash
meson compile -C build
```

Expected: no errors.

- [ ] **Step 3: Commit**

```bash
git add src/common/tags.rs
git commit -m "Add GENRE tag constant"
```

---

## Task 3: Genre splitter logic + automatons + unit tests

**Files:**
- Create: `src/common/genre.rs`
- Modify: `src/common/mod.rs`
- Modify: `src/utils.rs`

The genre splitter is the only piece in this plan that warrants real unit tests. Write the tests first, watch them fail, then implement.

- [ ] **Step 1: Add genre automatons + rebuild helpers to `src/utils.rs`**

Append these blocks just below the existing `ARTIST_DELIM_EXCEPTION_AUTOMATON` rebuild function (around line 458):

```rust
fn build_genre_delim_automaton() -> Option<AhoCorasick> {
    let setting = settings_manager()
        .child("library")
        .value("genre-tag-delims");
    let delims: Vec<&str> = setting.array_iter_str().unwrap().collect();
    build_aho_corasick_automaton(&delims)
}
fn build_genre_delim_exceptions_automaton() -> Option<AhoCorasick> {
    let setting = settings_manager()
        .child("library")
        .value("genre-tag-delim-exceptions");
    let excepts: Vec<&str> = setting.array_iter_str().unwrap().collect();
    build_aho_corasick_automaton(&excepts)
}

pub static GENRE_DELIM_AUTOMATON: Lazy<RwLock<Option<AhoCorasick>>> = Lazy::new(|| {
    let opt_automaton = build_genre_delim_automaton();
    RwLock::new(opt_automaton)
});

pub fn rebuild_genre_delim_automaton() {
    if let Ok(mut automaton) = GENRE_DELIM_AUTOMATON.write() {
        let new = build_genre_delim_automaton();
        *automaton = new;
    }
}

pub static GENRE_DELIM_EXCEPTION_AUTOMATON: Lazy<RwLock<Option<AhoCorasick>>> = Lazy::new(|| {
    let opt_automaton = build_genre_delim_exceptions_automaton();
    RwLock::new(opt_automaton)
});

pub fn rebuild_genre_delim_exception_automaton() {
    if let Ok(mut automaton) = GENRE_DELIM_EXCEPTION_AUTOMATON.write() {
        let new = build_genre_delim_exceptions_automaton();
        *automaton = new;
    }
}
```

- [ ] **Step 2: Create `src/common/genre.rs` with the splitter, the GObject, and failing tests**

```rust
use crate::utils::{GENRE_DELIM_AUTOMATON, GENRE_DELIM_EXCEPTION_AUTOMATON};
use aho_corasick::{AhoCorasick, Match};
use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use std::cell::OnceCell;

/// Split a single Genre tag value into atomic genre names.
///
/// Mirrors `parse_mb_artist_tag`'s two-pass Aho-Corasick algorithm but consults
/// the genre automatons. **Differs deliberately** from the artist version: the
/// outer guard only requires the delimiter automaton to be present. An empty
/// exceptions list (the genre default) must NOT disable splitting.
pub fn parse_genre_tag(input: &str) -> Vec<&str> {
    let delim_guard = GENRE_DELIM_AUTOMATON.read().unwrap();
    let Some(delim_ac) = delim_guard.as_ref() else {
        return vec![input];
    };
    let exc_guard = GENRE_DELIM_EXCEPTION_AUTOMATON.read().unwrap();
    let exc_ac: Option<&AhoCorasick> = exc_guard.as_ref();

    let mut buffer: String = input.to_owned();
    let mut found: Vec<&str> = Vec::new();

    if let Some(exc_ac) = exc_ac {
        for mat in exc_ac.find_iter(input) {
            let start = mat.start();
            let end = mat.end();
            if let Some(name) = input.get(start..end) {
                found.push(name);
                let len = end - start;
                buffer.replace_range(start..end, &" ".repeat(len));
            }
        }
    }

    let matched_delims = delim_ac.find_iter(&buffer).collect::<Vec<Match>>();
    if matched_delims.is_empty() {
        if !found.is_empty() {
            return found;
        }
        return vec![input];
    }

    let first_range = 0..matched_delims[0].start();
    if buffer
        .get(first_range.clone())
        .is_some_and(|substr| !substr.trim().is_empty())
        && let Some(g) = input.get(first_range).map(str::trim)
    {
        found.push(g);
    }
    for i in 1..(matched_delims.len()) {
        let between = matched_delims[i - 1].end()..matched_delims[i].start();
        if buffer
            .get(between.clone())
            .is_some_and(|substr| !substr.trim().is_empty())
            && let Some(g) = input.get(between).map(str::trim)
        {
            found.push(g);
        }
    }
    let last_range = matched_delims.last().unwrap().end().min(buffer.len())..;
    if !buffer[last_range.clone()].trim().is_empty() {
        found.push(input[last_range].trim());
    }
    found
}

/// Apply the hybrid splitting rule to a song's full set of Genre tag values.
///
/// - `values.len() >= 2`: trust MPD; each value is its own genre, never re-split.
/// - `values.len() == 1`: pass through `parse_genre_tag`.
/// - `values.is_empty()`: returns empty vec.
///
/// Empty / whitespace-only entries are dropped. Output is owned `String`s
/// because callers store the result long-term.
pub fn parse_genre_values(values: &[String]) -> Vec<String> {
    if values.is_empty() {
        return Vec::new();
    }
    if values.len() >= 2 {
        return values
            .iter()
            .filter(|v| !v.trim().is_empty())
            .map(|v| v.trim().to_owned())
            .collect();
    }
    let single = values[0].as_str();
    if single.trim().is_empty() {
        return Vec::new();
    }
    parse_genre_tag(single)
        .into_iter()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

mod imp {
    use super::*;
    use glib::{ParamSpec, ParamSpecString};
    use once_cell::sync::Lazy;

    #[derive(Default, Debug)]
    pub struct Genre {
        pub name: OnceCell<String>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Genre {
        const NAME: &'static str = "EuphonicaGenre";
        type Type = super::Genre;
    }

    impl ObjectImpl for Genre {
        fn properties() -> &'static [ParamSpec] {
            static PROPERTIES: Lazy<Vec<ParamSpec>> =
                Lazy::new(|| vec![ParamSpecString::builder("name").read_only().build()]);
            PROPERTIES.as_ref()
        }

        fn property(&self, _id: usize, pspec: &ParamSpec) -> glib::Value {
            match pspec.name() {
                "name" => self.obj().get_name().to_value(),
                _ => unimplemented!(),
            }
        }
    }
}

glib::wrapper! {
    pub struct Genre(ObjectSubclass<imp::Genre>);
}

impl Genre {
    pub fn new(name: &str) -> Self {
        let obj: Self = glib::Object::builder().build();
        let _ = obj.imp().name.set(name.to_owned());
        obj
    }

    pub fn get_name(&self) -> &str {
        self.imp().name.get().map(String::as_str).unwrap_or("")
    }
}

impl Default for Genre {
    fn default() -> Self {
        glib::Object::new()
    }
}

#[cfg(test)]
mod tests {
    //! These tests exercise the splitter against the default delimiters
    //! `[",", ";", "/"]` and an empty exceptions list. They do **not** rely on
    //! GSettings being initialised — instead they install fixed automatons
    //! into the static `RwLock`s, run the tested logic, and restore.

    use super::*;
    use crate::utils::{
        build_aho_corasick_automaton, GENRE_DELIM_AUTOMATON, GENRE_DELIM_EXCEPTION_AUTOMATON,
    };
    use std::sync::Mutex;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn with_automatons<F: FnOnce()>(delims: &[&str], excepts: &[&str], f: F) {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prev_delim = std::mem::replace(
            &mut *GENRE_DELIM_AUTOMATON.write().unwrap(),
            build_aho_corasick_automaton(delims),
        );
        let prev_excepts = std::mem::replace(
            &mut *GENRE_DELIM_EXCEPTION_AUTOMATON.write().unwrap(),
            build_aho_corasick_automaton(excepts),
        );

        f();

        *GENRE_DELIM_AUTOMATON.write().unwrap() = prev_delim;
        *GENRE_DELIM_EXCEPTION_AUTOMATON.write().unwrap() = prev_excepts;
    }

    #[test]
    fn single_simple_value_is_unchanged() {
        with_automatons(&[",", ";", "/"], &[], || {
            assert_eq!(
                parse_genre_values(&["Rock".to_owned()]),
                vec!["Rock".to_owned()]
            );
        });
    }

    #[test]
    fn single_compound_value_is_split_on_comma() {
        with_automatons(&[",", ";", "/"], &[], || {
            assert_eq!(
                parse_genre_values(&["Rock, Pop".to_owned()]),
                vec!["Rock".to_owned(), "Pop".to_owned()]
            );
        });
    }

    #[test]
    fn single_compound_value_is_split_on_semicolon() {
        with_automatons(&[",", ";", "/"], &[], || {
            assert_eq!(
                parse_genre_values(&["Rock; Pop; Jazz".to_owned()]),
                vec!["Rock".to_owned(), "Pop".to_owned(), "Jazz".to_owned()]
            );
        });
    }

    #[test]
    fn ampersand_is_not_a_default_delimiter() {
        with_automatons(&[",", ";", "/"], &[], || {
            assert_eq!(
                parse_genre_values(&["Drum & Bass".to_owned()]),
                vec!["Drum & Bass".to_owned()]
            );
        });
    }

    #[test]
    fn multi_value_response_is_trusted_not_resplit() {
        with_automatons(&[",", ";", "/"], &[], || {
            // Even though "Rock, Pop" contains a delimiter, MPD already gave us
            // a list — we must trust it.
            assert_eq!(
                parse_genre_values(&[
                    "Jazz".to_owned(),
                    "Rock, Pop".to_owned(),
                ]),
                vec!["Jazz".to_owned(), "Rock, Pop".to_owned()]
            );
        });
    }

    #[test]
    fn slash_exception_is_preserved() {
        with_automatons(&[",", ";", "/"], &["AC/DC"], || {
            assert_eq!(
                parse_genre_values(&["AC/DC".to_owned()]),
                vec!["AC/DC".to_owned()]
            );
        });
    }

    #[test]
    fn empty_exceptions_does_not_disable_splitting() {
        // This is the key regression test: with the artist version's outer
        // guard, an empty exceptions list returns the input unchanged. Genre
        // splitter must keep working.
        with_automatons(&[",", ";", "/"], &[], || {
            assert_eq!(
                parse_genre_values(&["Rock, Pop".to_owned()]),
                vec!["Rock".to_owned(), "Pop".to_owned()]
            );
        });
    }

    #[test]
    fn empty_input_returns_empty() {
        with_automatons(&[",", ";", "/"], &[], || {
            assert!(parse_genre_values(&[]).is_empty());
            assert!(parse_genre_values(&["".to_owned()]).is_empty());
            assert!(parse_genre_values(&["   ".to_owned()]).is_empty());
        });
    }

    #[test]
    fn whitespace_only_entries_are_dropped_in_multi() {
        with_automatons(&[",", ";", "/"], &[], || {
            assert_eq!(
                parse_genre_values(&["Rock".to_owned(), "  ".to_owned(), "Pop".to_owned()]),
                vec!["Rock".to_owned(), "Pop".to_owned()]
            );
        });
    }
}
```

- [ ] **Step 3: Wire `genre` into `src/common/mod.rs`**

Add `pub mod genre;` to the `pub mod` block (after `pub mod dynamic_playlist;`). Add this re-export alongside the existing `pub use artist::...` line:

```rust
pub use genre::{Genre, parse_genre_tag, parse_genre_values};
```

- [ ] **Step 4: Run the tests; expect them to pass on first run**

```bash
meson compile -C build
cargo test --manifest-path Cargo.toml --target-dir build/src common::genre::tests
```

Expected: 9 tests pass. If `cargo test` complains it cannot find `config.rs`, run `meson compile -C build` once more to populate it, then retry. If a test fails, the splitter logic is wrong — fix it before continuing. Do not skip a failing test.

- [ ] **Step 5: Commit**

```bash
git add src/common/genre.rs src/common/mod.rs src/utils.rs
git commit -m "Add Genre GObject and genre tag splitter with unit tests"
```

---

## Task 4: SongInfo.genres population

**Files:**
- Modify: `src/common/song.rs`

- [ ] **Step 1: Add the `genres` field to `SongInfo`**

In `src/common/song.rs`, find the `pub struct SongInfo` block (around line 81). After the `pub last_played: Option<OffsetDateTime>` field (the last existing field), add:

```rust
    pub genres: Vec<String>,
```

The `#[derivative(Default)]` derive will give it `Vec::new()` by default — no manual default needed.

- [ ] **Step 2: Populate `genres` in the tag-iteration loop in `from_mpd`**

In the same file, find the `for (tag, val) in song.tags.into_iter()` loop (around line 508). Just before the loop, declare a buffer:

```rust
        let mut raw_genres: Vec<String> = Vec::new();
```

Inside the match block, immediately before the catch-all `_ => {}`, add an arm:

```rust
                tags::GENRE => {
                    raw_genres.push(val);
                }
```

After the loop ends (after the closing `}` for `for (tag, val) in song.tags...`), and before the `// Assume the artist IDs...` comment (~line 597), add:

```rust
        res.genres = crate::common::parse_genre_values(&raw_genres);
```

- [ ] **Step 3: Verify it compiles**

```bash
meson compile -C build
```

Expected: no errors. (No runtime test exists for this — verification is the next time you run the app, song-level genres will be available.)

- [ ] **Step 4: Commit**

```bash
git add src/common/song.rs
git commit -m "Populate SongInfo.genres from MPD Genre tags using hybrid splitter"
```

---

## Task 5: Client wrapper `get_genres`

**Files:**
- Modify: `src/client/wrapper.rs`

- [ ] **Step 1: Add the method**

In `src/client/wrapper.rs`, find the existing `get_albums_by_query` method (around line 791). Just above it, add:

```rust
    pub async fn get_genres<F>(&self, respond: &mut F) -> ClientResult<()>
    where
        F: FnMut(crate::common::Genre),
    {
        use crate::common::{Genre, parse_genre_tag};
        // FxHashSet is already imported at the top of this file.
        let (s, r) = oneshot::channel();
        let grouped_vals = self
            .background(
                Task::List(Term::Tag(Cow::Borrowed("genre")), Query::new(), None, s),
                r,
            )
            .await?;
        let mut seen: FxHashSet<String> = FxHashSet::default();
        for (_key, values) in grouped_vals.groups.into_iter() {
            for value in values.into_iter() {
                if value.trim().is_empty() {
                    continue;
                }
                for atomic in parse_genre_tag(&value) {
                    let trimmed = atomic.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    if seen.insert(trimmed.to_owned()) {
                        respond(Genre::new(trimmed));
                    }
                }
            }
        }
        Ok(())
    }
```

The reason this uses the **background** client and not the foreground: it's a one-time bulk fetch, exactly like `get_albums_by_query`'s discovery phase. It does not need to respond to user clicks within tens of milliseconds.

- [ ] **Step 2: Verify it compiles**

```bash
meson compile -C build
```

Expected: no errors.

- [ ] **Step 3: Commit**

```bash
git add src/client/wrapper.rs
git commit -m "Add MpdWrapper::get_genres returning split, deduplicated Genre objects"
```

---

## Task 6: Library controller — genres state and `init_genres`

**Files:**
- Modify: `src/library/controller.rs`

- [ ] **Step 1: Add the `Genre` import**

At the top of `src/library/controller.rs`, find the `use crate::{... common::{Album, Artist, ...}, ...}` import block. Add `Genre` to the `common::{...}` list:

```rust
    common::{Album, Artist, DynamicPlaylist, Genre, INode, Song, SongInfo, Stickers, tags},
```

- [ ] **Step 2: Add the new fields to `imp::Library`**

Inside the `pub struct Library { ... }` definition (around line 29), after the `pub recent_artists: gio::ListStore` field, add:

```rust
        #[derivative(Default(value = "gio::ListStore::new::<Genre>()"))]
        pub genres: gio::ListStore,
        pub genres_initialized: Cell<bool>,
```

- [ ] **Step 3: Reset the new fields in `clear()`**

Find the `pub fn clear(&self)` method (around line 128). Inside, after the line `self.imp().recent_artists.remove_all();`, add:

```rust
        self.imp().genres.remove_all();
        self.imp().genres_initialized.set(false);
```

- [ ] **Step 4: Add the public getter and `init_genres` method**

Find the existing `pub fn artists(&self) -> gio::ListStore` (around line 442). Just after `pub fn recent_artists`, add:

```rust
    /// Get a reference to the local genres store.
    pub fn genres(&self) -> gio::ListStore {
        self.imp().genres.clone()
    }
```

Then find `pub async fn init_artists` (around line 637). Just above it, add:

```rust
    pub async fn init_genres(&self) -> ClientResult<()> {
        if !self.imp().genres_initialized.get() {
            self.imp().genres_initialized.set(true);
            let model = self.imp().genres.clone();
            model.remove_all();
            self.client()
                .get_genres(&mut |genre| {
                    model.append(&genre);
                })
                .await?;
        }
        Ok(())
    }
```

- [ ] **Step 5: Verify it compiles**

```bash
meson compile -C build
```

Expected: no errors.

- [ ] **Step 6: Commit**

```bash
git add src/library/controller.rs
git commit -m "Add genres ListStore and init_genres to Library controller"
```

---

## Task 7: Library controller — `get_genre_albums`

**Files:**
- Modify: `src/library/controller.rs`

- [ ] **Step 1: Add the method**

In the same `impl Library` block, find `pub async fn get_artist_content` (around line 659). Right after that method ends (after the closing `}` of `get_artist_content`, but still inside the `impl Library` block), add:

```rust
    /// Find all albums whose songs include the given genre after splitting.
    /// Mirrors `get_artist_content`'s shape: server-side filter narrows the
    /// candidate set, client-side verification drops substring false positives
    /// (e.g. "Rock" matching "Rock & Roll"), and unique albums are emitted.
    pub async fn get_albums_by_genre<FA>(
        &self,
        genre: String,
        mut respond_album: FA,
    ) -> ClientResult<()>
    where
        FA: FnMut(Album),
    {
        let mut song_query = Query::new();
        song_query.and_with_op(
            Term::Tag(tags::GENRE.into()),
            QueryOperation::Contains,
            genre.clone(),
        );

        let mut visited_albums = FxHashSet::default();
        self.client()
            .get_song_infos_by_query(song_query, true, &mut |batch| {
                for song in batch.into_iter() {
                    if !song.genres.iter().any(|g| g == &genre) {
                        continue;
                    }
                    if let Some(album) = song.album.as_ref() {
                        if visited_albums.insert(album.get_comp_id().to_owned()) {
                            respond_album(album.clone().into());
                        }
                    }
                }
            })
            .await
    }
```

- [ ] **Step 2: Verify it compiles**

```bash
meson compile -C build
```

Expected: no errors.

- [ ] **Step 3: Commit**

```bash
git add src/library/controller.rs
git commit -m "Add Library::get_albums_by_genre with substring-filter + verification"
```

---

## Task 8: GenreCell widget + UI template

**Files:**
- Create: `src/library/genre_cell.rs`
- Create: `src/gtk/library/genre-cell.ui`
- Modify: `src/euphonica.gresource.xml`

- [ ] **Step 1: Create the UI template**

Write `src/gtk/library/genre-cell.ui`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<interface>
	<template class="EuphonicaGenreCell" parent="GtkBox">
		<style>
			<class name="card"/>
			<class name="padding-12"/>
		</style>
		<property name="halign">3</property>
		<property name="valign">1</property>
		<property name="margin-top">6</property>
		<property name="margin-bottom">6</property>
		<property name="margin-start">6</property>
		<property name="margin-end">6</property>
		<property name="orientation">0</property>
		<property name="hexpand">true</property>
		<child>
			<object class="GtkLabel" id="name">
				<property name="ellipsize">3</property>
				<property name="justify">center</property>
				<property name="hexpand">true</property>
				<property name="halign">3</property>
				<style>
					<class name="title-4"/>
				</style>
			</object>
		</child>
	</template>
</interface>
```

- [ ] **Step 2: Create the widget**

Write `src/library/genre_cell.rs`:

```rust
use gtk::{glib, CompositeTemplate, prelude::*, subclass::prelude::*};

use crate::common::Genre;

mod imp {
    use super::*;

    #[derive(Default, CompositeTemplate)]
    #[template(resource = "/io/github/htkhiem/Euphonica/gtk/library/genre-cell.ui")]
    pub struct GenreCell {
        #[template_child]
        pub name: TemplateChild<gtk::Label>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for GenreCell {
        const NAME: &'static str = "EuphonicaGenreCell";
        type Type = super::GenreCell;
        type ParentType = gtk::Box;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for GenreCell {}
    impl WidgetImpl for GenreCell {}
    impl BoxImpl for GenreCell {}
}

glib::wrapper! {
    pub struct GenreCell(ObjectSubclass<imp::GenreCell>)
        @extends gtk::Box, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

impl Default for GenreCell {
    fn default() -> Self {
        glib::Object::new()
    }
}

impl GenreCell {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn bind(&self, genre: &Genre) {
        self.imp().name.set_label(genre.get_name());
    }

    pub fn unbind(&self) {
        self.imp().name.set_label("");
    }
}
```

- [ ] **Step 3: Register the new UI in `src/euphonica.gresource.xml`**

Inside the existing `<gresource prefix="/io/github/htkhiem/Euphonica">` block, alongside the other `gtk/library/*.ui` lines (around line 27), add:

```xml
    <file preprocess="xml-stripblanks">gtk/library/genre-cell.ui</file>
```

- [ ] **Step 4: Verify it compiles**

```bash
meson compile -C build
```

Expected: no errors. The compile failure mode for missing gresource entries is "could not load resource" at runtime, not at build, so a clean build is the bar here.

- [ ] **Step 5: Commit**

```bash
git add src/library/genre_cell.rs src/gtk/library/genre-cell.ui src/euphonica.gresource.xml
git commit -m "Add GenreCell text-only tile widget"
```

---

## Task 9: GenreContentView + UI template

**Files:**
- Create: `src/library/genre_content_view.rs`
- Create: `src/gtk/library/genre-content-view.ui`
- Modify: `src/euphonica.gresource.xml`

This widget hosts the album grid for one genre. It does **not** nest its own AlbumContentView — clicking an album emits a signal the window handles via `goto_album`.

- [ ] **Step 1: Create the UI template**

Write `src/gtk/library/genre-content-view.ui`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<interface>
	<requires lib="gtk" version="4.0"/>
	<template class="EuphonicaGenreContentView" parent="GtkWidget">
		<child>
			<object class="AdwToolbarView">
				<child type="top">
					<object class="AdwHeaderBar">
						<property name="show-title">false</property>
						<child type="start">
							<object class="GtkBox">
								<property name="spacing">6</property>
								<child>
									<object class="GtkLabel" id="genre_name">
										<property name="ellipsize">3</property>
										<style>
											<class name="title-2"/>
										</style>
									</object>
								</child>
								<child>
									<object class="GtkLabel" id="album_count">
										<style>
											<class name="dim-label"/>
											<class name="caption"/>
										</style>
									</object>
								</child>
							</object>
						</child>
					</object>
				</child>
				<property name="content">
					<object class="EuphonicaContentStack" id="stack">
						<property name="placeholder">
							<object class="AdwStatusPage">
								<property name="title" translatable="true">No Albums</property>
							</object>
						</property>
						<property name="content">
							<object class="GtkScrolledWindow">
								<property name="hscrollbar-policy">never</property>
								<property name="vscrollbar-policy">automatic</property>
								<property name="propagate-natural-height">true</property>
								<property name="has-frame">false</property>
								<property name="vexpand">true</property>
								<property name="child">
									<object class="GtkGridView" id="album_grid">
										<property name="orientation">1</property>
										<property name="min-columns">1</property>
										<property name="max-columns">10</property>
										<property name="single-click-activate">true</property>
										<style>
											<class name="no-bg"/>
											<class name="padding-12"/>
										</style>
									</object>
								</property>
							</object>
						</property>
					</object>
				</property>
			</object>
		</child>
	</template>
</interface>
```

- [ ] **Step 2: Create the widget**

Write `src/library/genre_content_view.rs`:

```rust
use adw::subclass::prelude::*;
use derivative::Derivative;
use glib::{clone, subclass::Signal, WeakRef};
use gtk::{
    gio, glib, prelude::*, CompositeTemplate, ListItem, SignalListItemFactory, SingleSelection,
};
use std::{cell::RefCell, rc::Rc, sync::OnceLock};

use super::{AlbumCell, Library};
use crate::{
    cache::Cache,
    common::{Album, ContentStack, Genre},
};

mod imp {
    use super::*;

    #[derive(Debug, CompositeTemplate, Derivative)]
    #[derivative(Default)]
    #[template(resource = "/io/github/htkhiem/Euphonica/gtk/library/genre-content-view.ui")]
    pub struct GenreContentView {
        #[template_child]
        pub stack: TemplateChild<ContentStack>,
        #[template_child]
        pub genre_name: TemplateChild<gtk::Label>,
        #[template_child]
        pub album_count: TemplateChild<gtk::Label>,
        #[template_child]
        pub album_grid: TemplateChild<gtk::GridView>,

        #[derivative(Default(value = "gio::ListStore::new::<Album>()"))]
        pub album_list: gio::ListStore,
        pub library: WeakRef<Library>,
        pub cache: RefCell<Option<Rc<Cache>>>,
        pub current_genre: RefCell<Option<Genre>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for GenreContentView {
        const NAME: &'static str = "EuphonicaGenreContentView";
        type Type = super::GenreContentView;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
            klass.set_layout_manager_type::<gtk::BinLayout>();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for GenreContentView {
        fn dispose(&self) {
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }

        fn constructed(&self) {
            self.parent_constructed();
            self.stack.show_placeholder();
            self.album_list
                .bind_property("n-items", &self.album_count.get(), "label")
                .transform_to(|_, n: u32| {
                    Some(if n == 1 {
                        "1 album".to_string()
                    } else {
                        format!("{n} albums")
                    })
                })
                .sync_create()
                .build();
        }

        fn signals() -> &'static [Signal] {
            static SIGNALS: OnceLock<Vec<Signal>> = OnceLock::new();
            SIGNALS.get_or_init(|| {
                vec![Signal::builder("album-clicked")
                    .param_types([Album::static_type()])
                    .build()]
            })
        }
    }

    impl WidgetImpl for GenreContentView {}
}

glib::wrapper! {
    pub struct GenreContentView(ObjectSubclass<imp::GenreContentView>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Default for GenreContentView {
    fn default() -> Self {
        glib::Object::new()
    }
}

impl GenreContentView {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn setup(&self, library: &Library, cache: Rc<Cache>) {
        self.imp().library.set(Some(library));
        self.imp().cache.replace(Some(cache.clone()));

        let sel_model = SingleSelection::new(Some(self.imp().album_list.clone()));
        self.imp().album_grid.set_model(Some(&sel_model));

        let factory = SignalListItemFactory::new();
        factory.connect_setup(clone!(
            #[weak]
            cache,
            move |_, list_item| {
                let item = list_item
                    .downcast_ref::<ListItem>()
                    .expect("Needs to be ListItem");
                let cell = AlbumCell::new(item, cache, None);
                item.set_child(Some(&cell));
            }
        ));
        factory.connect_bind(move |_, list_item| {
            let item = list_item
                .downcast_ref::<ListItem>()
                .expect("Needs to be ListItem");
            let album = item
                .item()
                .and_downcast::<Album>()
                .expect("The item has to be a common::Album.");
            let cell = item
                .child()
                .and_downcast::<AlbumCell>()
                .expect("The child has to be an AlbumCell.");
            cell.bind(&album);
        });
        factory.connect_unbind(move |_, list_item| {
            let item = list_item
                .downcast_ref::<ListItem>()
                .expect("Needs to be ListItem");
            let cell = item
                .child()
                .and_downcast::<AlbumCell>()
                .expect("The child has to be an AlbumCell.");
            cell.unbind();
        });
        self.imp().album_grid.set_factory(Some(&factory));

        self.imp().album_grid.connect_activate(clone!(
            #[weak(rename_to = this)]
            self,
            move |grid, position| {
                let model = grid.model().expect("The model has to exist.");
                let album = model
                    .item(position)
                    .and_downcast::<Album>()
                    .expect("The item has to be a common::Album.");
                this.emit_by_name::<()>("album-clicked", &[&album.to_value()]);
            }
        ));
    }

    pub fn bind(&self, genre: &Genre) {
        self.imp().genre_name.set_label(genre.get_name());
        self.imp().current_genre.replace(Some(genre.clone()));
        self.populate();
    }

    pub fn unbind(&self) {
        self.imp().album_list.remove_all();
        self.imp().current_genre.replace(None);
        self.imp().stack.show_placeholder();
    }

    fn populate(&self) {
        self.imp().album_list.remove_all();
        let Some(library) = self.imp().library.upgrade() else {
            return;
        };
        let Some(genre) = self.imp().current_genre.borrow().clone() else {
            return;
        };
        let model = self.imp().album_list.clone();
        let stack = self.imp().stack.get();
        stack.show_spinner();
        let name = genre.get_name().to_owned();
        glib::spawn_future_local(async move {
            let _ = library
                .get_albums_by_genre(name, |album| {
                    model.append(&album);
                })
                .await;
            if model.n_items() > 0 {
                stack.show_content();
            } else {
                stack.show_placeholder();
            }
        });
    }
}
```

- [ ] **Step 3: Register the new UI**

In `src/euphonica.gresource.xml`, add (alongside the other library `.ui` entries):

```xml
    <file preprocess="xml-stripblanks">gtk/library/genre-content-view.ui</file>
```

- [ ] **Step 4: Verify it compiles**

```bash
meson compile -C build
```

Expected: no errors.

- [ ] **Step 5: Commit**

```bash
git add src/library/genre_content_view.rs src/gtk/library/genre-content-view.ui src/euphonica.gresource.xml
git commit -m "Add GenreContentView per-genre album grid widget"
```

---

## Task 10: GenreView (top-level grid widget)

**Files:**
- Create: `src/library/genre_view.rs`
- Create: `src/gtk/library/genre-view.ui`
- Modify: `src/euphonica.gresource.xml`
- Modify: `src/library/mod.rs`

- [ ] **Step 1: Create the UI template**

Write `src/gtk/library/genre-view.ui`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<interface>
	<requires lib="gtk" version="4.0"/>
	<template class="EuphonicaGenreView" parent="GtkWidget">
		<child>
			<object class="AdwNavigationView" id="nav_view">
				<child>
					<object class="AdwNavigationPage">
						<property name="title">Genres</property>
						<child>
							<object class="AdwToolbarView">
								<child type="top">
									<object class="AdwHeaderBar">
										<property name="show-title">false</property>
										<child type="start">
											<object class="GtkButton" id="show_sidebar">
												<property name="icon-name">dock-left-symbolic</property>
												<property name="tooltip-text" translatable="true">Show sidebar</property>
												<property name="visible">false</property>
											</object>
										</child>
										<child type="end">
											<object class="GtkToggleButton" id="search_btn">
												<property name="icon-name">edit-find-symbolic</property>
											</object>
										</child>
										<child type="end">
											<object class="GtkButton" id="sort_dir_btn">
												<style>
													<class name="flat"/>
												</style>
												<child>
													<object class="GtkBox">
														<property name="spacing">6</property>
														<child>
															<object class="GtkImage" id="sort_dir">
																<property name="icon-name">view-sort-ascending-symbolic</property>
															</object>
														</child>
														<child>
															<object class="GtkLabel">
																<property name="label" translatable="true">Name</property>
															</object>
														</child>
													</object>
												</child>
											</object>
										</child>
									</object>
								</child>
								<child type="top">
									<object class="GtkSearchBar" id="search_bar">
										<property name="key-capture-widget">nav_view</property>
										<child>
											<object class="GtkSearchEntry" id="search_entry">
												<property name="search-delay">150</property>
												<property name="width-request">100</property>
											</object>
										</child>
									</object>
								</child>
								<property name="content">
									<object class="EuphonicaContentStack" id="stack">
										<property name="placeholder">
											<object class="AdwStatusPage">
												<property name="title" translatable="true">No Genres</property>
											</object>
										</property>
										<property name="content">
											<object class="GtkScrolledWindow">
												<property name="hscrollbar-policy">never</property>
												<property name="vscrollbar-policy">automatic</property>
												<property name="propagate-natural-height">true</property>
												<property name="has-frame">false</property>
												<property name="vexpand">true</property>
												<property name="child">
													<object class="GtkGridView" id="grid_view">
														<property name="orientation">1</property>
														<property name="min-columns">1</property>
														<property name="max-columns">6</property>
														<property name="single-click-activate">true</property>
														<style>
															<class name="no-bg"/>
															<class name="padding-12"/>
														</style>
													</object>
												</property>
											</object>
										</property>
									</object>
								</property>
							</object>
						</child>
					</object>
				</child>
				<child>
					<object class="AdwNavigationPage" id="content_page">
						<property name="tag">content</property>
						<property name="title">Albums</property>
						<child>
							<object class="EuphonicaGenreContentView" id="content_view"></object>
						</child>
					</object>
				</child>
			</object>
		</child>
	</template>
</interface>
```

- [ ] **Step 2: Create the widget**

Write `src/library/genre_view.rs`:

```rust
use adw::prelude::*;
use adw::subclass::prelude::*;
use glib::{clone, subclass::Signal, Properties, WeakRef};
use gtk::{
    glib, CompositeTemplate, ListItem, SignalListItemFactory, SingleSelection,
};
use std::{cell::Cell, cmp::Ordering, rc::Rc, sync::OnceLock};

use super::{GenreCell, GenreContentView, Library};
use crate::{
    cache::Cache,
    common::{ContentStack, Genre},
    utils::{g_cmp_str_options, g_search_substr, settings_manager, LazyInit},
};

mod imp {
    use super::*;

    #[derive(Default, Debug, CompositeTemplate, Properties)]
    #[properties(wrapper_type = super::GenreView)]
    #[template(resource = "/io/github/htkhiem/Euphonica/gtk/library/genre-view.ui")]
    pub struct GenreView {
        #[template_child]
        pub nav_view: TemplateChild<adw::NavigationView>,
        #[template_child]
        pub show_sidebar: TemplateChild<gtk::Button>,

        #[template_child]
        pub sort_dir: TemplateChild<gtk::Image>,
        #[template_child]
        pub sort_dir_btn: TemplateChild<gtk::Button>,
        #[template_child]
        pub search_btn: TemplateChild<gtk::ToggleButton>,
        #[template_child]
        pub search_bar: TemplateChild<gtk::SearchBar>,
        #[template_child]
        pub search_entry: TemplateChild<gtk::SearchEntry>,

        #[template_child]
        pub stack: TemplateChild<ContentStack>,
        #[template_child]
        pub grid_view: TemplateChild<gtk::GridView>,
        #[template_child]
        pub content_page: TemplateChild<adw::NavigationPage>,
        #[template_child]
        pub content_view: TemplateChild<GenreContentView>,

        pub search_filter: gtk::CustomFilter,
        pub sorter: gtk::CustomSorter,
        pub last_search_len: Cell<usize>,
        #[property(get, set)]
        pub collapsed: Cell<bool>,
        pub library: WeakRef<Library>,
        pub initializing: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for GenreView {
        const NAME: &'static str = "EuphonicaGenreView";
        type Type = super::GenreView;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
            klass.set_layout_manager_type::<gtk::BinLayout>();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for GenreView {
        fn dispose(&self) {
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }

        fn constructed(&self) {
            self.parent_constructed();
            self.stack.show_placeholder();

            self.obj()
                .bind_property("collapsed", &self.show_sidebar.get(), "visible")
                .sync_create()
                .build();
            self.show_sidebar.connect_clicked(clone!(
                #[weak(rename_to = this)]
                self,
                move |_| {
                    this.obj().emit_by_name::<()>("show-sidebar-clicked", &[]);
                }
            ));
        }

        fn signals() -> &'static [Signal] {
            static SIGNALS: OnceLock<Vec<Signal>> = OnceLock::new();
            SIGNALS.get_or_init(|| vec![Signal::builder("show-sidebar-clicked").build()])
        }
    }

    impl WidgetImpl for GenreView {}
}

glib::wrapper! {
    pub struct GenreView(ObjectSubclass<imp::GenreView>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Default for GenreView {
    fn default() -> Self {
        Self::new()
    }
}

impl GenreView {
    pub fn new() -> Self {
        glib::Object::new()
    }

    pub fn get_content_view(&self) -> GenreContentView {
        self.imp().content_view.get()
    }

    pub fn setup(&self, library: &Library, cache: Rc<Cache>) {
        self.imp().library.set(Some(library));
        self.setup_sort();
        self.setup_search();
        self.setup_gridview(cache.clone());
        self.imp().content_view.get().setup(library, cache);
    }

    fn setup_sort(&self) {
        let settings = settings_manager();
        let state = settings.child("state").child("genreview");
        let library_settings = settings.child("library");
        let sort_dir_btn = self.imp().sort_dir_btn.get();
        sort_dir_btn.connect_clicked(clone!(
            #[weak]
            state,
            move |_| {
                if state.string("sort-direction") == "asc" {
                    let _ = state.set_string("sort-direction", "desc");
                } else {
                    let _ = state.set_string("sort-direction", "asc");
                }
            }
        ));
        let sort_dir = self.imp().sort_dir.get();
        state
            .bind("sort-direction", &sort_dir, "icon-name")
            .get_only()
            .mapping(|dir, _| match dir.get::<String>().unwrap().as_ref() {
                "asc" => Some("view-sort-ascending-symbolic".to_value()),
                _ => Some("view-sort-descending-symbolic".to_value()),
            })
            .build();

        self.imp().sorter.set_sort_func(clone!(
            #[strong]
            library_settings,
            #[strong]
            state,
            move |a, b| {
                let g1 = a.downcast_ref::<Genre>().expect("Sort obj must be Genre.");
                let g2 = b.downcast_ref::<Genre>().expect("Sort obj must be Genre.");
                let asc = state.enum_("sort-direction") > 0;
                let case_sensitive = library_settings.boolean("sort-case-sensitive");
                let nulls_first = library_settings.boolean("sort-nulls-first");
                g_cmp_str_options(
                    Some(g1.get_name()),
                    Some(g2.get_name()),
                    nulls_first,
                    asc,
                    case_sensitive,
                )
            }
        ));

        state.connect_changed(
            Some("sort-direction"),
            clone!(
                #[weak(rename_to = this)]
                self,
                move |_, _| {
                    this.imp().sorter.changed(gtk::SorterChange::Inverted);
                }
            ),
        );
    }

    fn setup_search(&self) {
        let settings = settings_manager();
        let library_settings = settings.child("library");
        self.imp().search_filter.set_filter_func(clone!(
            #[weak(rename_to = this)]
            self,
            #[strong]
            library_settings,
            #[upgrade_or]
            true,
            move |obj| {
                let genre = obj
                    .downcast_ref::<Genre>()
                    .expect("Search obj must be Genre.");
                let term = this.imp().search_entry.text();
                if term.is_empty() {
                    return true;
                }
                let case_sensitive = library_settings.boolean("search-case-sensitive");
                g_search_substr(Some(genre.get_name()), &term, case_sensitive)
            }
        ));
        let search_entry = self.imp().search_entry.get();
        search_entry.connect_search_changed(clone!(
            #[weak(rename_to = this)]
            self,
            move |entry| {
                let new_len = entry.text().len();
                let old_len = this.imp().last_search_len.replace(new_len);
                match new_len.cmp(&old_len) {
                    Ordering::Greater => this
                        .imp()
                        .search_filter
                        .changed(gtk::FilterChange::MoreStrict),
                    Ordering::Less => this
                        .imp()
                        .search_filter
                        .changed(gtk::FilterChange::LessStrict),
                    Ordering::Equal => this
                        .imp()
                        .search_filter
                        .changed(gtk::FilterChange::Different),
                }
            }
        ));
    }

    fn setup_gridview(&self, _cache: Rc<Cache>) {
        let settings = settings_manager().child("ui");
        let library = self.imp().library.upgrade().unwrap();
        let genres = library.genres();

        let search_bar = self.imp().search_bar.get();
        let search_entry = self.imp().search_entry.get();
        search_bar.connect_entry(&search_entry);
        let search_btn = self.imp().search_btn.get();
        search_btn
            .bind_property("active", &search_bar, "search-mode-enabled")
            .sync_create()
            .build();

        let search_model = gtk::FilterListModel::new(
            Some(genres.clone()),
            Some(self.imp().search_filter.clone()),
        );
        search_model.set_incremental(true);
        let sort_model =
            gtk::SortListModel::new(Some(search_model), Some(self.imp().sorter.clone()));
        sort_model.set_incremental(true);
        let sel_model = SingleSelection::new(Some(sort_model));

        let grid_view = self.imp().grid_view.get();
        grid_view.set_model(Some(&sel_model));
        settings
            .bind("max-columns", &grid_view, "max-columns")
            .build();

        let factory = SignalListItemFactory::new();
        factory.connect_setup(move |_, list_item| {
            let item = list_item
                .downcast_ref::<ListItem>()
                .expect("Needs to be ListItem");
            let cell = GenreCell::new();
            item.set_child(Some(&cell));
        });
        factory.connect_bind(move |_, list_item| {
            let item = list_item
                .downcast_ref::<ListItem>()
                .expect("Needs to be ListItem");
            let genre = item
                .item()
                .and_downcast::<Genre>()
                .expect("The item has to be a common::Genre.");
            let cell = item
                .child()
                .and_downcast::<GenreCell>()
                .expect("The child has to be a GenreCell.");
            cell.bind(&genre);
        });
        factory.connect_unbind(move |_, list_item| {
            let item = list_item
                .downcast_ref::<ListItem>()
                .expect("Needs to be ListItem");
            let cell = item
                .child()
                .and_downcast::<GenreCell>()
                .expect("The child has to be a GenreCell.");
            cell.unbind();
        });
        grid_view.set_factory(Some(&factory));

        grid_view.connect_activate(clone!(
            #[weak(rename_to = this)]
            self,
            move |grid, position| {
                let model = grid.model().expect("The model has to exist.");
                let genre = model
                    .item(position)
                    .and_downcast::<Genre>()
                    .expect("The item has to be a common::Genre.");
                this.on_genre_clicked(&genre);
            }
        ));
    }

    pub fn on_genre_clicked(&self, genre: &Genre) {
        let content_view = self.imp().content_view.get();
        content_view.unbind();
        content_view.bind(genre);
        if self
            .imp()
            .nav_view
            .visible_page_tag()
            .is_none_or(|tag| tag.as_str() != "content")
        {
            self.imp().nav_view.push_by_tag("content");
        }
    }
}

impl LazyInit for GenreView {
    fn populate(&self) {
        if let Some(library) = self.imp().library.upgrade() {
            if !self.imp().initializing.get() {
                self.imp().initializing.set(true);
                let stack = self.imp().stack.get();
                let this = self.clone();
                stack.show_spinner();
                glib::spawn_future_local(async move {
                    let _ = library.init_genres().await;
                    if library.genres().n_items() > 0 {
                        stack.show_content();
                    } else {
                        stack.show_placeholder();
                    }
                    this.imp().initializing.set(false);
                });
            }
        }
    }
}
```

- [ ] **Step 3: Register the new UI in gresource**

In `src/euphonica.gresource.xml`, add:

```xml
    <file preprocess="xml-stripblanks">gtk/library/genre-view.ui</file>
```

- [ ] **Step 4: Update `src/library/mod.rs`**

Add to the `mod` declarations (after `mod artist_view;`):

```rust
mod genre_cell;
mod genre_content_view;
mod genre_view;
```

Add to the re-exports (after `pub use artist_view::ArtistView;`):

```rust
use genre_cell::GenreCell;
pub use genre_content_view::GenreContentView;
pub use genre_view::GenreView;
```

- [ ] **Step 5: Verify it compiles**

```bash
meson compile -C build
```

Expected: no errors.

- [ ] **Step 6: Commit**

```bash
git add src/library/genre_view.rs src/gtk/library/genre-view.ui src/library/mod.rs src/euphonica.gresource.xml
git commit -m "Add GenreView top-level grid widget with search and sort"
```

---

## Task 11: Sidebar entry — icon, button, handler

**Files:**
- Create: `src/gtk/icons/genre-symbolic.svg`
- Modify: `src/euphonica.gresource.xml`
- Modify: `src/gtk/sidebar.ui`
- Modify: `src/sidebar/sidebar.rs`

- [ ] **Step 1: Add the symbolic icon**

Write `src/gtk/icons/genre-symbolic.svg`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16">
  <path d="M 4 2 L 4 11.5 A 2.5 2.5 0 1 0 5 13.5 L 5 5 L 12 5 L 12 9.5 A 2.5 2.5 0 1 0 13 11.5 L 13 2 Z" fill="#2e3436"/>
</svg>
```

This is a generic music-note glyph in the GNOME symbolic style. Symbolic icons get recoloured by the theme — the explicit `#2e3436` fill is what GNOME uses as the recolour anchor.

- [ ] **Step 2: Register the icon in gresource**

In `src/euphonica.gresource.xml`, find the `<gresource prefix="/io/github/htkhiem/Euphonica/icons/scalable/actions/">` block. Add an entry alongside the others:

```xml
    <file preprocess="xml-stripblanks" alias="genre-symbolic.svg">gtk/icons/genre-symbolic.svg</file>
```

- [ ] **Step 3: Add the sidebar button to `src/gtk/sidebar.ui`**

In `src/gtk/sidebar.ui`, find the `<object class="EuphonicaSidebarButton" id="artists_btn">` block (around line 31). Just after that `<child>...</child>` block ends and before the `folders_btn` `<child>` begins (around line 37), insert:

```xml
						<child>
							<object class="EuphonicaSidebarButton" id="genres_btn">
								<property name="group">recent_btn</property>
								<property name="label" translatable="true">Genres</property>
								<property name="icon_name">genre-symbolic</property>
							</object>
						</child>
```

The `group` property ties this toggle button to the same group as the other top-level buttons (so only one is active at a time).

- [ ] **Step 4: Add the template child + toggle handler in `src/sidebar/sidebar.rs`**

In `src/sidebar/sidebar.rs`, find the `pub struct Sidebar { ... }` (around line 19). After the line `pub artists_btn: TemplateChild<SidebarButton>,`, add:

```rust
        #[template_child]
        pub genres_btn: TemplateChild<SidebarButton>,
```

In the same file, find the `setup` method's existing `artists_btn` block (around line 150). Just below it, add:

```rust
        self.imp().genres_btn.connect_toggled(clone!(
            #[weak]
            stack,
            move |btn| {
                if btn.is_active() {
                    stack.set_visible_child_name("genres");
                }
            }
        ));
```

Then in the same method, find the loop that wires up `show-sidebar` clicks (around line 407). Add `&self.imp().genres_btn.get(),` to the array literal alongside the other entries.

- [ ] **Step 5: Verify it compiles**

```bash
meson compile -C build
```

Expected: no errors.

- [ ] **Step 6: Commit**

```bash
git add src/gtk/icons/genre-symbolic.svg src/euphonica.gresource.xml src/gtk/sidebar.ui src/sidebar/sidebar.rs
git commit -m "Wire Genres entry into sidebar with new symbolic icon"
```

---

## Task 12: Window integration — stack page + setup + signal handler

**Files:**
- Modify: `src/window.ui`
- Modify: `src/window.rs`

- [ ] **Step 1: Add the stack page in `src/window.ui`**

Open `src/window.ui`. Find the existing `<object class="EuphonicaArtistView" id="artist_view">` stack page (around line 253) and its enclosing `<object class="GtkStackPage">`. Just before the folders stack page (the one with `<property name="name">folders</property>`), insert:

```xml
                        <child>
                          <object class="GtkStackPage">
                            <property name="title" translatable="true">Genres</property>
                            <property name="name">genres</property>
                            <property name="child">
                              <object class="EuphonicaGenreView" id="genre_view">
                              </object>
                            </property>
                          </object>
                        </child>
```

- [ ] **Step 2: Add the `collapsed` setter**

Near the top of `src/window.ui` (around line 28-29), the existing `setter` lines bind `collapsed` for `album_view` and `artist_view`. Add:

```xml
        <setter object="genre_view" property="collapsed">true</setter>
```

(Place it after the `artist_view` setter.)

- [ ] **Step 3: Import `GenreView` in `src/window.rs`**

Find the `library::{...}` import block (around line 25). Add `GenreContentView, GenreView,` to the list:

```rust
    library::{
        AlbumView, ArtistContentView, ArtistView, DynamicPlaylistView, FolderView,
        GenreContentView, GenreView, PlaylistView, RecentView,
    },
```

- [ ] **Step 4: Add the template child**

Find the `pub artist_view: TemplateChild<ArtistView>,` line (around line 169). After it, add:

```rust
        #[template_child]
        pub genre_view: TemplateChild<GenreView>,
```

- [ ] **Step 5: Add to the show-sidebar Widget array**

Find the array literal around line 430 that lists `recent_view`, `album_view`, etc. for the show-sidebar wiring. Add `self.genre_view.upcast_ref::<gtk::Widget>(),` after the artist_view entry.

- [ ] **Step 6: Add the setup call**

Find the `win.imp().artist_view.setup(...)` call (around line 953). Just after it, add:

```rust
        win.imp()
            .genre_view
            .setup(app.get_library(), app.get_cache());
```

- [ ] **Step 7: Wire the album-clicked signal handler**

Find the existing `win.imp().artist_view.get_content_view().connect_closure("album-clicked", ...)` block (around line 1042). Just after it, add:

```rust
        win.imp().genre_view.get_content_view().connect_closure(
            "album-clicked",
            false,
            closure_local!(
                #[watch(rename_to = this)]
                win,
                move |_: GenreContentView, album: Album| {
                    this.goto_album(&album);
                }
            ),
        );
```

- [ ] **Step 8: Verify it compiles**

```bash
meson compile -C build
```

Expected: no errors. If you get a "no method named `goto_album` found" or similar error, double-check that `Album` is in scope in `window.rs` (it is — already imported via the `common::{Album, Artist, ...}` block).

- [ ] **Step 9: Commit**

```bash
git add src/window.ui src/window.rs
git commit -m "Register GenreView in window stack and route album-clicked to goto_album"
```

---

## Task 13: Preferences UI for genre delimiters

**Files:**
- Modify: `src/gtk/preferences/library.ui`
- Modify: `src/preferences/library.rs`

- [ ] **Step 1: Add the genre delimiter section to the UI**

In `src/gtk/preferences/library.ui`, find the closing `</object>` of the `<object class="AdwPreferencesGroup"><property name="title">Artists</property>` group (around line 109). Just after that closing tag, add a new sibling group:

```xml
		<child>
			<object class="AdwPreferencesGroup">
				<property name="title" translatable="true">Genres</property>
				<child>
					<object class="AdwExpanderRow">
						<property name="title" translatable="true">Genre tag delimiters</property>
						<property name="subtitle" translatable="true">Terms used to separate genres in your tags. Specify one on each line. The default is comma, semicolon, and forward slash.</property>
						<child>
							<object class="GtkListBoxRow">
								<style>
									<class name="padding-0"/>
								</style>
								<child>
									<object class="GtkScrolledWindow">
										<property name="hexpand">true</property>
										<property name="height-request">180</property>
										<child>
											<object class="GtkTextView" id="genre_delims">
												<property name="monospace">true</property>
											</object>
										</child>
									</object>
								</child>
							</object>
						</child>
						<child>
							<object class="AdwActionRow">
								<child type="suffix">
									<object class="GtkButton" id="genre_delims_apply">
										<property name="sensitive">false</property>
										<property name="valign">center</property>
										<property name="label" translatable="true">Save</property>
										<style>
											<class name="suggested-action"/>
										</style>
									</object>
								</child>
							</object>
						</child>
					</object>
				</child>
				<child>
					<object class="AdwExpanderRow">
						<property name="title" translatable="true">Delimiter exceptions</property>
						<property name="subtitle" translatable="true">In case some genre names contain delimiter characters (e.g. "AC/DC" with the slash delimiter), place them here.</property>
						<child>
							<object class="GtkListBoxRow">
								<style>
									<class name="padding-0"/>
								</style>
								<child>
									<object class="GtkScrolledWindow">
										<property name="hexpand">true</property>
										<property name="height-request">180</property>
										<child>
											<object class="GtkTextView" id="genre_excepts">
												<property name="monospace">true</property>
											</object>
										</child>
									</object>
								</child>
							</object>
						</child>
						<child>
							<object class="AdwActionRow">
								<child type="suffix">
									<object class="GtkButton" id="genre_excepts_apply">
										<property name="sensitive">false</property>
										<property name="valign">center</property>
										<property name="label" translatable="true">Save</property>
										<style>
											<class name="suggested-action"/>
										</style>
									</object>
								</child>
							</object>
						</child>
					</object>
				</child>
			</object>
		</child>
```

- [ ] **Step 2: Add template children to `src/preferences/library.rs`**

In the `imp::LibraryPreferences` struct (around line 14), after the existing `artist_excepts_apply` field, add:

```rust
        #[template_child]
        pub genre_delims: TemplateChild<gtk::TextView>,
        #[template_child]
        pub genre_delims_apply: TemplateChild<gtk::Button>,
        #[template_child]
        pub genre_excepts: TemplateChild<gtk::TextView>,
        #[template_child]
        pub genre_excepts_apply: TemplateChild<gtk::Button>,
```

- [ ] **Step 3: Add the wiring in `setup`**

In the `setup` method, after the existing `artist_excepts_apply.connect_clicked` block (just before the closing `}` of `setup`), add:

```rust
        // Setup genre section
        let genre_delims_buf = imp.genre_delims.buffer();
        let genre_delims_apply = imp.genre_delims_apply.get();
        genre_delims_buf.set_text(
            &library_settings
                .value("genre-tag-delims")
                .array_iter_str()
                .unwrap()
                .collect::<Vec<&str>>()
                .join("\n"),
        );
        genre_delims_buf.connect_changed(clone!(
            #[weak]
            genre_delims_apply,
            move |_| {
                genre_delims_apply.set_sensitive(true);
            }
        ));
        genre_delims_apply.connect_clicked(clone!(
            #[weak]
            library_settings,
            #[weak]
            genre_delims_buf,
            move |btn| {
                let _ = library_settings.set_value(
                    "genre-tag-delims",
                    &genre_delims_buf
                        .text(
                            &genre_delims_buf.start_iter(),
                            &genre_delims_buf.end_iter(),
                            false,
                        )
                        .to_string()
                        .lines()
                        .collect::<Vec<&str>>()
                        .to_variant(),
                );
                btn.set_sensitive(false);
                utils::rebuild_genre_delim_automaton();
            }
        ));

        let genre_excepts_buf = imp.genre_excepts.buffer();
        let genre_excepts_apply = imp.genre_excepts_apply.get();
        genre_excepts_buf.set_text(
            &library_settings
                .value("genre-tag-delim-exceptions")
                .array_iter_str()
                .unwrap()
                .collect::<Vec<&str>>()
                .join("\n"),
        );
        genre_excepts_buf.connect_changed(clone!(
            #[weak]
            genre_excepts_apply,
            move |_| {
                genre_excepts_apply.set_sensitive(true);
            }
        ));
        genre_excepts_apply.connect_clicked(clone!(
            #[weak]
            library_settings,
            #[weak]
            genre_excepts_buf,
            move |btn| {
                let _ = library_settings.set_value(
                    "genre-tag-delim-exceptions",
                    &genre_excepts_buf
                        .text(
                            &genre_excepts_buf.start_iter(),
                            &genre_excepts_buf.end_iter(),
                            false,
                        )
                        .to_string()
                        .lines()
                        .collect::<Vec<&str>>()
                        .to_variant(),
                );
                btn.set_sensitive(false);
                utils::rebuild_genre_delim_exception_automaton();
            }
        ));
```

- [ ] **Step 4: Verify it compiles**

```bash
meson compile -C build
```

Expected: no errors.

- [ ] **Step 5: Commit**

```bash
git add src/gtk/preferences/library.ui src/preferences/library.rs
git commit -m "Add Genres section to Library preferences"
```

---

## Task 14: Manual smoke test

This is the final verification pass. There is no code change.

Pre-requisite: a working MPD instance with at least the following test data variants in its library (you may have to retag a few files):

- One album where every track has a single Genre tag of `Rock`.
- One album where every track has a single Genre tag of `Rock, Pop` (one comma-compound value).
- One album where every track has two Genre tags `Rock` and `Pop` (two separate `Genre:` lines, MPD multi-value).
- One album where every track has a Genre of `Drum & Bass`.
- (Optional) One album where every track has a Genre of `Rock & Roll`.

- [ ] **Step 1: Install and run**

```bash
meson install -C build
euphonica
```

Connect Euphonica to your test MPD instance via Preferences → Connection if you haven't already.

- [ ] **Step 2: Genres list appearance**

Click the new **Genres** entry in the sidebar. Verify:

- A grid of genre tiles appears.
- The tiles include `Rock`, `Pop`, and `Drum & Bass` (and `Rock & Roll` if you added it).
- The tiles do **not** include `Rock, Pop` as a single tile (it should have been split).
- Search and ascending/descending sort both work.

- [ ] **Step 3: Album-by-genre verification**

Click the `Rock` tile. Verify:

- The albums tagged with the comma-compound `Rock, Pop` appear.
- The albums with multi-value `Rock` + `Pop` appear.
- The album with single-value `Rock` appears.
- The `Rock & Roll`-only album does **not** appear (false-positive verification).
- Click an album cell → confirm it pushes the existing `AlbumContentView` (sidebar should switch to Albums).

- [ ] **Step 4: Drum & Bass verification (no false split)**

Navigate back, click `Drum & Bass`. Verify the album appears (i.e. `&` did not split).

- [ ] **Step 5: Refresh path**

Press F5 (the existing refresh shortcut). Verify the Genres list reloads after MPD reconnection without crashing.

- [ ] **Step 6: Preferences round-trip**

Open Preferences → Library → Genres. Add `\` (backslash) to the delimiters list, click Save. Then reload the library (refresh / disconnect + reconnect). The next Genres view should split anything containing a backslash. (Skip this step if you have no library data with backslashes.)

- [ ] **Step 7: Final commit (only if you made any fixes during smoke testing)**

If smoke testing surfaced any bug, fix it, re-test, and commit with a descriptive message. If it all passed, no commit is needed.

---

## Self-review checklist (run mentally before declaring done)

- **Spec coverage:** Genres sidebar entry → Task 11. Genre tile grid + LazyInit → Task 10. Per-genre album grid → Tasks 7 + 9. Album → AlbumContentView via existing goto → Task 12. Hybrid splitter (multi-value vs single) → Task 3. New gschema keys + `state.genreview` → Task 1. `SongInfo.genres` field → Task 4. `library` controller plumbing → Tasks 6 + 7. Preferences UI → Task 13. Edge case: empty exceptions list does not disable splitting → covered by `parse_genre_tag` design + dedicated test.
- **Type names that recur across tasks:** `Genre`, `GenreCell`, `GenreView`, `GenreContentView`, `parse_genre_tag`, `parse_genre_values`, `init_genres`, `get_albums_by_genre`, `get_genres`, `genres_btn`, `genre_view`, `EuphonicaGenre*` GObject names. Cross-checked between Task 3 / Task 6 / Task 7 / Task 9 / Task 10 / Task 11 / Task 12 — names match.
- **No placeholders** ("TBD", "implement later", "similar to Task N") — verified.
- **Code shown for every code step** — verified.
