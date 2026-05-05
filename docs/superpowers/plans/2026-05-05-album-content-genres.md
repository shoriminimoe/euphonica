# Album Content Genres — implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Display the union of genres across an album's tracks as pill buttons on `AlbumContentView`, where clicking a pill switches the sidebar to Genres and pushes the per-genre album list.

**Architecture:** A new `GenreTag` widget (a `gtk::Button` mirroring `ArtistTag` minus the avatar) is appended to a new `AdwWrapBox` (`genres_box`) below the existing metadata row. Genre tags are populated inside the existing `bind()` post-fetch async closure, after the song list arrives, by deduping `SongInfo.genres` across tracks. Clicking calls a new `EuphonicaWindow::goto_genre(&Genre)` method that mirrors `goto_artist`.

**Tech Stack:** Rust 2024, GTK4 + libadwaita via gtk-rs, `rustc-hash::FxHashSet`. Build via Meson driving Cargo through Flatpak (Ubuntu 24.04 host gtk4 is too old for native).

**Spec reference:** `docs/superpowers/specs/2026-05-05-album-content-genres-design.md`.

---

## Pre-flight notes

This codebase has **no automated test harness**. Verification is by Flatpak build + manual smoke test (Task 5 hands back to the user). Each preceding task should end with a clean Flatpak build.

**Build commands:**

```bash
# Single build that also reinstalls the user-level Flatpak so `flatpak run --branch=master io.github.htkhiem.Euphonica` picks up the change:
fusermount -u /home/sam/Projects/euphonica/.flatpak-builder/rofiles/* 2>/dev/null
rm -rf /home/sam/Projects/euphonica/.flatpak-builder/rofiles
flatpak-builder --force-clean --user --install --install-deps-from=flathub --repo=repo build-flatpak io.github.htkhiem.Euphonica-dev.json > /tmp/flatpak-task<N>-build.log 2>&1
```

The first cleanup line handles a stale `rofiles-fuse` mount that builds sometimes leave behind. The build takes ~3 minutes when cargo deps are cached.

**Branch:** Work on `feat/album-content-genres` (already created from `feat/genres-view`). Do NOT switch branches.

**Commit cadence:** Short imperative title per task, no Conventional-Commits prefix. Optional `Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>` trailer.

---

## File map

**New files:**

- `src/library/genre_tag.rs` — `GenreTag` GObject (button subclass, label-only) with click handler calling `window.goto_genre(...)`.
- `src/gtk/library/genre-tag.ui` — Template (GtkButton parent + GtkLabel child) styled as a pill, mirroring `artist-tag.ui` minus the avatar.

**Modified files:**

- `src/sidebar/sidebar.rs` — Add `"genres"` arm to `Sidebar::set_view`'s match.
- `src/window.rs` — Add `pub fn goto_genre(&self, genre: &Genre)` mirroring `goto_artist`. Add `Genre` to the `crate::common::{...}` import block.
- `src/library/mod.rs` — `pub mod genre_tag;` and `pub use genre_tag::GenreTag;`.
- `src/euphonica.gresource.xml` — Register `gtk/library/genre-tag.ui`.
- `src/gtk/library/album-content-view.ui` — Insert a new `AdwWrapBox` named `genres_box` between `metadata_box` and `infobox_spinner`, initially `visible="false"`.
- `src/library/album_content_view.rs` — Add `genres_box` template child, `genre_tags: gio::ListStore<GenreTag>` field, populate inside the existing `bind()` post-fetch async closure, clear inside `unbind()`.

---

## Task 1: Sidebar `set_view` arm + `goto_genre` window method

**Files:**
- Modify: `src/sidebar/sidebar.rs:444-453`
- Modify: `src/window.rs` (add `Genre` to imports + `goto_genre` after `goto_artist`)

This task wires up the navigation target before any caller exists. Building this first means later tasks (the GenreTag widget that calls `goto_genre`) compile cleanly.

