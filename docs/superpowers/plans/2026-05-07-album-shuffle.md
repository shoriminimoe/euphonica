# Album Shuffle Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a one-shot **Shuffle queue** action with two modes — uniform tracks shuffle and shuffle-by-album — triggered by a new `AdwSplitButton` in the queue view's toolbar, with the selected mode persisted in GSettings.

**Architecture:** A new gschema key (`state.queueview.shuffle-mode`) drives a stateful GAction in the queue view; clicking the split button's main action invokes a new `Player::shuffle_queue(mode)` method that finds the boundary at the end of the currently-playing album cluster and either calls MPD's native `shuffle START:` (tracks mode) or builds a client-side reorder of `queue[boundary..]` and applies it via `delete START:END` + `push_multiple` (album mode). Both modes leave the currently-playing album cluster intact at the head of the queue.

**Tech Stack:** Rust 2024, GTK4 + libadwaita via gtk-rs, `rand::seq::SliceRandom`, mpd-rs (htkhiem fork). Build via Meson driving Cargo through Flatpak.

**Spec reference:** `docs/superpowers/specs/2026-05-07-album-shuffle-design.md`.

---

## Pre-flight notes

This codebase has **no automated test harness**. Verification is by Flatpak build + manual smoke test (Task 6 hands back to the user). Each implementation task ends with a clean Flatpak build.

**Build commands:**

```bash
fusermount -u /home/sam/Projects/euphonica/.flatpak-builder/rofiles/* 2>/dev/null
rm -rf /home/sam/Projects/euphonica/.flatpak-builder/rofiles /home/sam/Projects/euphonica/build-flatpak
CARGO_BUILD_JOBS=4 flatpak-builder --force-clean --user --install --install-deps-from=flathub --repo=repo build-flatpak io.github.htkhiem.Euphonica-dev.json > /tmp/flatpak-task<N>-build.log 2>&1
```

Wiping both `rofiles/` and `build-flatpak/` is the most reliable cleanup pattern from prior tasks — bare `rofiles/*` cleanup has left stale state that breaks subsequent runs. `CARGO_BUILD_JOBS=4` prevents OOM on this 15GB-RAM host. Build takes ~3 minutes when cargo deps are cached.

**Branch:** Work on `feat/album-shuffle` (already created from `feat/album-artists-sidebar`). Do NOT switch branches.

**Commit cadence:** Short imperative title per task, no Conventional-Commits prefix. Optional `Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>` trailer.

---

## File map

**New files:** none.

**Modified files:**

- `data/io.github.htkhiem.Euphonica.gschema.xml` — new `shufflemode` top-level enum + new `shuffle-mode` key inside `state.queueview` schema.
- `src/client/wrapper.rs` — add `shuffle_range` and `delete_range` async methods.
- `src/client/connection.rs` — add `Task::ShuffleRange` and `Task::DeleteRange` variants + dispatch arms.
- `src/player/controller.rs` — add `pub enum ShuffleMode` and `pub async fn shuffle_queue(&self, mode: ShuffleMode) -> ClientResult<()>`.
- `src/player/mod.rs` — re-export `ShuffleMode`.
- `src/gtk/player/queue-view.ui` — new `AdwSplitButton id="shuffle_btn"` in the toolbar + new `<menu id="shuffle_menu_model">` with two radio-style items.
- `src/player/queue_view.rs` — add `shuffle_btn` template child, stateful `shuffle-mode` action, GSettings binding, click handler that calls `player.shuffle_queue(...)`, sensitivity binding, tooltip update.

---

## Task 1: GSettings additions

**Files:**
- Modify: `data/io.github.htkhiem.Euphonica.gschema.xml`

This task adds the persisted-state machinery the toggle relies on. No callers exist yet; the build remains green.

- [ ] **Step 1: Add the `shufflemode` enum**

In `data/io.github.htkhiem.Euphonica.gschema.xml`, find the existing top-level enum block (around lines 3-33 — defines `sortby`, `sortdir`, `pcmsource`, `volumeunit`, `titlewrapmode`). After the last existing `<enum>` element and before the first `<schema>`, add:

```xml
	<enum id="io.github.htkhiem.Euphonica.shufflemode">
		<value nick="tracks" value="0"/>
		<value nick="album" value="1"/>
	</enum>
```

