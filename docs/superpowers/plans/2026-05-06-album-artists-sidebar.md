# Album Artists Sidebar Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a parallel "Album Artists" sidebar entry that lists artists from MPD's `AlbumArtist` tag (alongside the existing Artists entry which uses `Artist`), reusing the existing `ArtistView` and `ArtistContentView` widget classes parameterized by an `ArtistKind` enum.

**Architecture:** A new `ArtistKind { Artist, AlbumArtist }` enum drives parameterization. Two `ArtistView` instances live in the window stack, each with its own kind, each backed by a separate `Library` ListStore (`artists` + `album_artists`). Sidebar navigation, content fetch, and population dispatch on kind. No new widget types are introduced.

**Tech Stack:** Rust 2024, GTK4 + libadwaita via gtk-rs. Build via Meson driving Cargo through Flatpak (Ubuntu 24.04 host gtk4 is too old for native).

**Spec reference:** `docs/superpowers/specs/2026-05-06-album-artists-sidebar-design.md`.

---

## Pre-flight notes

This codebase has **no automated test harness**. Verification is by Flatpak build + manual smoke test (Task 7 hands back to the user). Each implementation task ends with a clean Flatpak build.

**Build commands:**

```bash
fusermount -u /home/sam/Projects/euphonica/.flatpak-builder/rofiles/* 2>/dev/null
rm -rf /home/sam/Projects/euphonica/.flatpak-builder/rofiles
CARGO_BUILD_JOBS=4 flatpak-builder --force-clean --user --install --install-deps-from=flathub --repo=repo build-flatpak io.github.htkhiem.Euphonica-dev.json > /tmp/flatpak-task<N>-build.log 2>&1
```

`CARGO_BUILD_JOBS=4` prevents OOM on this 15GB-RAM host (default parallelism has been killed mid-build before). The cleanup line handles a stale `rofiles-fuse` mount that builds sometimes leave behind. Build takes ~3 minutes when cargo deps are cached.

**Branch:** Work on `feat/album-artists-sidebar` (already created from `fix/fifo-path-entry`). Do NOT switch branches.

**Commit cadence:** Short imperative title per task, no Conventional-Commits prefix. Optional `Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>` trailer.

---

## File map

**New files:** none.

**Modified files:**

- `src/library/mod.rs` — Add and re-export `ArtistKind` enum.
- `src/library/controller.rs` — Add `album_artists` ListStore + init flag. Drop `use_album_artist` parameter from `init_artists`. Add `init_album_artists`, `album_artists()` getter, `get_album_artist_content`. Extend `clear()`.
- `src/library/artist_view.rs` — Add `kind` storage on `imp::ArtistView`. `setup` takes a `kind: ArtistKind` parameter. `populate` / `setup_gridview` / `on_artist_clicked` dispatch on kind.
- `src/library/artist_content_view.rs` — Add `kind` storage on `imp::ArtistContentView`. `setup` takes a `kind: ArtistKind` parameter. `bind`'s async closure dispatches to the right Library content-fetch method.
- `src/sidebar/sidebar.rs` — New `album_artists_btn` template child + `connect_toggled` handler + show-sidebar array entry + `set_view` arm.
- `src/gtk/sidebar.ui` — New `EuphonicaSidebarButton id="album_artists_btn"` between `artists_btn` and `folders_btn`.
- `src/window.rs` — New `album_artist_view: TemplateChild<ArtistView>`. Setup call, signal handler routing, `maybe_populate_visible` arm, show-sidebar array entry. Add `ArtistKind` argument to the existing `artist_view.setup` call.
- `src/window.ui` — New stack page; new collapsed setter line in the breakpoint block.

---

## Task 1: `ArtistKind` enum

**Files:**
- Modify: `src/library/mod.rs`

This task adds the parameterization enum that Tasks 2-4 and 6 will consume. No callers exist yet, so the build remains clean.

- [ ] **Step 1: Add the enum at the top of `src/library/mod.rs`**

Open `src/library/mod.rs`. The file currently starts with a series of `mod foo;` declarations followed by `pub use foo::Foo;` re-exports. After all the `mod ...;` declarations and before (or interleaved with) the `pub use ...;` block, add:

```rust
/// Distinguishes between the two artist kinds the library exposes:
/// the per-track Artist tag and the per-album AlbumArtist tag.
/// Used to parameterize ArtistView and ArtistContentView so the same
/// widget classes can drive both sidebar entries.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ArtistKind {
    Artist,
    AlbumArtist,
}

impl Default for ArtistKind {
    fn default() -> Self {
        ArtistKind::Artist
    }
}
```

The `Default` impl is what makes a `Cell<ArtistKind>` field default to `ArtistKind::Artist` without an explicit `#[derivative(Default(value = ...))]` attribute. This is the value the existing Artists view will end up with after the parameterization in Task 3 — i.e. existing behaviour is preserved by default.