- [ ] **Step 1: Add the `"genres"` arm to `Sidebar::set_view`**

In `src/sidebar/sidebar.rs`, locate the `set_view` match (lines 444-453). Insert a `"genres"` arm. The current match looks like:

```rust
    pub fn set_view(&self, view_name: &str) {
        // TODO: something less dumb than this
        match view_name {
            "albums" => self.imp().albums_btn.set_active(true),
            "artists" => self.imp().artists_btn.set_active(true),
            "queue" => self.imp().queue_btn.set_active(true),
            "playlists" => self.imp().playlists_btn.set_active(true),
            _ => unimplemented!(),
        };
    }
```

Add the `"genres"` arm immediately after the `"artists"` arm:

```rust
    pub fn set_view(&self, view_name: &str) {
        // TODO: something less dumb than this
        match view_name {
            "albums" => self.imp().albums_btn.set_active(true),
            "artists" => self.imp().artists_btn.set_active(true),
            "genres" => self.imp().genres_btn.set_active(true),
            "queue" => self.imp().queue_btn.set_active(true),
            "playlists" => self.imp().playlists_btn.set_active(true),
            _ => unimplemented!(),
        };
    }
```

The `genres_btn` template child already exists on the `Sidebar` struct (added during the genres-view feature).

- [ ] **Step 2: Add `Genre` to `src/window.rs`'s `crate::common::{...}` import block**

In `src/window.rs`, locate the existing import statement that brings in `Album`, `Artist`, etc. from `crate::common`. The block is approximately:

```rust
    common::{Album, Artist, INode, ThemeSelector, blend_mode::*, paintables::FadePaintable},
```

Add `Genre` alphabetically:

```rust
    common::{Album, Artist, Genre, INode, ThemeSelector, blend_mode::*, paintables::FadePaintable},
```

- [ ] **Step 3: Add `goto_genre` method**

In `src/window.rs`, find the existing `pub fn goto_artist(&self, artist: &Artist)` (around line 1278). Just AFTER it (and before `pub fn goto_playlist` around line 1288), add:

```rust
    pub fn goto_genre(&self, genre: &Genre) {
        self.imp().genre_view.on_genre_clicked(genre);
        self.imp().sidebar.set_view("genres");
        if self.imp().split_view.shows_sidebar() {
            self.imp()
                .split_view
                .set_show_sidebar(!self.imp().split_view.is_collapsed());
        }
    }
```

The `genre_view` template child and `on_genre_clicked` method already exist (from the genres-view feature). `Genre` is now in scope from Step 2.

- [ ] **Step 4: Verify the build**

```bash
fusermount -u /home/sam/Projects/euphonica/.flatpak-builder/rofiles/* 2>/dev/null
rm -rf /home/sam/Projects/euphonica/.flatpak-builder/rofiles
flatpak-builder --force-clean --user --install --install-deps-from=flathub --repo=repo build-flatpak io.github.htkhiem.Euphonica-dev.json > /tmp/flatpak-task1-build.log 2>&1
```

Wait for completion. Confirm exit code 0 and the log ends with `Pruning cache`. **Do not return until the build is verified done.**

The build's only new compile warning relative to baseline should be `goto_genre` being unused — that's expected; Task 2's GenreTag will consume it.

- [ ] **Step 5: Commit**

```bash
git add src/sidebar/sidebar.rs src/window.rs
git commit -m "Add goto_genre window method and Sidebar set_view arm for genres"
```

---

## Task 2: GenreTag widget + UI template

**Files:**
- Create: `src/library/genre_tag.rs`
- Create: `src/gtk/library/genre-tag.ui`
- Modify: `src/library/mod.rs`
- Modify: `src/euphonica.gresource.xml`

- [ ] **Step 1: Create the UI template**

