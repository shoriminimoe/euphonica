# Genre View Albums/Artists Toggle Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a linked Albums/Artists toggle to `GenreContentView` so users can switch between an album grid and an artist grid scoped to the current genre, with the last-selected toggle persisting in-memory across genre navigations.

**Architecture:** A new `Library::get_artists_by_genre` method mirrors the existing `get_albums_by_genre` but buckets songs by `artist.get_comp_id()`. The `genre-content-view.ui` template is restructured to host a `GtkStack` with two named pages (`albums` / `artists`), each with its own `ContentStack` + `GtkGridView`. A linked toggle pair in a sub-toolbar row drives the active stack page via property binding. `GenreContentView` lazy-populates the artists subview on first toggle, persists the last-selected state in a `Cell<bool>`, and emits a new `artist-clicked` signal that the window handler routes through `goto_artist`.

**Tech Stack:** Rust 2024, GTK4 + libadwaita via gtk-rs, `rustc-hash::FxHashSet`. Build via Meson driving Cargo through Flatpak (Ubuntu 24.04 host gtk4 is too old for native).

**Spec reference:** `docs/superpowers/specs/2026-05-05-genre-view-toggle-design.md`.

---

## Pre-flight notes

This codebase has **no automated test harness**. Verification is by Flatpak build + manual smoke test (Task 4 hands back to the user). Each implementation task should end with a clean Flatpak build.

**Build commands:**

```bash
fusermount -u /home/sam/Projects/euphonica/.flatpak-builder/rofiles/* 2>/dev/null
rm -rf /home/sam/Projects/euphonica/.flatpak-builder/rofiles
CARGO_BUILD_JOBS=4 flatpak-builder --force-clean --user --install --install-deps-from=flathub --repo=repo build-flatpak io.github.htkhiem.Euphonica-dev.json > /tmp/flatpak-task<N>-build.log 2>&1
```

The first cleanup line handles a stale `rofiles-fuse` mount that builds sometimes leave behind. `CARGO_BUILD_JOBS=4` prevents OOM on a 15GB-RAM host (default parallelism has been killed mid-build before). The build takes ~3 minutes when cargo deps are cached.

**Branch:** Work on `feat/genre-view-toggle` (already created from `feat/album-content-genres`). Do NOT switch branches.

**Commit cadence:** Short imperative title per task, no Conventional-Commits prefix. Optional `Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>` trailer.

---

## File map

**New files:** none.

**Modified files:**

- `src/library/controller.rs` — new `pub async fn get_artists_by_genre` method, immediately after `get_albums_by_genre`.
- `src/gtk/library/genre-content-view.ui` — full-template refactor: header bar carries only the genre name; new sub-toolbar row holds the linked Albums/Artists toggle plus the count label; content becomes a `GtkStack` with two named pages each containing a `ContentStack` + `GtkGridView`.
- `src/library/genre_content_view.rs` — new template children for the toggle/stack/artists subview, `artist_list: gio::ListStore<Artist>` field, `artists_initialized: Cell<bool>`, `last_subview_albums: Cell<bool>` (persists across `unbind()`), updated `setup` (artist factory, toggle handler, count-label binding), updated `bind` (restore toggle from cell), updated `unbind` (clear artists list + initialized flag, do NOT reset persistence cell), new `artist-clicked` signal.
- `src/window.rs` — add `artist-clicked` signal handler on `genre_view.get_content_view()` that routes to `goto_artist`.

---

## Task 1: `Library::get_artists_by_genre`

**Files:**
- Modify: `src/library/controller.rs`

This task adds the data-fetch method. It runs independently of the UI changes — the new method has no callers yet (Task 2 will use it). Because Rust treats unused `pub` methods as warnings (not errors), the build remains clean.

- [ ] **Step 1: Add the method**

In `src/library/controller.rs`, find the existing `pub async fn get_albums_by_genre` method (around line 725). Right after that method's closing `}` (around line 755), inside the `impl Library` block, add:

```rust
    /// Find all artists whose songs include the given genre after splitting.
    /// Same algorithmic shape as `get_albums_by_genre`: server-side substring
    /// filter narrows the candidate set, client-side verification drops
    /// substring false positives, and each surviving song's `artists` Vec
    /// contributes — deduped by `Artist::get_comp_id()`.
    pub async fn get_artists_by_genre<FA>(
        &self,
        genre: String,
        mut respond_artist: FA,
    ) -> ClientResult<()>
    where
        FA: FnMut(Artist),
    {
        let mut song_query = Query::new();
        song_query.and_with_op(
            Term::Tag(tags::GENRE.into()),
            QueryOperation::Contains,
            genre.clone(),
        );

        let mut visited_artists = FxHashSet::default();
        self.client()
            .get_song_infos_by_query(song_query, true, &mut |batch| {
                for song in batch.into_iter() {
                    if !song.genres.iter().any(|g| g == &genre) {
                        continue;
                    }
                    for info in song.artists.iter() {
                        if visited_artists.insert(info.get_comp_id().to_owned()) {
                            respond_artist(Artist::from(info.clone()));
                        }
                    }
                }
            })
            .await
    }
```

The conversion `Artist::from(info.clone())` works because `common::Artist` has a `From<ArtistInfo>` impl (used elsewhere in this file). No new imports needed — `Artist`, `Query`, `Term`, `QueryOperation`, `FxHashSet`, `tags` are already in scope.

- [ ] **Step 2: Verify the build**

```bash
fusermount -u /home/sam/Projects/euphonica/.flatpak-builder/rofiles/* 2>/dev/null
rm -rf /home/sam/Projects/euphonica/.flatpak-builder/rofiles
CARGO_BUILD_JOBS=4 flatpak-builder --force-clean --user --install --install-deps-from=flathub --repo=repo build-flatpak io.github.htkhiem.Euphonica-dev.json > /tmp/flatpak-task1-build.log 2>&1
```

**Wait for the build to complete** (~3 min). Confirm exit code 0 and the log ends with `Pruning cache`. Expected new warning: `get_artists_by_genre` is unused — that's fine; Task 2 will consume it.

- [ ] **Step 3: Commit**

```bash
git add src/library/controller.rs
git commit -m "Add Library::get_artists_by_genre with substring filter + verification"
```

---

## Task 2: GenreContentView UI + Rust refactor

**Files:**
- Modify: `src/gtk/library/genre-content-view.ui`
- Modify: `src/library/genre_content_view.rs`

This is the bulk of the work. The .ui and .rs files have to change together — adding template children in Rust without matching IDs in the template causes runtime crashes; restructuring the template without updating the Rust references breaks the build. Treat as a single atomic change.

- [ ] **Step 1: Replace `src/gtk/library/genre-content-view.ui` entirely**

Overwrite the file with this exact content:

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
							<object class="GtkLabel" id="genre_name">
								<property name="ellipsize">3</property>
								<style>
									<class name="title-2"/>
								</style>
							</object>
						</child>
					</object>
				</child>
				<child type="top">
					<object class="GtkBox">
						<property name="halign">center</property>
						<property name="spacing">12</property>
						<property name="margin-start">12</property>
						<property name="margin-end">12</property>
						<property name="margin-top">6</property>
						<property name="margin-bottom">6</property>
						<child>
							<object class="GtkBox">
								<style>
									<class name="linked"/>
								</style>
								<child>
									<object class="GtkToggleButton" id="albums_btn">
										<property name="active">true</property>
										<child>
											<object class="GtkBox">
												<property name="spacing">6</property>
												<child>
													<object class="GtkImage">
														<property name="icon-name">library-music-symbolic</property>
													</object>
												</child>
												<child>
													<object class="GtkLabel">
														<property name="label" translatable="true">Albums</property>
													</object>
												</child>
											</object>
										</child>
									</object>
								</child>
								<child>
									<object class="GtkToggleButton" id="artists_btn">
										<property name="group">albums_btn</property>
										<child>
											<object class="GtkBox">
												<property name="spacing">6</property>
												<child>
													<object class="GtkImage">
														<property name="icon-name">music-artist-symbolic</property>
													</object>
												</child>
												<child>
													<object class="GtkLabel">
														<property name="label" translatable="true">Artists</property>
													</object>
												</child>
											</object>
										</child>
									</object>
								</child>
							</object>
						</child>
						<child>
							<object class="GtkLabel" id="count">
								<style>
									<class name="dim-label"/>
									<class name="caption"/>
								</style>
							</object>
						</child>
					</object>
				</child>
				<property name="content">
					<object class="GtkStack" id="subview_stack">
						<property name="transition-type">crossfade</property>
						<child>
							<object class="GtkStackPage">
								<property name="name">albums</property>
								<property name="child">
									<object class="EuphonicaContentStack" id="albums_stack">
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
						<child>
							<object class="GtkStackPage">
								<property name="name">artists</property>
								<property name="child">
									<object class="EuphonicaContentStack" id="artists_stack">
										<property name="placeholder">
											<object class="AdwStatusPage">
												<property name="title" translatable="true">No Artists</property>
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
													<object class="GtkGridView" id="artist_grid">
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
					</object>
				</property>
			</object>
		</child>
	</template>