- [ ] **Step 2: Add the `shuffle-mode` key to `state.queueview` schema**

The `state.queueview` schema exists at line 409-427 of `data/io.github.htkhiem.Euphonica.gschema.xml` (with existing keys `show-lyrics`, `use-synced-lyrics`, `maximize-lyrics-view`, `sort-direction`). Inside that schema, after the existing `sort-direction` key (around line 426) and before the schema's closing `</schema>` tag (line 427), add:

```xml
		<key name="shuffle-mode" enum="io.github.htkhiem.Euphonica.shufflemode">
			<default>'tracks'</default>
			<summary>Last-selected mode for the queue's Shuffle button</summary>
		</key>
```

- [ ] **Step 3: Verify the build**

```bash
fusermount -u /home/sam/Projects/euphonica/.flatpak-builder/rofiles/* 2>/dev/null
rm -rf /home/sam/Projects/euphonica/.flatpak-builder/rofiles /home/sam/Projects/euphonica/build-flatpak
CARGO_BUILD_JOBS=4 flatpak-builder --force-clean --user --install --install-deps-from=flathub --repo=repo build-flatpak io.github.htkhiem.Euphonica-dev.json > /tmp/flatpak-task1-build.log 2>&1
```

**Wait for the build to complete** (~3 min). Confirm exit code 0 and the log ends with `Pruning cache`. The schema is compiled by `glib-compile-schemas` during the install step.

- [ ] **Step 4: Commit**

```bash
git add data/io.github.htkhiem.Euphonica.gschema.xml
git commit -m "Add shufflemode enum and queue shuffle-mode state key"
```

---

## Task 2: MpdWrapper additions — shuffle_range and delete_range

**Files:**
- Modify: `src/client/wrapper.rs`
- Modify: `src/client/connection.rs`

Adds two thin wrappers around mpd-rs primitives. No callers yet; Task 3 consumes them.

- [ ] **Step 1: Add `Task::ShuffleRange` and `Task::DeleteRange` variants**

In `src/client/connection.rs`, find the `Task` enum definition (around line 320 onwards — the variants like `Add`, `AddMultiple`, `Insert`, `InsertMultiple`, `FindAdd`, etc. are there). Find an appropriate location near other queue-mutating variants (e.g. after `InsertMultiple`) and add:

```rust
    /// Shuffle a contiguous range of the queue server-side. `start` is the
    /// inclusive start position; the range extends to the end of the queue.
    ShuffleRange(u32, Responder<()>),
    /// Delete a contiguous range of the queue server-side, [start, end).
    DeleteRange(u32, u32, Responder<()>),
```

- [ ] **Step 2: Add dispatch arms for the two new tasks**

In the same file, find the task-loop match block (`match task { ... }`) where existing arms like `Task::SwapPos`, `Task::DeleteAtPos`, `Task::ClearQueue` are dispatched (around line 985-989). Add new arms near them:

```rust
                    Task::ShuffleRange(start, resp) => {
                        self.respond_with_client(|c| c.shuffle(start..), resp)
                    }
                    Task::DeleteRange(start, end, resp) => {
                        self.respond_with_client(|c| c.delete(start..end), resp)
                    }
```

The mpd-rs methods used: `Client::shuffle<T: ToQueueRange>` accepts `RangeFrom<u32>` (e.g. `start..`); `Client::delete<T: ToQueueRangeOrPlace>` accepts `Range<u32>` (e.g. `start..end`).

- [ ] **Step 3: Add the wrapper methods to `MpdWrapper`**

In `src/client/wrapper.rs`, find the existing queue-mutation wrappers (e.g. `swap_pos`, `delete_at_pos`, `clear_queue` around lines 755-768). Add near them:

```rust
    pub async fn shuffle_range(&self, start: u32) -> ClientResult<()> {
        let (s, r) = oneshot::channel();
        self.foreground(Task::ShuffleRange(start, s), r).await
    }

    pub async fn delete_range(&self, start: u32, end: u32) -> ClientResult<()> {
        let (s, r) = oneshot::channel();
        self.foreground(Task::DeleteRange(start, end, s), r).await
    }
```

These run on the foreground client because they're interactive queue ops.