The `Copy` derive lets us read the kind out of `Cell<ArtistKind>` cheaply via `.get()`.

- [ ] **Step 2: Verify the build**

```bash
fusermount -u /home/sam/Projects/euphonica/.flatpak-builder/rofiles/* 2>/dev/null
rm -rf /home/sam/Projects/euphonica/.flatpak-builder/rofiles
CARGO_BUILD_JOBS=4 flatpak-builder --force-clean --user --install --install-deps-from=flathub --repo=repo build-flatpak io.github.htkhiem.Euphonica-dev.json > /tmp/flatpak-task1-build.log 2>&1
```

**Wait for the build to complete** (~3 min). Confirm exit code 0 and the log ends with `Pruning cache`. Expected new warning: `ArtistKind` is unused — Task 2+ will consume it.

- [ ] **Step 3: Commit**

```bash
git add src/library/mod.rs
git commit -m "Add ArtistKind enum to parameterize artist views"
```

---

## Task 2: Library controller — split ListStores, drop flag, add album-artist methods

**Files:**
- Modify: `src/library/controller.rs`
- Modify: `src/library/artist_view.rs:437` (one-line touch-up)

This task makes Library expose two parallel ListStores (`artists` and `album_artists`) and two parallel init methods. It drops the `use_album_artist: bool` parameter from `init_artists` (now Artist-tag only). It adds `get_album_artist_content` mirroring `get_artist_content`'s shape. It also fixes the one existing call site in `artist_view.rs` so the build stays green at the end of this task — `artist_view.rs` will be more thoroughly parameterized in Task 3.

- [ ] **Step 1: Add the new fields to `imp::Library`**

In `src/library/controller.rs`, locate the `imp::Library` struct (around line 29). Find the existing block:

```rust
        #[derivative(Default(value = "gio::ListStore::new::<Artist>()"))]
        pub artists: gio::ListStore,
        pub artists_initialized: Cell<bool>,
        #[derivative(Default(value = "gio::ListStore::new::<Artist>()"))]
        pub recent_artists: gio::ListStore,
```

Right after the existing `recent_artists` field (so the new fields are co-located with the other artist-related ones), insert:

```rust
        #[derivative(Default(value = "gio::ListStore::new::<Artist>()"))]
        pub album_artists: gio::ListStore,
        pub album_artists_initialized: Cell<bool>,
```

- [ ] **Step 2: Reset the new fields in `clear()`**

Find the `pub fn clear(&self)` method (around line 128). Inside it, after the line `self.imp().recent_artists.remove_all();`, add:

```rust
        self.imp().album_artists.remove_all();
        self.imp().album_artists_initialized.set(false);
```

- [ ] **Step 3: Drop the `use_album_artist` flag from `init_artists`**

Find the existing `init_artists` method (around line 661). The current signature is:

```rust
    pub async fn init_artists(&self, use_album_artist: bool) -> ClientResult<()> {
        if !self.imp().artists_initialized.get() {
            self.imp().artists_initialized.set(true);
            let model = self.imp().artists.clone();
            model.remove_all();

            self.client()
                .get_artists(use_album_artist, &mut |artist| {
                    model.append(&artist);
                })
                .await?;
        }
        Ok(())
    }
```

Replace it entirely with:

```rust
    pub async fn init_artists(&self) -> ClientResult<()> {
        if !self.imp().artists_initialized.get() {
            self.imp().artists_initialized.set(true);
            let model = self.imp().artists.clone();
            model.remove_all();

            self.client()
                .get_artists(false, &mut |artist| {
                    model.append(&artist);
                })
                .await?;
        }
        Ok(())
    }
```

The only changes are the dropped parameter and the explicit `false` passed to `client.get_artists`.

- [ ] **Step 4: Add `init_album_artists` immediately after `init_artists`**

Right after the `init_artists` method (just before `pub async fn get_artist_content`), add:

```rust
    pub async fn init_album_artists(&self) -> ClientResult<()> {
        if !self.imp().album_artists_initialized.get() {
            self.imp().album_artists_initialized.set(true);
            let model = self.imp().album_artists.clone();
            model.remove_all();

            self.client()
                .get_artists(true, &mut |artist| {
                    model.append(&artist);
                })
                .await?;
        }
        Ok(())
    }
```

- [ ] **Step 5: Add the `album_artists()` getter**

Find the existing `pub fn artists(&self) -> gio::ListStore` getter (around line 442). Just after the existing `recent_artists()` getter and before the `playlists` getters (or wherever maintains symmetry with `artists()` / `recent_artists()`), add:

```rust
    /// Get a reference to the local album_artists store.
    pub fn album_artists(&self) -> gio::ListStore {
        self.imp().album_artists.clone()
    }
```

- [ ] **Step 6: Add `get_album_artist_content` immediately after `get_artist_content`**