</interface>
```

Key changes from the existing template:
- `album_count` label (formerly in the header bar with `genre_name`) is gone.
- Header bar's start widget is now just the `genre_name` label (the wrapping `GtkBox` is dropped).
- New `<child type="top">` after the header bar holds the linked toggle pair plus a `count` label, all in a centered `GtkBox`.
- The single `EuphonicaContentStack id="stack"` is replaced by a `GtkStack id="subview_stack"` with two named pages. The existing album grid is wrapped in the `albums` page (now using `albums_stack` instead of `stack`); a new mirror structure for `artists` page hosts `artist_grid` inside `artists_stack`.

- [ ] **Step 2: Replace `src/library/genre_content_view.rs` entirely**

Overwrite the file with this exact content:

```rust
use adw::subclass::prelude::*;
use derivative::Derivative;
use glib::{clone, subclass::Signal, WeakRef};
use gtk::{
    gio, glib, prelude::*, CompositeTemplate, ListItem, SignalListItemFactory, SingleSelection,
};
use std::{cell::{Cell, RefCell}, rc::Rc, sync::OnceLock};

use super::{AlbumCell, ArtistCell, Library};
use crate::{
    cache::Cache,
    common::{Album, Artist, ContentStack, Genre},
};

mod imp {
    use super::*;

    #[derive(Debug, CompositeTemplate, Derivative)]
    #[derivative(Default)]
    #[template(resource = "/io/github/htkhiem/Euphonica/gtk/library/genre-content-view.ui")]
    pub struct GenreContentView {
        #[template_child]
        pub genre_name: TemplateChild<gtk::Label>,
        #[template_child]
        pub count: TemplateChild<gtk::Label>,
        #[template_child]
        pub albums_btn: TemplateChild<gtk::ToggleButton>,
        #[template_child]
        pub artists_btn: TemplateChild<gtk::ToggleButton>,
        #[template_child]
        pub subview_stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub albums_stack: TemplateChild<ContentStack>,
        #[template_child]
        pub album_grid: TemplateChild<gtk::GridView>,
        #[template_child]
        pub artists_stack: TemplateChild<ContentStack>,
        #[template_child]
        pub artist_grid: TemplateChild<gtk::GridView>,

        #[derivative(Default(value = "gio::ListStore::new::<Album>()"))]
        pub album_list: gio::ListStore,
        #[derivative(Default(value = "gio::ListStore::new::<Artist>()"))]
        pub artist_list: gio::ListStore,

        pub library: WeakRef<Library>,
        pub cache: RefCell<Option<Rc<Cache>>>,
        pub current_genre: RefCell<Option<Genre>>,

        // Track whether the artists subview has been populated for the current
        // genre. Resets in `unbind()`. Albums are always populated on bind, so
        // there's no equivalent flag for them.
        pub artists_initialized: Cell<bool>,

        // Persists the user's last-selected toggle across `unbind()`/`bind()`
        // within a single session. NOT reset by `unbind()`. Default is true
        // (Albums) on first construction.
        #[derivative(Default(value = "Cell::new(true)"))]
        pub last_subview_albums: Cell<bool>,
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
            self.albums_stack.show_placeholder();
            self.artists_stack.show_placeholder();
        }