Write `src/gtk/library/genre-tag.ui`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<interface>
  <template class="EuphonicaGenreTag" parent="GtkButton">
    <style>
      <class name="circular"/>
    </style>
    <child>
      <object class="GtkLabel" id="name">
        <property name="ellipsize">3</property>
        <property name="margin-start">8</property>
        <property name="margin-end">8</property>
      </object>
    </child>
  </template>
</interface>
```

This mirrors `src/gtk/library/artist-tag.ui` but drops the inner GtkBox + AdwAvatar and uses the GtkLabel directly as the only child (since there's no avatar/label split to manage). The `circular` style class gives the rounded pill shape. The `margin-start`/`margin-end` of 8 compensate for the lack of the avatar's horizontal real estate so the pill isn't unreasonably narrow on short genre names.

- [ ] **Step 2: Create the widget**

Write `src/library/genre_tag.rs`:

```rust
use gtk::{glib, prelude::*, subclass::prelude::*, CompositeTemplate};
use glib::{clone, Object};
use std::cell::OnceCell;

use crate::{common::Genre, window::EuphonicaWindow};

mod imp {
    use super::*;

    #[derive(Default, CompositeTemplate)]
    #[template(resource = "/io/github/htkhiem/Euphonica/gtk/library/genre-tag.ui")]
    pub struct GenreTag {
        #[template_child]
        pub name: TemplateChild<gtk::Label>,
        pub genre: OnceCell<Genre>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for GenreTag {
        const NAME: &'static str = "EuphonicaGenreTag";
        type Type = super::GenreTag;
        type ParentType = gtk::Button;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for GenreTag {}
    impl WidgetImpl for GenreTag {}
    impl ButtonImpl for GenreTag {}
}

glib::wrapper! {
    pub struct GenreTag(ObjectSubclass<imp::GenreTag>)
        @extends gtk::Button, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Actionable;
}

impl GenreTag {
    pub fn new(genre_name: &str, window: &EuphonicaWindow) -> Self {
        let res: Self = Object::builder().build();
        let genre = Genre::new(genre_name);
        res.imp().name.set_label(genre.get_name());
        let _ = res.imp().genre.set(genre);

        res.connect_clicked(clone!(
            #[weak(rename_to = this)]
            res,
            #[weak]
            window,
            move |_| {
                window.goto_genre(this.imp().genre.get().unwrap());
            }
        ));

        res
    }