Find the existing `get_artist_content` method (around line 683). Just after its closing `}` (and still inside the `impl Library` block), add:

```rust
    /// Find all albums + songs whose AlbumArtist matches this artist (after
    /// post-split comp_id verification). Mirrors `get_artist_content`'s shape
    /// but uses the AlbumArtist tag for the server-side filter and the
    /// album's parsed `artists` Vec for the client-side membership check.
    pub async fn get_album_artist_content<FA, FS>(
        &self,
        artist: &Artist,
        mut respond_album: FA,
        mut respond_song: FS,
    ) -> ClientResult<()>
    where
        FA: FnMut(Album),
        FS: FnMut(Vec<Song>),
    {
        let mut song_query = Query::new();
        song_query.and_with_op(
            Term::Tag(tags::ALBUMARTIST.into()),
            QueryOperation::Contains,
            artist.get_name().to_owned(),
        );

        let comp_id = artist.get_info().get_comp_id();
        let mut visited_albums = FxHashSet::default();
        self.client()
            .get_song_infos_by_query(song_query, true, &mut |batch| {
                let filtered: Vec<SongInfo> = batch
                    .into_iter()
                    .filter(|s| {
                        s.album
                            .as_ref()
                            .map(|a| a.artists.iter().any(|ai| ai.get_comp_id() == comp_id))
                            .unwrap_or(false)
                    })
                    .collect();
                for song in filtered.iter() {
                    if let Some(album) = song.album.as_ref()
                        && visited_albums.insert(album.get_comp_id().to_owned())
                    {
                        respond_album(album.clone().into());
                    }
                }
                respond_song(filtered.into_iter().map(|si| si.into()).collect());
            })
            .await?;

        Ok(())
    }
```

The only differences from `get_artist_content` are:
1. `Term::Tag(tags::ALBUMARTIST.into())` instead of `tags::ARTIST`.
2. The client-side filter checks `s.album.as_ref().map(|a| a.artists.iter().any(...))` instead of `s.artists.iter().any(...)`.

`tags::ALBUMARTIST` is already defined in `src/common/tags.rs` and `tags::*` is already in scope at the top of `controller.rs`. No new imports needed.

- [ ] **Step 7: Fix the existing `init_artists` call in `artist_view.rs:437`**

In `src/library/artist_view.rs`, find the existing line (around line 437):

```rust
                    let _ = library.init_artists(false).await;
```

Change it to:

```rust
                    let _ = library.init_artists().await;
```

This is the ONLY change to `artist_view.rs` in Task 2. Task 3 will do the broader parameterization.

- [ ] **Step 8: Verify the build**

```bash
fusermount -u /home/sam/Projects/euphonica/.flatpak-builder/rofiles/* 2>/dev/null
rm -rf /home/sam/Projects/euphonica/.flatpak-builder/rofiles
CARGO_BUILD_JOBS=4 flatpak-builder --force-clean --user --install --install-deps-from=flathub --repo=repo build-flatpak io.github.htkhiem.Euphonica-dev.json > /tmp/flatpak-task2-build.log 2>&1
```

**Wait for the build to complete.** Confirm exit code 0. Expected new warnings: `init_album_artists`, `album_artists`, and `get_album_artist_content` are unused — Tasks 3, 4, and 6 will consume them.

- [ ] **Step 9: Commit**

```bash
git add src/library/controller.rs src/library/artist_view.rs
git commit -m "Split Library artists into Artist + AlbumArtist parallel stores"
```

---

## Task 3: `ArtistView` parameterization

**Files:**
- Modify: `src/library/artist_view.rs`

This task adds a `kind` field to `ArtistView`'s imp struct and dispatches on it in three places: `populate` (which init method to call), `setup_gridview` (which ListStore to bind), and `on_artist_clicked` (passing kind to the content view in Task 4 — but this task can stash it on imp for now and Task 4 reads it at bind time).

- [ ] **Step 1: Import `ArtistKind`**

At the top of `src/library/artist_view.rs`, find the existing import block:

```rust
use super::{ArtistCell, ArtistContentView, Library};
```

Change it to:

```rust
use super::{ArtistCell, ArtistContentView, ArtistKind, Library};
```

Also, look near the top for `use std::cell::Cell` (or similar). The `Cell` type is needed for the new field; if not already imported, find the line `use std::{cell::Cell, ...};` and confirm `Cell` is in scope. The file already has `pub initializing: Cell<bool>` so `Cell` is already imported.

- [ ] **Step 2: Add the `kind` field to `imp::ArtistView`**

Find the `imp::ArtistView` struct (around line 21). Locate the existing `pub initializing: Cell<bool>` field (around line 62). Right after it, add:

```rust
        pub kind: Cell<ArtistKind>,
```

The default value will be `ArtistKind::Artist` because of the `Default` impl added in Task 1.