        fn signals() -> &'static [Signal] {
            static SIGNALS: OnceLock<Vec<Signal>> = OnceLock::new();
            SIGNALS.get_or_init(|| {
                vec![
                    Signal::builder("album-clicked")
                        .param_types([Album::static_type()])
                        .build(),
                    Signal::builder("artist-clicked")
                        .param_types([Artist::static_type()])
                        .build(),
                ]
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

        self.setup_album_grid(cache.clone());
        self.setup_artist_grid(cache.clone());
        self.setup_toggle();
        self.setup_count_label();
    }

    fn setup_album_grid(&self, cache: Rc<Cache>) {
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

    fn setup_artist_grid(&self, cache: Rc<Cache>) {
        let sel_model = SingleSelection::new(Some(self.imp().artist_list.clone()));
        self.imp().artist_grid.set_model(Some(&sel_model));

        let factory = SignalListItemFactory::new();
        factory.connect_setup(clone!(
            #[weak]
            cache,
            move |_, list_item| {
                let item = list_item
                    .downcast_ref::<ListItem>()
                    .expect("Needs to be ListItem");
                // `false`: don't immediately fetch avatars externally — same
                // choice as `ArtistView::setup_gridview` makes.
                let cell = ArtistCell::new(item, cache, false);
                item.set_child(Some(&cell));
            }
        ));
        factory.connect_bind(move |_, list_item| {
            let item = list_item
                .downcast_ref::<ListItem>()
                .expect("Needs to be ListItem");
            let artist = item
                .item()
                .and_downcast::<Artist>()
                .expect("The item has to be a common::Artist.");
            let cell = item
                .child()
                .and_downcast::<ArtistCell>()
                .expect("The child has to be an ArtistCell.");
            cell.bind(&artist);
        });
        factory.connect_unbind(move |_, list_item| {
            let item = list_item
                .downcast_ref::<ListItem>()
                .expect("Needs to be ListItem");
            let cell = item
                .child()
                .and_downcast::<ArtistCell>()
                .expect("The child has to be an ArtistCell.");
            cell.unbind();
        });
        self.imp().artist_grid.set_factory(Some(&factory));

        self.imp().artist_grid.connect_activate(clone!(
            #[weak(rename_to = this)]
            self,
            move |grid, position| {
                let model = grid.model().expect("The model has to exist.");
                let artist = model
                    .item(position)
                    .and_downcast::<Artist>()
                    .expect("The item has to be a common::Artist.");
                this.emit_by_name::<()>("artist-clicked", &[&artist.to_value()]);
            }
        ));
    }

    fn setup_toggle(&self) {
        // Bind toggle button state to stack visible-child-name.
        self.imp()
            .albums_btn
            .bind_property("active", &self.imp().subview_stack.get(), "visible-child-name")
            .transform_to(|_, active: bool| {
                Some(if active { "albums" } else { "artists" })
            })
            .sync_create()
            .build();

        // On every toggle: persist the new state and lazy-populate artists.
        self.imp().albums_btn.connect_toggled(clone!(
            #[weak(rename_to = this)]
            self,
            move |btn| {
                let albums_active = btn.is_active();
                this.imp().last_subview_albums.set(albums_active);
                if !albums_active && !this.imp().artists_initialized.get() {
                    this.populate_artists();
                }
            }
        ));
    }

    fn setup_count_label(&self) {
        // The count label reads from whichever ListStore corresponds to the
        // active subview. Update on either ListStore's n-items change AND on
        // toggle change.
        self.imp().album_list.connect_items_changed(clone!(
            #[weak(rename_to = this)]
            self,
            move |_, _, _, _| this.update_count_label()
        ));
        self.imp().artist_list.connect_items_changed(clone!(
            #[weak(rename_to = this)]
            self,
            move |_, _, _, _| this.update_count_label()
        ));
        self.imp().albums_btn.connect_toggled(clone!(
            #[weak(rename_to = this)]
            self,
            move |_| this.update_count_label()
        ));
    }

    fn update_count_label(&self) {
        let albums_active = self.imp().albums_btn.is_active();
        let n = if albums_active {
            self.imp().album_list.n_items()
        } else {
            self.imp().artist_list.n_items()
        };
        let label = match (albums_active, n) {
            (true, 1) => "1 album".to_string(),
            (true, _) => format!("{n} albums"),
            (false, 1) => "1 artist".to_string(),
            (false, _) => format!("{n} artists"),
        };
        self.imp().count.set_label(&label);
    }

    pub fn bind(&self, genre: &Genre) {
        self.imp().genre_name.set_label(genre.get_name());
        self.imp().current_genre.replace(Some(genre.clone()));

        // Restore the persisted toggle state. set_active() fires the toggled
        // signal only when the state actually changes, so we explicitly call
        // populate_artists() afterward if the persisted state is artists AND
        // we changed nothing (the connect_toggled handler wouldn't have fired).
        let want_albums = self.imp().last_subview_albums.get();
        let was_albums = self.imp().albums_btn.is_active();
        self.imp().albums_btn.set_active(want_albums);

        // Always populate albums on bind (cheap; the album subview is the default).
        self.populate_albums();

        // If the toggle didn't change AND artists is the active subview, the
        // toggled handler didn't fire but we still need artists populated.
        if want_albums == was_albums && !want_albums {
            self.populate_artists();
        }

        self.update_count_label();
    }

    pub fn unbind(&self) {
        self.imp().album_list.remove_all();
        self.imp().artist_list.remove_all();
        self.imp().artists_initialized.set(false);
        self.imp().current_genre.replace(None);
        self.imp().albums_stack.show_placeholder();
        self.imp().artists_stack.show_placeholder();
        // Note: last_subview_albums is intentionally NOT reset — it must
        // persist across binds for in-session toggle persistence.
    }

    fn populate_albums(&self) {
        self.imp().album_list.remove_all();
        let Some(library) = self.imp().library.upgrade() else {
            return;
        };
        let Some(genre) = self.imp().current_genre.borrow().clone() else {
            return;
        };
        let model = self.imp().album_list.clone();
        let stack = self.imp().albums_stack.get();
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

    fn populate_artists(&self) {
        if self.imp().artists_initialized.get() {
            return;
        }
        self.imp().artists_initialized.set(true);
        self.imp().artist_list.remove_all();
        let Some(library) = self.imp().library.upgrade() else {
            return;
        };
        let Some(genre) = self.imp().current_genre.borrow().clone() else {
            return;
        };
        let model = self.imp().artist_list.clone();
        let stack = self.imp().artists_stack.get();
        stack.show_spinner();
        let name = genre.get_name().to_owned();
        glib::spawn_future_local(async move {
            let _ = library
                .get_artists_by_genre(name, |artist| {
                    model.append(&artist);
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

Notable points in the code above:
- `ArtistCell` is imported via `super::ArtistCell`. That re-export already exists in `src/library/mod.rs` from the genres-view feature work.
- The artist cell factory mirrors `ArtistView::setup_gridview`'s factory verbatim (avatar binding, cache subscription via `ArtistCell::bind/unbind`).
- `last_subview_albums` defaults to `true` via `#[derivative(Default(value = "Cell::new(true)"))]` so the very first `bind()` after construction shows Albums.
- The `bind()` logic handles a subtle case: if `set_active()` is called with the same value as before, the `connect_toggled` handler does NOT fire, so we manually call `populate_artists()` to ensure correctness. The `populate_artists()` method has its own `artists_initialized` guard so duplicate calls are safe.
- `update_count_label` is called from three places: each ListStore's `items-changed`, the toggle button's `toggled`, and explicitly at the end of `bind()` to reset for the new genre.

- [ ] **Step 3: Verify the build**

```bash
fusermount -u /home/sam/Projects/euphonica/.flatpak-builder/rofiles/* 2>/dev/null
rm -rf /home/sam/Projects/euphonica/.flatpak-builder/rofiles
CARGO_BUILD_JOBS=4 flatpak-builder --force-clean --user --install --install-deps-from=flathub --repo=repo build-flatpak io.github.htkhiem.Euphonica-dev.json > /tmp/flatpak-task2-build.log 2>&1
```

**Wait for the build to complete.** Confirm exit code 0. The unused warning for `get_artists_by_genre` from Task 1 should now be gone.

- [ ] **Step 4: Commit**

```bash
git add src/gtk/library/genre-content-view.ui src/library/genre_content_view.rs
git commit -m "Add Albums/Artists toggle to GenreContentView with lazy artist fetch"
```

---

## Task 3: Window-level `artist-clicked` handler

**Files:**
- Modify: `src/window.rs`

The `GenreContentView::artist-clicked` signal added in Task 2 needs a window-level handler that calls `goto_artist`. This mirrors the existing `album-clicked` → `goto_album` handler exactly.

- [ ] **Step 1: Add the new signal handler**

In `src/window.rs`, find the existing handler block:

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

Just AFTER that block (typically around line 1065 — adjacent to where it lives), add an analogous block for artists:

```rust
        win.imp().genre_view.get_content_view().connect_closure(
            "artist-clicked",
            false,
            closure_local!(
                #[watch(rename_to = this)]
                win,
                move |_: GenreContentView, artist: Artist| {
                    this.goto_artist(&artist);
                }
            ),
        );
```

`Artist` is already in the `crate::common::{...}` import block (used by `goto_artist`). `GenreContentView` is already imported.

- [ ] **Step 2: Verify the build**

```bash
fusermount -u /home/sam/Projects/euphonica/.flatpak-builder/rofiles/* 2>/dev/null
rm -rf /home/sam/Projects/euphonica/.flatpak-builder/rofiles
CARGO_BUILD_JOBS=4 flatpak-builder --force-clean --user --install --install-deps-from=flathub --repo=repo build-flatpak io.github.htkhiem.Euphonica-dev.json > /tmp/flatpak-task3-build.log 2>&1
```

**Wait for the build to complete.** Confirm exit code 0.

- [ ] **Step 3: Commit**

```bash
git add src/window.rs
git commit -m "Route GenreContentView artist-clicked to goto_artist"
```

---

## Task 4: Manual smoke test (handed back to user)

This task is for the user to run, not a subagent. There is no code change.

Pre-requisite: an MPD library where:
- At least one genre has both albums and (multiple) artists.
- (Optional, for membership-rule check): an artist whose AlbumArtist isn't this genre but has tracks tagged with it.

- [ ] **Step 1: Run the dev build**

```bash
flatpak run --branch=master io.github.htkhiem.Euphonica
```

- [ ] **Step 2: Default state**

Click any genre tile from the Genres view. Expect:
- The Albums toggle is active by default.
- The album grid populates as before.
- The count label reads "N albums".

- [ ] **Step 3: Toggle to Artists**

Click the **Artists** button. Expect:
- The grid switches to artist tiles (avatar + name).
- A brief spinner appears while artists are fetched (first time only).
- The count label reads "N artists".

- [ ] **Step 4: Toggle back and forth**

Click Albums → Artists → Albums. Expect: instant transitions after the first artist load. No re-fetch.

- [ ] **Step 5: Click an artist tile**

In the Artists subview, click an artist. Expect:
- The sidebar switches to **Artists**.
- The existing `ArtistContentView` opens scoped to that artist.

- [ ] **Step 6: Click an album tile**

Navigate back to a genre, click an album in the Albums subview. Expect: sidebar switches to Albums and `AlbumContentView` opens (unchanged from prior behaviour).

- [ ] **Step 7: In-session toggle persistence**

In genre G, set the toggle to Artists. Navigate back to the genre grid, then click a different genre H. Expect: the toggle is still Artists, and H's artists populate.

- [ ] **Step 8: App restart resets to Albums**

Quit and re-run. Click any genre. Expect: Albums is the active toggle.

- [ ] **Step 9: Membership rule check**

Pick a genre G. Note the artist list. The artists shown should include ANY artist who appears in any track of any song tagged with G — not just AlbumArtists. If your library has a "featured artist" relationship that contributes only as `Artist` (not `AlbumArtist`), that featured artist should appear.

- [ ] **Step 10: Empty subviews**

If you have a genre with at least one album but no track artists (rare but possible), or vice versa, verify that the empty subview shows its "No Albums" / "No Artists" placeholder while the other subview still has content.

- [ ] **Step 11: Done**

If all checks pass, the feature is complete. Branch `feat/genre-view-toggle` is ready to merge.

---

## Self-review checklist (run mentally before declaring plan complete)

- **Spec coverage:**
  - Linked toggle pair, default Albums → Task 2 (UI template + setup_toggle).
  - `library-music-symbolic` / `music-artist-symbolic` icons → Task 2 UI template.
  - Membership rule (any artist with a song in this genre) → Task 1 (`get_artists_by_genre`).
  - `ArtistCell` reuse → Task 2 (`setup_artist_grid`).
  - Click → `goto_artist` → Task 2 emits `artist-clicked`, Task 3 routes to `goto_artist`.
  - Toggle persists in-memory across genre navigations → `last_subview_albums: Cell<bool>` in Task 2; not reset by `unbind()`.
  - App restart resets → `last_subview_albums` lives on the widget instance only; restart creates fresh widget.
  - Each subview has its own ContentStack with placeholder → Task 2 UI template (`albums_stack`, `artists_stack`).
  - `unbind()` cleanup clears both lists, resets `artists_initialized`, does NOT reset persistence cell → Task 2 (`unbind`).
  - Lazy artist fetch on first toggle → Task 2 (`setup_toggle` connect_toggled + `populate_artists` guard).
- **Type / method names recurring across tasks:** `get_artists_by_genre` (Task 1 → Task 2), `artist-clicked` signal (Task 2 → Task 3), `goto_artist` (Task 3 — already exists). All consistent.
- **No placeholders** — verified.
- **Code shown for every code step** — verified (full file replacements for Task 2; precise insertion blocks for Tasks 1 & 3).