    pub fn get_name(&self) -> glib::GString {
        self.imp().name.label()
    }
}
```

The constructor takes a name string (not a `Genre`) because callers in `AlbumContentView` will iterate over `SongInfo.genres: Vec<String>` and have nothing else; the widget then mints its own `Genre` GObject for use in the click handler.

- [ ] **Step 3: Register the new UI in gresource**

In `src/euphonica.gresource.xml`, find the block that lists `gtk/library/*.ui` files (around line 27 — alongside `artist-tag.ui`, `album-cell.ui`, etc.). Add:

```xml
    <file preprocess="xml-stripblanks">gtk/library/genre-tag.ui</file>
```

Place it near the other `genre-*.ui` entries or near `artist-tag.ui` — both are reasonable. Match the surrounding indentation.

- [ ] **Step 4: Wire `genre_tag` into `src/library/mod.rs`**

Open `src/library/mod.rs`. Add a `mod genre_tag;` declaration alongside the other genre-related `mod` lines (the genres-view feature added `mod genre_cell;`, `mod genre_content_view;`, `mod genre_view;` — `mod genre_tag;` slots in alphabetically among them).

Then add a re-export. Look at how `ArtistTag` is exposed: it lives at `src/library/artist_tag.rs` and is `use`d internally by `album_content_view.rs` via `use super::{Library, artist_tag::ArtistTag};` — so it's NOT in the `pub use` block at the bottom of `mod.rs`. Mirror that pattern: add NO `pub use` for `GenreTag`. Instead, `album_content_view.rs` (Task 4) will import it via `use super::genre_tag::GenreTag;`.

So just one line is added to `src/library/mod.rs`:

```rust
mod genre_tag;
```

Place it between the existing `mod genre_view;` and `mod folder_view;` (or wherever maintains alphabetical/logical grouping with the other `genre_*` mods).

- [ ] **Step 5: Verify the build**

```bash
fusermount -u /home/sam/Projects/euphonica/.flatpak-builder/rofiles/* 2>/dev/null
rm -rf /home/sam/Projects/euphonica/.flatpak-builder/rofiles
flatpak-builder --force-clean --user --install --install-deps-from=flathub --repo=repo build-flatpak io.github.htkhiem.Euphonica-dev.json > /tmp/flatpak-task2-build.log 2>&1
```

Wait for completion. Confirm exit code 0. The `goto_genre` warning from Task 1 should now be gone (consumed by GenreTag's click handler). A new "unused: GenreTag" or similar warning may appear since nothing instantiates it yet — that's expected; Task 4 will consume it.

- [ ] **Step 6: Commit**

```bash
git add src/library/genre_tag.rs src/gtk/library/genre-tag.ui src/library/mod.rs src/euphonica.gresource.xml
git commit -m "Add GenreTag pill button widget"
```

---

## Task 3: AlbumContentView UI template — `genres_box`

**Files:**
- Modify: `src/gtk/library/album-content-view.ui`

- [ ] **Step 1: Insert `genres_box` between `metadata_box` and `infobox_spinner`**

Open `src/gtk/library/album-content-view.ui`. Locate the existing `<object class="GtkBox" id="metadata_box">` element (starts around line 97). Find its closing `</object>` (around line 164) and the `</child>` that wraps it (around line 165).

Just AFTER that `</child>` (and BEFORE the `<child>` that contains `<object class="GtkStack" id="infobox_spinner">` around line 167), insert:

```xml
                    <child>
                      <object class="AdwWrapBox" id="genres_box">
                        <property name="child-spacing">3</property>
                        <property name="line-spacing">3</property>
                        <property name="justify">0</property>
                        <property name="justify-last-line">false</property>
                        <property name="align">0.0</property>
                        <property name="visible">false</property>
                      </object>
                    </child>
```

The first five `AdwWrapBox` properties mirror `artists_box`'s configuration verbatim (ensure layout consistency with the artist row above). `visible="false"` keeps the box collapsed until the bind closure determines whether any genre tags exist.

Match the indentation of the surrounding `<child>` blocks — they use spaces (the file uses 2-space indentation per nesting level).

- [ ] **Step 2: Verify the build**

```bash
fusermount -u /home/sam/Projects/euphonica/.flatpak-builder/rofiles/* 2>/dev/null
rm -rf /home/sam/Projects/euphonica/.flatpak-builder/rofiles
flatpak-builder --force-clean --user --install --install-deps-from=flathub --repo=repo build-flatpak io.github.htkhiem.Euphonica-dev.json > /tmp/flatpak-task3-build.log 2>&1
```

Wait for completion. Confirm exit code 0. The .ui file is consumed at runtime, but the gresource bundle compilation happens at build time, so the build will fail if the XML is malformed.

- [ ] **Step 3: Commit**

```bash
git add src/gtk/library/album-content-view.ui
git commit -m "Add genres_box wrap to AlbumContentView template"
```

---

## Task 4: AlbumContentView wiring — populate and clean up `genres_box`

**Files:**
- Modify: `src/library/album_content_view.rs`

This task adds the Rust-side wiring: the new template child, the ListStore for tags, the population code in `bind()`, and the cleanup in `unbind()`.

- [ ] **Step 1: Add the import**

At the top of `src/library/album_content_view.rs`, find the existing `use super::{Library, artist_tag::ArtistTag};` line (around line 1). Add `genre_tag::GenreTag` to it:

```rust
use super::{Library, artist_tag::ArtistTag, genre_tag::GenreTag};
```

- [ ] **Step 2: Add the `genres_box` template child + `genre_tags` ListStore field**

In the `imp::AlbumContentView` struct definition, locate the existing `pub artists_box: TemplateChild<adw::WrapBox>` field (around line 43). After it (or anywhere in the template-children block), add:

```rust
        #[template_child]
        pub genres_box: TemplateChild<adw::WrapBox>,
```

Then locate the existing `artist_tags: gio::ListStore` field (around line 87-88, with the `#[derivative(Default(value = ...))]` attribute). After it, add:

```rust
        #[derivative(Default(value = "gio::ListStore::new::<GenreTag>()"))]
        pub genre_tags: gio::ListStore,
```

- [ ] **Step 3: Populate genre tags in `bind()`'s post-fetch closure**

In `src/library/album_content_view.rs`, locate the existing `bind()` method (around line 808). The async closure inside it (starting at line 884) currently looks like:

```rust
        glib::spawn_future_local(clone!(
            #[weak(rename_to = this)]
            self,
            #[strong]
            album,
            async move {
                let library = this.imp().library.upgrade().unwrap();
                // Important, MPD-side content first
                let stack = this.imp().content_stack.get();
                stack.show_spinner();
                let song_list = this.imp().song_list.clone();
                song_list.remove_all();
                match library
                    .get_album_songs(&album, &mut |songs| {
                        song_list.extend_from_slice(&songs);
                    })
                    .await
                {
                    Ok(()) => {
                        if song_list.n_items() > 0 {
                            stack.show_content();
                        } else {
                            stack.show_placeholder();
                        }
                    }
                    Err(e) => {
                        dbg!(e);
                    }
                };
                this.imp().runtime.set_label(&format_secs_as_duration(
                    song_list
                        .iter()
                        .map(|item: Result<Song, _>| {
                            if let Ok(song) = item {
                                return song.get_duration();
                            }
                            0
                        })
                        .sum::<u64>() as f64,
                ));
                // The extra fluff later
                this.schedule_cover(false).await;
                this.update_meta(false).await;
            }
        ));
```

Insert the genre-population block immediately after the `runtime.set_label(...)` call (just before the `// The extra fluff later` comment around line 924).

You will need `FxHashSet` from `rustc_hash`. Check the existing `use` statements at the top of `album_content_view.rs`. If `rustc_hash::FxHashSet` is not already imported, add `use rustc_hash::FxHashSet;` near the top of the file.

The new block to insert is:

```rust
                // Populate genre tags from union of songs' genres.
                let genres_box = this.imp().genres_box.get();
                let window = this.imp().window.upgrade().unwrap();
                let mut seen: FxHashSet<String> = FxHashSet::default();
                let mut new_tags: Vec<GenreTag> = Vec::new();
                for item in song_list.iter::<Song>() {
                    if let Ok(song) = item {
                        for genre in song.get_info().genres.iter() {
                            if seen.insert(genre.clone()) {
                                new_tags.push(GenreTag::new(genre, &window));
                            }
                        }
                    }
                }
                this.imp().genre_tags.extend_from_slice(&new_tags);
                for tag in new_tags {
                    genres_box.append(&tag);
                }
                genres_box.set_visible(!seen.is_empty());
```

The `song.get_info()` accessor returns `&SongInfo` (it already exists — used elsewhere in the codebase).

- [ ] **Step 4: Add the cleanup in `unbind()`**

In the same file, find the `unbind()` method (around line 931). Inside it, find the existing block that clears `artists_box` (around lines 936-940):

```rust
        // Clear artists wrapbox. TODO: when adw 1.8 drops as stable please use remove_all() instead.
        for tag in self.imp().artist_tags.iter::<gtk::Widget>() {
            self.imp().artists_box.remove(&tag.unwrap());
        }
        self.imp().artist_tags.remove_all();
```

Just AFTER that block, add an analogous block for genres:

```rust
        // Clear genres wrapbox. TODO: when adw 1.8 drops as stable please use remove_all() instead.
        for tag in self.imp().genre_tags.iter::<gtk::Widget>() {
            self.imp().genres_box.remove(&tag.unwrap());
        }
        self.imp().genre_tags.remove_all();
        self.imp().genres_box.set_visible(false);
```

The `set_visible(false)` resets the box for the next bind cycle (so an album with no genres doesn't display a leftover-from-previous row).

- [ ] **Step 5: Verify the build**

```bash
fusermount -u /home/sam/Projects/euphonica/.flatpak-builder/rofiles/* 2>/dev/null
rm -rf /home/sam/Projects/euphonica/.flatpak-builder/rofiles
flatpak-builder --force-clean --user --install --install-deps-from=flathub --repo=repo build-flatpak io.github.htkhiem.Euphonica-dev.json > /tmp/flatpak-task4-build.log 2>&1
```

Wait for completion. Confirm exit code 0. Previously expected unused warnings (`GenreTag`, `goto_genre`) should now be gone — both are consumed.

- [ ] **Step 6: Commit**

```bash
git add src/library/album_content_view.rs
git commit -m "Populate genre tags in AlbumContentView bind cycle"
```

---

## Task 5: Manual smoke test (handed back to user)

This task is for the user to run, not a subagent. There is no code change.

Pre-requisite: an album in MPD with multiple atomic genres across its tracks. If you have an album where one track has `Rock, Pop` (compound) and another has `Jazz`, the post-split union should be `Rock`, `Pop`, `Jazz`.

- [ ] **Step 1: Run the dev build**

```bash
flatpak run --branch=master io.github.htkhiem.Euphonica
```

(Replace `--branch=master` with whatever distinguishes your dev install if you've changed the manifest's app id.)

- [ ] **Step 2: Open an album with genres**

Navigate to Albums → click any album whose tracks have Genre tags. Below the Originally released / Tracks / Runtime row, you should see one or more pill buttons, one per atomic genre.

- [ ] **Step 3: Click a genre pill**

Click any genre pill. Expect:
- The sidebar's active item switches to **Genres**.
- The Genres view's per-genre album list opens, scoped to the genre you clicked.
- The album you started on appears in that list.

- [ ] **Step 4: Open an album with no genres**

If you have an album whose tracks have no Genre tags, navigate to it. Expect: no genre row visible (no empty space between the metadata block and the album wiki / track list).

- [ ] **Step 5: Bind / unbind cycle**

Navigate from one album with genres to another with different genres. Expect: no leftover pills from the previous album.

- [ ] **Step 6: Compound genre verification**

If any of your album's tracks has a compound genre tag like `Rock, Pop`, verify it appears as TWO pills (`Rock` + `Pop`), not one (the splitter from the genres-view feature should already be doing this).

- [ ] **Step 7: Done**

If all 6 checks pass, the feature is complete. Branch `feat/album-content-genres` is ready to merge. If any check fails, the controller will dispatch a fix subagent.

---

## Self-review checklist (run mentally before declaring plan complete)

- **Spec coverage:**
  - Pill button widget under metadata row → Tasks 2 + 3 + 4.
  - Union of post-split genres across tracks → Task 4 (uses `SongInfo.genres`).
  - Click → switches sidebar to Genres + pushes per-genre list → Task 1.
  - Hidden when zero genres → Task 4 (`set_visible(!seen.is_empty())`).
  - `unbind()` cleanup → Task 4.
  - `Sidebar::set_view` arm → Task 1.
- **Type / method names that recur:** `GenreTag` (type, Tasks 2 + 4), `GenreTag::new(name: &str, window: &EuphonicaWindow)` (Task 2 → Task 4), `goto_genre(&self, genre: &Genre)` (Task 1 → Task 2's click handler), `Genre::new(name: &str)` (already exists from genres-view feature; consumed by Task 2). All consistent across tasks.
- **No placeholders** ("TBD", "implement later", "similar to Task N") — none.
- **Code shown for every code step** — yes.