- [ ] **Step 4: Verify the build**

```bash
fusermount -u /home/sam/Projects/euphonica/.flatpak-builder/rofiles/* 2>/dev/null
rm -rf /home/sam/Projects/euphonica/.flatpak-builder/rofiles /home/sam/Projects/euphonica/build-flatpak
CARGO_BUILD_JOBS=4 flatpak-builder --force-clean --user --install --install-deps-from=flathub --repo=repo build-flatpak io.github.htkhiem.Euphonica-dev.json > /tmp/flatpak-task2-build.log 2>&1
```

**Wait for the build to complete.** Confirm exit code 0. Expected new warnings: `shuffle_range` and `delete_range` are unused — Task 3 will consume them.

- [ ] **Step 5: Commit**

```bash
git add src/client/wrapper.rs src/client/connection.rs
git commit -m "Add MpdWrapper shuffle_range and delete_range methods"
```

---

## Task 3: `Player::shuffle_queue` and `ShuffleMode` enum

**Files:**
- Modify: `src/player/controller.rs`
- Modify: `src/player/mod.rs`

This task adds the operation logic — boundary detection, group shuffling, dispatch.

- [ ] **Step 1: Add the `ShuffleMode` enum to `controller.rs`**

In `src/player/controller.rs`, near the top of the file (after the existing `use` block and before `mod imp { ... }`), add:

```rust
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ShuffleMode {
    Tracks,
    Album,
}

impl ShuffleMode {
    pub fn from_settings_str(s: &str) -> Self {
        match s {
            "album" => ShuffleMode::Album,
            _ => ShuffleMode::Tracks,
        }
    }

    pub fn as_settings_str(&self) -> &'static str {
        match self {
            ShuffleMode::Tracks => "tracks",
            ShuffleMode::Album => "album",
        }
    }
}
```

The string conversions correspond to the gschema enum nicks defined in Task 1.

- [ ] **Step 2: Add the `shuffle_queue` method to `impl Player`**

In `src/player/controller.rs`, find the existing `pub fn current_song` method (around line 711) — `shuffle_queue` is a related queue-mutation method. Also find the existing `pub async fn rate_current_song` (around line 1805) for a nearby async method as a structural template. Add `shuffle_queue` somewhere logically near the other queue ops; placing it just after `current_song` is fine:

```rust
    pub async fn shuffle_queue(&self, mode: ShuffleMode) -> ClientResult<()> {
        use rand::seq::SliceRandom;

        let queue = self.imp().queue.clone();
        let queue_len = queue.n_items();
        if queue_len == 0 {
            return Ok(());
        }

        // Compute the boundary: first position AFTER the currently-playing
        // album cluster. If nothing is playing, boundary = 0.
        let boundary = match self.current_song() {
            Some(cur_song) => {
                let cur_pos = cur_song.get_queue_pos();
                let cur_comp_id = cur_song
                    .get_info()
                    .album
                    .as_ref()
                    .map(|a| a.get_comp_id().to_owned());
                let mut cluster_end = cur_pos;
                let mut idx = cur_pos + 1;
                while idx < queue_len {
                    let next_song = match queue.item(idx).and_downcast::<Song>() {
                        Some(s) => s,
                        None => break,
                    };
                    let next_comp_id = next_song
                        .get_info()
                        .album
                        .as_ref()
                        .map(|a| a.get_comp_id().to_owned());
                    if next_comp_id != cur_comp_id {
                        break;
                    }
                    cluster_end = idx;
                    idx += 1;
                }
                cluster_end + 1
            }
            None => 0,
        };

        if boundary >= queue_len {
            return Ok(());
        }

        let client = self.client()?;

        match mode {
            ShuffleMode::Tracks => {
                client.shuffle_range(boundary).await?;
            }
            ShuffleMode::Album => {
                // Build groups of contiguous same-comp_id runs in the tail.
                let mut groups: Vec<Vec<Song>> = Vec::new();
                let mut current_group: Vec<Song> = Vec::new();
                let mut current_group_id: Option<String> = None;
                for idx in boundary..queue_len {
                    let song = match queue.item(idx).and_downcast::<Song>() {
                        Some(s) => s,
                        None => continue,
                    };
                    let comp_id = song
                        .get_info()
                        .album
                        .as_ref()
                        .map(|a| a.get_comp_id().to_owned())
                        .unwrap_or_else(|| song.get_uri().to_owned());
                    if Some(&comp_id) == current_group_id.as_ref() {
                        current_group.push(song);
                    } else {
                        if !current_group.is_empty() {
                            groups.push(std::mem::take(&mut current_group));
                        }
                        current_group_id = Some(comp_id);
                        current_group.push(song);
                    }
                }
                if !current_group.is_empty() {
                    groups.push(current_group);
                }

                // Shuffle the groups, preserving in-group track order.
                let mut rng = rand::rng();
                groups.shuffle(&mut rng);

                // Flatten to a Vec<String> of URIs.
                let new_uris: Vec<String> = groups
                    .into_iter()
                    .flatten()
                    .map(|s| s.get_uri().to_owned())
                    .collect();

                // Apply: delete the tail, then push the new order.
                client.delete_range(boundary, queue_len).await?;
                client.add_multi(new_uris, false, None).await?;
            }
        }

        Ok(())
    }
```