- [ ] **Step 3: Add the `kind` parameter to `setup`**

Find the existing `setup` method signature (around line 135):

```rust
    pub fn setup(&self, library: &Library, cache: Rc<Cache>, window: &EuphonicaWindow) {
        self.imp().library.set(Some(library));
        self.setup_sort();
        self.setup_search();
        self.setup_gridview(cache.clone());

        let content_view = self.imp().content_view.get();
        content_view.setup(library, cache, window);
        self.imp().content_page.connect_hidden(move |_| {
            content_view.unbind();
        });
    }
```

Replace it with:

```rust
    pub fn setup(
        &self,
        library: &Library,
        cache: Rc<Cache>,
        window: &EuphonicaWindow,
        kind: ArtistKind,
    ) {
        self.imp().kind.set(kind);
        self.imp().library.set(Some(library));
        self.setup_sort();
        self.setup_search();
        self.setup_gridview(cache.clone());

        let content_view = self.imp().content_view.get();
        content_view.setup(library, cache, window);
        self.imp().content_page.connect_hidden(move |_| {
            content_view.unbind();
        });
    }
```

Two changes: (a) accept `kind` and store it on `self.imp()`; (b) the `content_view.setup` call is left at 3 arguments. The kind isn't passed down to `ArtistContentView` in this task — Task 4 adds the parameter to that call and propagates it.

This keeps Task 3 buildable on its own. Until Task 4 lands, both `ArtistView` instances will create `ArtistContentView` instances whose `kind` defaults to `ArtistKind::Artist` (per the `Default` impl from Task 1), which means the AlbumArtist view temporarily uses Artist-tag fetching. Task 4 fixes that by accepting and storing the kind on `ArtistContentView::imp` and updating this call site to pass `kind`.

- [ ] **Step 4: Update `setup_gridview` to use the right ListStore**

Find `fn setup_gridview` (around line 317). The current line that reads the ListStore is (around line 322):

```rust
        let artists = self.imp().library.upgrade().unwrap().artists();
```

Replace it with:

```rust
        let library = self.imp().library.upgrade().unwrap();
        let artists = match self.imp().kind.get() {
            ArtistKind::Artist => library.artists(),
            ArtistKind::AlbumArtist => library.album_artists(),
        };
```

The rest of the method is unchanged.

- [ ] **Step 5: Update `LazyInit::populate` to dispatch on kind**

Find the existing `impl LazyInit for ArtistView` block (around line 428). The current `populate` is:

```rust
impl LazyInit for ArtistView {
    fn populate(&self) {
        if let Some(library) = self.imp().library.upgrade() {
            if !self.imp().initializing.get() {
                self.imp().initializing.set(true);
                let stack = self.imp().stack.get();
                let this = self.clone();
                stack.show_spinner();
                glib::spawn_future_local(async move {
                    let _ = library.init_artists().await;
                    if library.artists().n_items() > 0 {
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

Replace it with:

```rust
impl LazyInit for ArtistView {
    fn populate(&self) {
        if let Some(library) = self.imp().library.upgrade() {
            if !self.imp().initializing.get() {
                self.imp().initializing.set(true);
                let stack = self.imp().stack.get();
                let this = self.clone();
                let kind = self.imp().kind.get();
                stack.show_spinner();
                glib::spawn_future_local(async move {
                    let n_items = match kind {
                        ArtistKind::Artist => {
                            let _ = library.init_artists().await;
                            library.artists().n_items()
                        }
                        ArtistKind::AlbumArtist => {
                            let _ = library.init_album_artists().await;
                            library.album_artists().n_items()
                        }
                    };
                    if n_items > 0 {
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

The kind is captured before spawning the local future so the async closure doesn't need to access `self.imp()` again.

- [ ] **Step 6: No changes needed in `on_artist_clicked`**

`on_artist_clicked` (around line 285) calls `content_view.bind(artist)`. The kind is already stored on `ArtistContentView::imp` (set by `setup` in Step 3 above), so the bind call doesn't need to thread kind explicitly. Leave this method unchanged.

- [ ] **Step 7: Verify the build**

```bash
fusermount -u /home/sam/Projects/euphonica/.flatpak-builder/rofiles/* 2>/dev/null
rm -rf /home/sam/Projects/euphonica/.flatpak-builder/rofiles
CARGO_BUILD_JOBS=4 flatpak-builder --force-clean --user --install --install-deps-from=flathub --repo=repo build-flatpak io.github.htkhiem.Euphonica-dev.json > /tmp/flatpak-task3-build.log 2>&1
```

**Wait for the build to complete.** Confirm exit code 0. The build should pass — the `content_view.setup` call still takes 3 arguments. The `init_album_artists`, `album_artists()`, and `get_album_artist_content` warnings from Task 2 are still present and will be consumed by Tasks 4 and 6.

- [ ] **Step 8: Commit**

```bash
git add src/library/artist_view.rs
git commit -m "Parameterize ArtistView with ArtistKind for dispatch"
```

---

## Task 4: `ArtistContentView` parameterization

**Files:**
- Modify: `src/library/artist_content_view.rs`
- Modify: `src/library/artist_view.rs` (single call-site update)

This task adds the `kind` field on `ArtistContentView::imp`, accepts it in `setup`, dispatches on it inside `bind()`'s async closure, and updates the one call site in `artist_view.rs` to pass the kind down.

- [ ] **Step 1: Import `ArtistKind`**

At the top of `src/library/artist_content_view.rs`, find the existing import block. The file currently has (around line 13):

```rust
use super::{AlbumCell, Library};
```

Change it to:

```rust
use super::{AlbumCell, ArtistKind, Library};
```

- [ ] **Step 2: Add the `kind` field to `imp::ArtistContentView`**

Find the `imp::ArtistContentView` struct (around line 29). Find the existing `pub selecting_all: Cell<bool>` field (around line 97). Right after it (still inside the struct), add:

```rust
        pub kind: Cell<ArtistKind>,
```

The default value is `ArtistKind::Artist` (via the `Default` impl from Task 1).

- [ ] **Step 3: Add the `kind` parameter to `setup`**

Find the existing `setup` method (around line 670):

```rust
    pub fn setup(&self, library: &Library, cache: Rc<Cache>, window: &EuphonicaWindow) {
        self.imp()
            .cache
            .set(cache)
            .expect("Could not register artist content view with cache controller");
        self.imp().library.set(Some(library));
        self.imp().window.set(Some(window));

        self.setup_info_box();
        self.setup_song_subview();
        ...
```

Replace the signature line with:

```rust
    pub fn setup(
        &self,
        library: &Library,
        cache: Rc<Cache>,
        window: &EuphonicaWindow,
        kind: ArtistKind,
    ) {
```

And add `self.imp().kind.set(kind);` as the very first line of the method body (before the existing `self.imp().cache.set(...)` call):

```rust
    pub fn setup(
        &self,
        library: &Library,
        cache: Rc<Cache>,
        window: &EuphonicaWindow,
        kind: ArtistKind,
    ) {
        self.imp().kind.set(kind);
        self.imp()
            .cache
            .set(cache)
            .expect("Could not register artist content view with cache controller");
        self.imp().library.set(Some(library));
        self.imp().window.set(Some(window));

        self.setup_info_box();
        self.setup_song_subview();
        // (rest unchanged)
```

- [ ] **Step 4: Dispatch on kind inside `bind()`**

Find the `bind` method (around line 736). Inside it, locate the async closure that calls `library.get_artist_content` (around line 778):

```rust
                let _ = library
                    .get_artist_content(
                        &artist,
                        |album| {
                            album_list.append(&album);
                        },
                        |songs| {
                            song_list.extend_from_slice(&songs);
                        },
                    )
                    .await;
```

Capture the kind BEFORE the `glib::spawn_future_local` block (so the async closure has it). Find the line around `let library = ...` near the top of the spawn closure setup; just before `glib::spawn_future_local(clone!(...))`, capture:

```rust
        let kind = self.imp().kind.get();
```

Inside the spawn closure, replace the single `let _ = library.get_artist_content(...)` call with a kind-dispatched call:

```rust
                let _ = match kind {
                    ArtistKind::Artist => {
                        library
                            .get_artist_content(
                                &artist,
                                |album| {
                                    album_list.append(&album);
                                },
                                |songs| {
                                    song_list.extend_from_slice(&songs);
                                },
                            )
                            .await
                    }
                    ArtistKind::AlbumArtist => {
                        library
                            .get_album_artist_content(
                                &artist,
                                |album| {
                                    album_list.append(&album);
                                },
                                |songs| {
                                    song_list.extend_from_slice(&songs);
                                },
                            )
                            .await
                    }
                };
```

The two arms differ only in which Library method they call. The album/song responder closures and the `&artist` are identical.

The `kind` variable is captured into the spawn closure via the existing `clone!(...)` macro implicitly (since it's a `Copy` type and is used directly, no `#[strong] kind` annotation needed — the compiler will move-capture it).

- [ ] **Step 5: Update the `content_view.setup` call in `artist_view.rs` to pass `kind`**

In `src/library/artist_view.rs`, find the `setup` method (around line 135 — modified by Task 3). It currently has:

```rust
        let content_view = self.imp().content_view.get();
        content_view.setup(library, cache, window);
```

Replace the second line with:

```rust
        content_view.setup(library, cache, window, self.imp().kind.get());
```

(`self.imp().kind.get()` reads the kind that was just set at the top of `setup` from the parameter. No need for the `kind` local variable since it's read once.)

- [ ] **Step 6: Verify the build**

```bash
fusermount -u /home/sam/Projects/euphonica/.flatpak-builder/rofiles/* 2>/dev/null
rm -rf /home/sam/Projects/euphonica/.flatpak-builder/rofiles
CARGO_BUILD_JOBS=4 flatpak-builder --force-clean --user --install --install-deps-from=flathub --repo=repo build-flatpak io.github.htkhiem.Euphonica-dev.json > /tmp/flatpak-task4-build.log 2>&1
```

**Wait for the build to complete.** Confirm exit code 0 and the log ends with `Pruning cache`.

The build should be green: `ArtistContentView::setup` now takes the kind, `artist_view.rs` passes it down, and `get_album_artist_content` (added in Task 2) is now consumed via the `bind()` dispatch.

The `init_album_artists` and `album_artists()` warnings are still there (consumed in Task 6).

- [ ] **Step 7: Commit**

```bash
git add src/library/artist_content_view.rs src/library/artist_view.rs
git commit -m "Parameterize ArtistContentView with ArtistKind for fetch dispatch"
```

---

## Task 5: Sidebar entry — button + handler + set_view arm

**Files:**
- Modify: `src/gtk/sidebar.ui`
- Modify: `src/sidebar/sidebar.rs`

This task adds the visible sidebar entry. It activates a stack child named `"album_artists"` which doesn't yet exist (Task 6 adds it), so clicking the entry now would print a runtime warning. That's expected.

- [ ] **Step 1: Add the sidebar button to `src/gtk/sidebar.ui`**

In `src/gtk/sidebar.ui`, find the existing `<object class="EuphonicaSidebarButton" id="artists_btn">` block (around line 31). Just after the `</child>` that wraps it (and before the `<child>` containing `genres_btn`, which sits between artists_btn and folders_btn), insert:

```xml
						<child>
							<object class="EuphonicaSidebarButton" id="album_artists_btn">
								<property name="group">recent_btn</property>
								<property name="label" translatable="true">Album Artists</property>
								<property name="icon_name">music-artist-symbolic</property>
							</object>
						</child>
```

The `group="recent_btn"` ties the toggle to the same group as the other top-level buttons.

- [ ] **Step 2: Add the `album_artists_btn` template child in `src/sidebar/sidebar.rs`**

In `src/sidebar/sidebar.rs`, find the `pub struct Sidebar { ... }` (around line 19). After the existing `pub artists_btn: TemplateChild<SidebarButton>` field, insert:

```rust
        #[template_child]
        pub album_artists_btn: TemplateChild<SidebarButton>,
```

- [ ] **Step 3: Add the `connect_toggled` handler in `setup`**

In the same file, find the `setup` method's existing `artists_btn.connect_toggled(...)` block (around line 150). Just below it (and before the existing `genres_btn` handler if one exists, or wherever the chain of buttons continues), add:

```rust
        self.imp().album_artists_btn.connect_toggled(clone!(
            #[weak]
            stack,
            move |btn| {
                if btn.is_active() {
                    stack.set_visible_child_name("album_artists");
                }
            }
        ));
```

- [ ] **Step 4: Add `album_artists_btn` to the show-sidebar click loop**

Still in `setup`, find the loop that wires up `show-sidebar` clicks via the array literal of buttons (around line 407 — same place where the previous features added `genres_btn`, etc.). Add `&self.imp().album_artists_btn.get(),` to the array, between the existing entries for `artists_btn` and `genres_btn` (or wherever maintains visual ordering matching the sidebar UI).

- [ ] **Step 5: Add the `"album_artists"` arm to `set_view`**

Find the `pub fn set_view(&self, view_name: &str)` method (around line 444). Add a new arm between `"artists"` and `"queue"`:

```rust
    pub fn set_view(&self, view_name: &str) {
        // TODO: something less dumb than this
        match view_name {
            "albums" => self.imp().albums_btn.set_active(true),
            "artists" => self.imp().artists_btn.set_active(true),
            "album_artists" => self.imp().album_artists_btn.set_active(true),
            "genres" => self.imp().genres_btn.set_active(true),
            "queue" => self.imp().queue_btn.set_active(true),
            "playlists" => self.imp().playlists_btn.set_active(true),
            _ => unimplemented!(),
        };
    }
```

(The `"genres"` arm exists from the earlier feature.)

- [ ] **Step 6: Verify the build**

```bash
fusermount -u /home/sam/Projects/euphonica/.flatpak-builder/rofiles/* 2>/dev/null
rm -rf /home/sam/Projects/euphonica/.flatpak-builder/rofiles
CARGO_BUILD_JOBS=4 flatpak-builder --force-clean --user --install --install-deps-from=flathub --repo=repo build-flatpak io.github.htkhiem.Euphonica-dev.json > /tmp/flatpak-task5-build.log 2>&1
```

**Wait for the build to complete.** Confirm exit code 0.

- [ ] **Step 7: Commit**

```bash
git add src/gtk/sidebar.ui src/sidebar/sidebar.rs
git commit -m "Wire Album Artists sidebar entry"
```

---

## Task 6: Window integration — stack page, setup, signal handler, populate dispatch

**Files:**
- Modify: `src/window.ui`
- Modify: `src/window.rs`

This task wires the second `ArtistView` instance into the window stack. It includes the `maybe_populate_visible` arm — **don't repeat the bug from the genres-view rollout where this was missed.**

- [ ] **Step 1: Add the new stack page in `src/window.ui`**

Open `src/window.ui`. Find the existing `<object class="EuphonicaArtistView" id="artist_view">` stack page (around line 253) and its enclosing `<object class="GtkStackPage">`. Locate where it ends (the `</child>` after `</object>` after `</property>` after `</object>` after `</child>`). Just AFTER that closing `</child>` (and BEFORE the next `<child>` block, which holds another stack page), insert:

```xml
                        <child>
                          <object class="GtkStackPage">
                            <property name="title" translatable="true">Album Artists</property>
                            <property name="name">album_artists</property>
                            <property name="child">
                              <object class="EuphonicaArtistView" id="album_artist_view">
                              </object>
                            </property>
                          </object>
                        </child>
```

- [ ] **Step 2: Add the `collapsed` setter in the breakpoint block**

Near the top of `src/window.ui` (around line 28-29), the existing `setter` lines bind `collapsed` for `album_view` and `artist_view` (and likely `genre_view` from the earlier feature). Add (after the `artist_view` setter):

```xml
        <setter object="album_artist_view" property="collapsed">true</setter>
```

- [ ] **Step 3: Add the `album_artist_view` template child in `src/window.rs`**

In `src/window.rs`, find the existing `pub artist_view: TemplateChild<ArtistView>,` line (around line 169). Just after it, add:

```rust
        #[template_child]
        pub album_artist_view: TemplateChild<ArtistView>,
```

- [ ] **Step 4: Import `ArtistKind`**

In the existing `library::{...}` import block (around line 25-28), add `ArtistKind`. Final block should look approximately like:

```rust
    library::{
        AlbumView, ArtistContentView, ArtistKind, ArtistView, DynamicPlaylistView, FolderView,
        GenreContentView, GenreView, PlaylistView, RecentView,
    },
```

(Place `ArtistKind` alphabetically between `ArtistContentView` and `ArtistView`.)

- [ ] **Step 5: Add `album_artist_view` to the show-sidebar Widget array**

Find the array literal (around line 430) that lists `recent_view`, `album_view`, etc. Add `self.album_artist_view.upcast_ref::<gtk::Widget>(),` after the `artist_view` entry:

```rust
            [
                self.recent_view.upcast_ref::<gtk::Widget>(),
                self.album_view.upcast_ref::<gtk::Widget>(),
                self.artist_view.upcast_ref::<gtk::Widget>(),
                self.album_artist_view.upcast_ref::<gtk::Widget>(),
                self.folder_view.upcast_ref::<gtk::Widget>(),
                // ...
            ]
```

(Existing entries — those past `folder_view` — remain unchanged.)

- [ ] **Step 6: Update existing `artist_view.setup` and add `album_artist_view.setup` calls**

Find the existing `win.imp().artist_view.setup(...)` call (around line 953). The current call is:

```rust
        win.imp()
            .artist_view
            .setup(app.get_library(), app.get_cache(), &win);
```

Replace it with:

```rust
        win.imp()
            .artist_view
            .setup(app.get_library(), app.get_cache(), &win, ArtistKind::Artist);
        win.imp()
            .album_artist_view
            .setup(app.get_library(), app.get_cache(), &win, ArtistKind::AlbumArtist);
```

- [ ] **Step 7: Wire the `album-clicked` signal handler for `album_artist_view`**

Find the existing `win.imp().artist_view.get_content_view().connect_closure("album-clicked", ...)` block (around line 1042 — the one that routes to `goto_album`). Just AFTER that block, add a parallel block for `album_artist_view`:

```rust
        win.imp().album_artist_view.get_content_view().connect_closure(
            "album-clicked",
            false,
            closure_local!(
                #[watch(rename_to = this)]
                win,
                move |_: ArtistContentView, album: Album| {
                    this.goto_album(&album);
                }
            ),
        );
```

- [ ] **Step 8: Add the `"album_artists"` arm to `maybe_populate_visible`**

Find the existing `maybe_populate_visible` method (around line 1208). The match arms currently include `"recent"`, `"albums"`, `"artists"`, `"genres"`, `"folders"`, `"queue"`. Add an `"album_artists"` arm after `"artists"`:

```rust
                    "artists" => {
                        imp.artist_view.populate();
                    }
                    "album_artists" => {
                        imp.album_artist_view.populate();
                    }
                    "genres" => {
                        imp.genre_view.populate();
                    }
```

**This is the most important step in this task.** Without it, navigating to the Album Artists stack page never calls `populate()` and the grid stays empty — same bug as the original genres-view rollout.

- [ ] **Step 9: Verify the build**

```bash
fusermount -u /home/sam/Projects/euphonica/.flatpak-builder/rofiles/* 2>/dev/null
rm -rf /home/sam/Projects/euphonica/.flatpak-builder/rofiles
CARGO_BUILD_JOBS=4 flatpak-builder --force-clean --user --install --install-deps-from=flathub --repo=repo build-flatpak io.github.htkhiem.Euphonica-dev.json > /tmp/flatpak-task6-build.log 2>&1
```

**Wait for the build to complete.** Confirm exit code 0. The previous unused warnings (`init_album_artists`, `album_artists`, `get_album_artist_content`, `ArtistKind`) should now all be consumed.

- [ ] **Step 10: Commit**

```bash
git add src/window.ui src/window.rs
git commit -m "Register Album Artists view in window stack and dispatch"
```

---

## Task 7: Manual smoke test (handed back to user)

This task is for the user to run, not a subagent. There is no code change.

Pre-requisite: an MPD library where some albums have AlbumArtist tags. Optionally, a library where some Artist tags differ from their album's AlbumArtist (so the difference between the two views is visible).

- [ ] **Step 1: Run the dev build**

```bash
flatpak run --branch=master io.github.htkhiem.Euphonica
```

- [ ] **Step 2: Verify the new sidebar entry**

The sidebar should show, top-to-bottom: Recent, Albums, Artists, **Album Artists**, Genres, Folders, Dynamic Playlists, Saved Playlists, Queue. The new entry uses the same icon as Artists (a music-artist symbolic).

- [ ] **Step 3: Click Album Artists**

The grid should populate with artists derived from the AlbumArtist tag.

- [ ] **Step 4: Compare to Artists**

Click Artists in the sidebar. Note which artists appear there but NOT in Album Artists (typically: featured / guest artists who appear in track-level Artist tags but never as album-level AlbumArtists). Click back to Album Artists. The grids should differ if your library has any such cases.

- [ ] **Step 5: Click an album-artist tile**

The ArtistContentView opens. The Discography tab shows the artist's albums (where they're the AlbumArtist). The All Songs tab shows every track on those albums.

- [ ] **Step 6: Click an album in the discography**

Sidebar switches to Albums; AlbumContentView opens for the picked album. (Existing `goto_album` flow, unchanged.)

- [ ] **Step 7: Refresh path**

Press F5 (or disconnect/reconnect MPD). Both Artists and Album Artists views reload independently.

- [ ] **Step 8: Sidebar toggle group**

Click between sidebar entries — only one is highlighted at a time. The show-sidebar action collapses the sidebar correctly when activated from Album Artists.

- [ ] **Step 9: `maybe_populate_visible` regression check**

Restart the app fresh. As your first action, click directly on Album Artists in the sidebar. The grid populates. (This validates Task 6 Step 8 — without it, the grid would stay empty until you navigate elsewhere and back.)

- [ ] **Step 10: Done**

If all checks pass, the feature is complete. Branch `feat/album-artists-sidebar` is ready to merge.

---

## Self-review checklist

- **Spec coverage:**
  - New sidebar entry between Artists and Genres → Task 5.
  - Reuses ArtistContentView with AlbumArtist scope → Tasks 4 + 6.
  - Membership rule (any album where this is AlbumArtist) → Task 2's `get_album_artist_content`.
  - Two parallel ListStores in Library → Task 2.
  - Drop `use_album_artist` flag from `init_artists` → Task 2.
  - Add `init_album_artists` and `album_artists()` → Task 2.
  - `ArtistKind` enum → Task 1.
  - ArtistView parameterization → Task 3.
  - ArtistContentView parameterization → Task 4.
  - State persistence shared (no schema changes) → no task needed (intentional).
  - `maybe_populate_visible` arm → Task 6 Step 8.
  - `set_view` arm → Task 5 Step 5.
- **Type / method names recurring across tasks:** `ArtistKind` (Task 1 → Tasks 2/3/4/6), `init_album_artists` (Task 2 → Task 3), `album_artists()` (Task 2 → Task 3), `get_album_artist_content` (Task 2 → Task 4), `kind: Cell<ArtistKind>` field (Tasks 3 + 4). All consistent.
- **No placeholders** ("TBD", "implement later", "similar to Task N") — verified.
- **Code shown for every code step** — verified.
- **Each task is buildable on its own:** Tasks 3 and 4 were originally drafted with a non-buildable intermediate state at Task 3; revised so Task 3 keeps `content_view.setup(...)` at 3 arguments and Task 4 updates both the `ArtistContentView::setup` signature and the call site. Each task now ends with a green build.