A few notes on the code:
- `Song::get_queue_pos()` and `Song::get_uri()` already exist (`common/song.rs:335` and elsewhere).
- `Song::get_info().album` returns `Option<&AlbumInfo>`. `AlbumInfo::get_comp_id()` returns `&str`.
- The fallback when a track has no album: synthesize a per-song unique id from `song.get_uri()`. This guarantees every album-less track is its own one-track group, matching the spec.
- `add_multi` is the existing wrapper at `wrapper.rs:1149`. Signature is `(uris: Vec<String>, recursive: bool, insert_pos: Option<usize>)`. We pass `false` (not recursive) and `None` (append, but since the queue tail is empty after `delete_range`, append == correct position).
- The `client()?` call returns `Result<Rc<MpdWrapper>, ClientError>` — confirm by looking at the existing `rate_current_song` which uses the same pattern.

- [ ] **Step 3: Re-export `ShuffleMode` from `src/player/mod.rs`**

Open `src/player/mod.rs`. Find the line that re-exports types from `controller`:

```rust
pub use controller::{PlaybackFlow, Player};
```

Change it to:

```rust
pub use controller::{PlaybackFlow, Player, ShuffleMode};
```

- [ ] **Step 4: Verify the build**

```bash
fusermount -u /home/sam/Projects/euphonica/.flatpak-builder/rofiles/* 2>/dev/null
rm -rf /home/sam/Projects/euphonica/.flatpak-builder/rofiles /home/sam/Projects/euphonica/build-flatpak
CARGO_BUILD_JOBS=4 flatpak-builder --force-clean --user --install --install-deps-from=flathub --repo=repo build-flatpak io.github.htkhiem.Euphonica-dev.json > /tmp/flatpak-task3-build.log 2>&1
```

**Wait for the build to complete.** Confirm exit code 0. The Task 2 unused warnings (`shuffle_range`, `delete_range`) should now be consumed. New unused warnings: `ShuffleMode` and `shuffle_queue` — Task 5 will consume them.

- [ ] **Step 5: Commit**

```bash
git add src/player/controller.rs src/player/mod.rs
git commit -m "Add Player shuffle_queue method with Tracks/Album modes"
```

---

## Task 4: queue-view.ui — split button + menu

**Files:**
- Modify: `src/gtk/player/queue-view.ui`

This task adds the visible UI element. Without Task 5's wiring, the button is inert (no action handler), but the build succeeds.

- [ ] **Step 1: Add the `AdwSplitButton` to the queue-view toolbar**

Open `src/gtk/player/queue-view.ui`. The relevant block is around lines 42-51:

```xml
                <child type="top">
                  <object class="AdwHeaderBar">
                    <child type="end">
                      <object class="GtkMenuButton" id="edit_menu">
                        <property name="menu-model">edit_menu_model</property>
                        <property name="icon-name">view-more-symbolic</property>
                      </object>
                    </child>
                  </object>
                </child>
```

Insert a new `<child type="end">` block just BEFORE the existing one (so it's a sibling inside the same `AdwHeaderBar`). The result should look like:

```xml
                <child type="top">
                  <object class="AdwHeaderBar">
                    <child type="end">
                      <object class="AdwSplitButton" id="shuffle_btn">
                        <property name="icon-name">media-playlist-shuffle-symbolic</property>
                        <property name="tooltip-text" translatable="true">Shuffle queue</property>
                        <property name="action-name">queue-view.shuffle</property>
                        <property name="menu-model">shuffle_menu_model</property>
                      </object>
                    </child>
                    <child type="end">
                      <object class="GtkMenuButton" id="edit_menu">
                        <property name="menu-model">edit_menu_model</property>
                        <property name="icon-name">view-more-symbolic</property>
                      </object>
                    </child>
                  </object>
                </child>
```

GTK's `AdwHeaderBar` packs `type="end"` children from right-to-left in declaration order, so this places `shuffle_btn` to the LEFT of `edit_menu` (closer to the title) — natural reading order for "queue actions, then more-menu".

- [ ] **Step 2: Add the `shuffle_menu_model` definition**

Find the existing `<menu id="edit_menu_model">` definition (around line 161). Just AFTER the existing menu's closing `</menu>` tag and before the `</template>` closing tag, add:

```xml
    <menu id="shuffle_menu_model">
      <section>
        <item>
          <attribute name="label" translatable="true">Shuffle tracks</attribute>
          <attribute name="action">queue-view.shuffle-mode</attribute>
          <attribute name="target">tracks</attribute>
        </item>
        <item>
          <attribute name="label" translatable="true">Shuffle by album</attribute>
          <attribute name="action">queue-view.shuffle-mode</attribute>
          <attribute name="target">album</attribute>
        </item>
      </section>
    </menu>
```

The two items use a single stateful action `queue-view.shuffle-mode` with different `target` values; GTK renders them as radio-checkable items because they share a stateful action and have explicit targets.

- [ ] **Step 3: Verify the build**

```bash
fusermount -u /home/sam/Projects/euphonica/.flatpak-builder/rofiles/* 2>/dev/null
rm -rf /home/sam/Projects/euphonica/.flatpak-builder/rofiles /home/sam/Projects/euphonica/build-flatpak
CARGO_BUILD_JOBS=4 flatpak-builder --force-clean --user --install --install-deps-from=flathub --repo=repo build-flatpak io.github.htkhiem.Euphonica-dev.json > /tmp/flatpak-task4-build.log 2>&1
```

**Wait for the build to complete.** Confirm exit code 0. The .ui file is consumed at runtime; the gresource bundle compilation happens at build time, so a malformed XML element would fail the build here.

- [ ] **Step 4: Commit**

```bash
git add src/gtk/player/queue-view.ui
git commit -m "Add Shuffle queue split button + menu to queue-view template"
```

---

## Task 5: queue_view.rs — template child, action, GSettings, click handler

**Files:**
- Modify: `src/player/queue_view.rs`

This task wires the split button to the `Player::shuffle_queue` method via a stateful action backed by GSettings.

- [ ] **Step 1: Add imports for `ShuffleMode` and any other needed types**

At the top of `src/player/queue_view.rs`, find the existing `use` block. Add `ShuffleMode` to the existing player import, e.g. change:

```rust
use crate::player::Player;
```

to:

```rust
use crate::player::{Player, ShuffleMode};
```

Also confirm `gio` is in scope (it should be — used by the existing `gio::ActionEntry` and `gio::SimpleActionGroup` calls). And `glib::clone` is already in scope.

- [ ] **Step 2: Add the `shuffle_btn` template child**

In the `imp::QueueView` struct (search for the struct definition at the top of the file), add a new template child field. The exact existing fields will guide where to place it; typically near other toolbar-button fields:

```rust
        #[template_child]
        pub shuffle_btn: TemplateChild<adw::SplitButton>,
```

- [ ] **Step 3: Add the stateful `shuffle-mode` action and the click `shuffle` action**

In the `imp::ObjectImpl::constructed` method, find the existing block where `action_clear_rating` is defined and added to the action group (around line 139-160). After the existing `action_clear_rating` definition, BEFORE the line that constructs `let actions = gio::SimpleActionGroup::new();`, add the following two action definitions:

```rust
            // Stateful action that mirrors the gschema enum string.
            let viz_settings = utils::settings_manager().child("state").child("queueview");
            let initial_mode = viz_settings.string("shuffle-mode").to_string();
            let action_shuffle_mode = gio::SimpleAction::new_stateful(
                "shuffle-mode",
                Some(glib::VariantTy::STRING),
                &initial_mode.to_variant(),
            );
            action_shuffle_mode.connect_activate(clone!(
                #[weak]
                obj,
                move |action, param| {
                    if let Some(s) = param.and_then(|v| v.get::<String>()) {
                        action.set_state(&s.to_variant());
                        let settings = utils::settings_manager()
                            .child("state")
                            .child("queueview");
                        let _ = settings.set_string("shuffle-mode", &s);
                        // Update the split button's tooltip to reflect the new mode.
                        let mode = ShuffleMode::from_settings_str(&s);
                        let tooltip = match mode {
                            ShuffleMode::Tracks => "Shuffle queue · tracks",
                            ShuffleMode::Album => "Shuffle queue · by album",
                        };
                        obj.imp().shuffle_btn.set_tooltip_text(Some(tooltip));
                    }
                }
            ));

            // Plain action: clicking the split button's main face activates this.
            let action_shuffle = gio::ActionEntry::builder("shuffle")
                .activate(clone!(
                    #[weak]
                    obj,
                    move |_, _, _| {
                        glib::spawn_future_local(clone!(
                            #[weak]
                            obj,
                            async move {
                                if let Some(player) = obj.imp().player.upgrade() {
                                    let mode_str = utils::settings_manager()
                                        .child("state")
                                        .child("queueview")
                                        .string("shuffle-mode");
                                    let mode = ShuffleMode::from_settings_str(&mode_str);
                                    let _ = player.shuffle_queue(mode).await;
                                }
                            }
                        ));
                    }
                ))
                .build();
```

You'll also need to ensure the `utils` module is imported at the top of the file (look for an existing `use crate::utils;` or similar — the genre and album views have it).

- [ ] **Step 4: Add the new actions to the action group**

Update the existing `actions.add_action_entries([action_clear_rating]);` line to also add the new entries. Replace it with:

```rust
            // Create a new action group and add actions to it
            let actions = gio::SimpleActionGroup::new();
            actions.add_action_entries([action_clear_rating, action_shuffle]);
            actions.add_action(&action_shuffle_mode);
            self.obj().insert_action_group("queue-view", Some(&actions));
```

(The stateful `SimpleAction` is added separately via `add_action` since it isn't an `ActionEntry`.)

- [ ] **Step 5: Set the initial tooltip and bind sensitivity**

After the action group is inserted (still in `constructed`), add:

```rust
            // Set the initial tooltip from the persisted mode.
            let initial_tooltip = match ShuffleMode::from_settings_str(&initial_mode) {
                ShuffleMode::Tracks => "Shuffle queue · tracks",
                ShuffleMode::Album => "Shuffle queue · by album",
            };
            self.shuffle_btn.set_tooltip_text(Some(initial_tooltip));

            // Sensitivity: enabled only when queue has at least one item.
            // The player exposes a `queue-len` u32 property.
            if let Some(player) = self.player.upgrade() {
                player
                    .bind_property("queue-len", &self.shuffle_btn.get(), "sensitive")
                    .transform_to(|_, n: u32| Some(n > 0))
                    .sync_create()
                    .build();
            }
```

- [ ] **Step 6: Verify the build**

```bash
fusermount -u /home/sam/Projects/euphonica/.flatpak-builder/rofiles/* 2>/dev/null
rm -rf /home/sam/Projects/euphonica/.flatpak-builder/rofiles /home/sam/Projects/euphonica/build-flatpak
CARGO_BUILD_JOBS=4 flatpak-builder --force-clean --user --install --install-deps-from=flathub --repo=repo build-flatpak io.github.htkhiem.Euphonica-dev.json > /tmp/flatpak-task5-build.log 2>&1
```

**Wait for the build to complete.** Confirm exit code 0. The previously-unused `ShuffleMode` and `shuffle_queue` warnings are now consumed.

- [ ] **Step 7: Commit**

```bash
git add src/player/queue_view.rs
git commit -m "Wire Shuffle queue split button to Player::shuffle_queue"
```

---

## Task 6: Manual smoke test (handed back to user)

This task is for the user to run, not a subagent. There is no code change.

Pre-requisite: a running MPD instance with multiple albums, plus a launched dev Flatpak (`flatpak run --branch=master io.github.htkhiem.Euphonica`).

- [ ] **Step 1: Empty queue** — Open the Queue view with an empty queue. The Shuffle split button should be insensitive (greyed out).

- [ ] **Step 2: Single song** — Add one song. Button becomes sensitive. Click main button. Nothing changes (queue length 1, boundary=1, tail empty). No crash, no error toast.

- [ ] **Step 3: Multiple albums, nothing playing** — Queue several full albums (use multi-select in Albums view). Without starting playback, click main button in tracks mode. Verify queue is fully shuffled. Click the dropdown → select "Shuffle by album" → main click. Verify albums are clustered together with album order randomized.

- [ ] **Step 4: Playing track 3 of album A; queue is A, B, C, D** — Start playback at album A's third track. Set mode to "Shuffle by album". Click main. Verify: tracks 1-N of A stay at the head; B/C/D appear after in some random order. Playback continues without interruption.

- [ ] **Step 5: Playing the last track of album A; queue is A, B, C, D** — Same as #4 but with currently-playing track being A's last. Boundary is at end-of-A. B, C, D get shuffled.

- [ ] **Step 6: Single album fills the whole queue** — Queue one album entirely; play any track. Album shuffle is identity (no audible change). Tracks shuffle randomizes within.

- [ ] **Step 7: Tracks with no album tag** — If you have any in your library, queue some interspersed with albumed tracks. In album mode, each no-album track should scatter independently.

- [ ] **Step 8: Same album appearing twice** — Queue album X, then album Y, then album X again. By-album shuffle treats them as two distinct groups.

- [ ] **Step 9: Mode persistence** — Set the dropdown to "Shuffle by album". Quit and relaunch the app. Open Queue view. The dropdown's "Shuffle by album" item should be checked, and the button tooltip should say "Shuffle queue · by album". Main click runs by-album.

- [ ] **Step 10: Tooltip update** — Switch the dropdown back to "Shuffle tracks" without quitting. Verify the button tooltip immediately changes to "Shuffle queue · tracks".

- [ ] **Step 11: Random mode interaction** — Toggle MPD's `random` mode on (the existing `random_btn` in playback controls). Run shuffle-by-album. The visible queue should be reordered as expected; whether playback follows the visible order is up to MPD's random mode (may not). Confirm there's no crash and the queue display is consistent.

- [ ] **Step 12: Done** — If all checks pass, the feature is complete. Branch `feat/album-shuffle` is ready to merge.

---

## Self-review checklist

- **Spec coverage:**
  - New AdwSplitButton in queue toolbar → Task 4 + Task 5.
  - Two modes (Tracks default, Album) → Tasks 1 + 3 + 4 + 5.
  - Currently-playing album cluster preserved → Task 3 (`boundary` calculation).
  - Tracks-mode uses MPD `shuffle START:` → Task 3 + Task 2.
  - Album-mode uses client-side group shuffle + delete-range + push-multiple → Task 3 + Task 2.
  - Mode persists via GSettings → Task 1 + Task 5.
  - Sensitivity bound to queue length → Task 5 Step 5.
  - Tooltip reflects mode → Task 5 Steps 3 + 5.
  - No-album tracks each form a one-track group via URI fallback → Task 3 (the `unwrap_or_else(|| song.get_uri().to_owned())`).
  - Same album in two non-contiguous runs treated as separate groups → Task 3 (contiguous-run grouping logic).
- **Type / method consistency across tasks:** `ShuffleMode` (Task 3 → Task 5), `from_settings_str`/`as_settings_str` (Task 3 → Task 5), `shuffle_range`/`delete_range` (Task 2 → Task 3), `shuffle_queue(mode)` (Task 3 → Task 5), `shuffle_btn` widget id (Task 4 ↔ Task 5), gschema key `shuffle-mode` (Task 1 → Task 5). All consistent.
- **No placeholders** ("TBD", "implement later", "similar to Task N") — verified.
- **Code shown for every code step** — verified.
- **Each task ends with a green build** — verified (intermediate states leave only unused-warning growth that subsequent tasks consume).
