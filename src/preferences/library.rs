use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{CompositeTemplate, gio, glib};
use std::cell::Cell;

use glib::clone;

use crate::utils;

mod imp {
    use super::*;

    #[derive(Debug, Default, CompositeTemplate)]
    #[template(resource = "/io/github/htkhiem/Euphonica/gtk/preferences/library.ui")]
    pub struct LibraryPreferences {
        #[template_child]
        pub sort_nulls_first: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub sort_case_sensitive: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub search_case_sensitive: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub artist_delims: TemplateChild<gtk::TextView>,
        #[template_child]
        pub artist_delims_apply: TemplateChild<gtk::Button>,
        #[template_child]
        pub artist_excepts: TemplateChild<gtk::TextView>,
        #[template_child]
        pub artist_excepts_apply: TemplateChild<gtk::Button>,
        #[template_child]
        pub genre_delims: TemplateChild<gtk::TextView>,
        #[template_child]
        pub genre_delims_apply: TemplateChild<gtk::Button>,
        #[template_child]
        pub genre_excepts: TemplateChild<gtk::TextView>,
        #[template_child]
        pub genre_excepts_apply: TemplateChild<gtk::Button>,
        #[template_child]
        pub n_recent_albums: TemplateChild<adw::SpinRow>,
        #[template_child]
        pub n_recent_artists: TemplateChild<adw::SpinRow>,
        #[template_child]
        pub n_recent_songs: TemplateChild<adw::SpinRow>,
        #[template_child]
        pub pause_recent: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub image_cache_size: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub info_db_size: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub open_cache_folder: TemplateChild<adw::ButtonRow>,
        #[template_child]
        pub refresh_cache_stats_btn: TemplateChild<gtk::Button>,

        #[template_child]
        pub dedup_albums: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub mount_priority_list: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub mount_priority_refresh: TemplateChild<gtk::Button>,

        pub n_async_in_progress: Cell<u8>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for LibraryPreferences {
        const NAME: &'static str = "EuphonicaLibraryPreferences";
        type Type = super::LibraryPreferences;
        type ParentType = adw::PreferencesPage;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
            klass.set_layout_manager_type::<gtk::BinLayout>();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for LibraryPreferences {
        fn constructed(&self) {
            self.parent_constructed();

            self.refresh_cache_stats_btn.connect_clicked(clone!(
                #[weak(rename_to = this)]
                self,
                move |_| {
                    this.obj().refresh_cache_stats();
                }
            ));

            self.open_cache_folder.connect_activated(|_| {
                let _ = open::that(utils::get_app_cache_path());
            });
        }
    }
    impl WidgetImpl for LibraryPreferences {}
    impl PreferencesPageImpl for LibraryPreferences {}
}

glib::wrapper! {
    pub struct LibraryPreferences(ObjectSubclass<imp::LibraryPreferences>)
        @extends adw::PreferencesPage,
    @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Widget;
}

impl Default for LibraryPreferences {
    fn default() -> Self {
        glib::Object::new()
    }
}

impl LibraryPreferences {
    pub fn setup(&self) {
        let imp = self.imp();
        // Populate with current gsettings values
        let settings = utils::settings_manager();
        // Set up library settings
        let library_settings = settings.child("library");
        let sort_nulls_first = imp.sort_nulls_first.get();
        library_settings
            .bind("sort-nulls-first", &sort_nulls_first, "active")
            .build();
        let sort_case_sensitive = imp.sort_case_sensitive.get();
        library_settings
            .bind("sort-case-sensitive", &sort_case_sensitive, "active")
            .build();
        let search_case_sensitive = imp.search_case_sensitive.get();
        library_settings
            .bind("search-case-sensitive", &search_case_sensitive, "active")
            .build();
        library_settings
            .bind("n-recent-albums", &imp.n_recent_albums.get(), "value")
            .build();
        library_settings
            .bind("n-recent-artists", &imp.n_recent_artists.get(), "value")
            .build();
        library_settings
            .bind("n-recent-songs", &imp.n_recent_songs.get(), "value")
            .build();
        library_settings
            .bind("pause-recent", &imp.pause_recent.get(), "active")
            .build();

        // Setup artist section
        let artist_delims_buf = imp.artist_delims.buffer();
        let artist_delims_apply = imp.artist_delims_apply.get();
        artist_delims_buf.set_text(
            &library_settings
                .value("artist-tag-delims")
                .array_iter_str()
                .unwrap()
                .collect::<Vec<&str>>()
                .join("\n"),
        );
        artist_delims_buf.connect_changed(clone!(
            #[weak]
            artist_delims_apply,
            move |_| {
                artist_delims_apply.set_sensitive(true);
            }
        ));
        artist_delims_apply.connect_clicked(clone!(
            #[weak]
            library_settings,
            #[weak]
            artist_delims_buf,
            move |btn| {
                let _ = library_settings.set_value(
                    "artist-tag-delims",
                    &artist_delims_buf
                        .text(
                            &artist_delims_buf.start_iter(),
                            &artist_delims_buf.end_iter(),
                            false,
                        )
                        .to_string()
                        .lines()
                        .collect::<Vec<&str>>()
                        .to_variant(),
                );
                btn.set_sensitive(false);
                // Reinitialise the automaton
                utils::rebuild_artist_delim_automaton();
            }
        ));

        let artist_excepts_buf = imp.artist_excepts.buffer();
        let artist_excepts_apply = imp.artist_excepts_apply.get();
        artist_excepts_buf.set_text(
            &library_settings
                .value("artist-tag-delim-exceptions")
                .array_iter_str()
                .unwrap()
                .collect::<Vec<&str>>()
                .join("\n"),
        );
        artist_excepts_buf.connect_changed(clone!(
            #[weak]
            artist_excepts_apply,
            move |_| {
                artist_excepts_apply.set_sensitive(true);
            }
        ));
        artist_excepts_apply.connect_clicked(clone!(
            #[weak]
            library_settings,
            #[weak]
            artist_excepts_buf,
            move |btn| {
                let _ = library_settings.set_value(
                    "artist-tag-delim-exceptions",
                    &artist_excepts_buf
                        .text(
                            &artist_excepts_buf.start_iter(),
                            &artist_excepts_buf.end_iter(),
                            false,
                        )
                        .to_string()
                        .lines()
                        .collect::<Vec<&str>>()
                        .to_variant(),
                );
                btn.set_sensitive(false);
                // Reinitialise the automaton
                utils::rebuild_artist_delim_exception_automaton();
            }
        ));

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

        // Dedup switch
        library_settings
            .bind("dedup-albums", &imp.dedup_albums.get(), "active")
            .build();

        // Sensitivity: list and refresh button greyed when dedup is off.
        let dedup_row = imp.dedup_albums.get();
        let list = imp.mount_priority_list.get();
        let refresh_btn = imp.mount_priority_refresh.get();
        let initial_on = dedup_row.is_active();
        list.set_sensitive(initial_on);
        refresh_btn.set_sensitive(initial_on);
        dedup_row.connect_active_notify(clone!(
            #[weak]
            list,
            #[weak]
            refresh_btn,
            move |row| {
                let on = row.is_active();
                list.set_sensitive(on);
                refresh_btn.set_sensitive(on);
            }
        ));

        // Populate the mount list from the wrapper's MountRegistry.
        self.repopulate_mount_list();

        refresh_btn.connect_clicked(clone!(
            #[weak(rename_to = this)]
            self,
            move |_| {
                this.refresh_mount_list_async();
            }
        ));
    }

    fn repopulate_mount_list(&self) {
        let imp = self.imp();
        let list = imp.mount_priority_list.get();
        // Clear
        while let Some(child) = list.first_child() {
            list.remove(&child);
        }
        let app = gio::Application::default()
            .and_then(|a| a.downcast::<crate::application::EuphonicaApplication>().ok());
        let Some(app) = app else { return };
        let client = app.get_client();
        let library_settings = utils::settings_manager().child("library");
        let priority: Vec<String> = library_settings
            .strv("mount-priority")
            .iter()
            .map(|s| s.to_string())
            .collect();
        let known: Vec<crate::client::mounts::Mount> = client.mounts().known().to_vec();
        if known.is_empty() {
            let row = adw::ActionRow::builder()
                .title("No mounts detected")
                .subtitle("Only the root storage is in use.")
                .selectable(false)
                .build();
            list.append(&row);
            return;
        }
        // Order: known mounts in priority order first, then any not in priority.
        let mut ordered: Vec<crate::client::mounts::Mount> = Vec::new();
        for name in &priority {
            if let Some(m) = known.iter().find(|m| &m.name == name) {
                ordered.push(m.clone());
            }
        }
        for m in &known {
            if !ordered.iter().any(|o| o.name == m.name) {
                ordered.push(m.clone());
            }
        }
        for m in &ordered {
            let row = adw::ActionRow::builder()
                .title(&m.name)
                .subtitle(&m.storage)
                .build();
            // Drag handle
            let handle = gtk::Image::from_icon_name("list-drag-handle-symbolic");
            handle.set_tooltip_text(Some("Drag to reorder"));
            row.add_prefix(&handle);
            list.append(&row);
        }
        self.wire_drag_reorder();
    }

    fn wire_drag_reorder(&self) {
        let list = self.imp().mount_priority_list.get();
        let mut child = list.first_child();
        while let Some(c) = child {
            let next = c.next_sibling();
            if let Some(row) = c.downcast_ref::<adw::ActionRow>() {
                attach_drag_source(row);
                attach_drop_target(row, &list);
            }
            child = next;
        }
    }

    fn refresh_mount_list_async(&self) {
        let app = gio::Application::default()
            .and_then(|a| a.downcast::<crate::application::EuphonicaApplication>().ok());
        let Some(app) = app else { return };
        let client = app.get_client();
        glib::spawn_future_local(clone!(
            #[weak(rename_to = this)]
            self,
            #[strong]
            client,
            async move {
                if let Err(e) = client.refresh_mounts().await {
                    eprintln!("[prefs] refresh_mounts failed: {e:?}");
                }
                this.repopulate_mount_list();
            }
        ));
    }

    pub fn refresh_cache_stats(&self) {
        // Avoid spawning additional tasks when current ones have not concluded yet
        if self.imp().n_async_in_progress.get() == 0 {
            self.imp().image_cache_size.set_subtitle("Computing...");
            self.imp().info_db_size.set_subtitle("Computing...");
            self.imp().n_async_in_progress.set(3);

            gio::File::for_path(utils::get_image_cache_path()).measure_disk_usage_async(
                gio::FileMeasureFlags::NONE,
                glib::source::Priority::DEFAULT,
                Option::<&gio::Cancellable>::None,
                None,
                clone!(
                    #[weak(rename_to = this)]
                    self,
                    move |res: Result<(u64, u64, u64), glib::error::Error>| {
                        if let Ok((bytes, _, n_files)) = res {
                            let size_str = glib::format_size(bytes);
                            this.imp()
                                .image_cache_size
                                .set_subtitle(&format!("{n_files} file(s) ({size_str})"));
                        }
                        this.imp()
                            .n_async_in_progress
                            .set(this.imp().n_async_in_progress.get() - 1);
                    }
                ),
            );

            gio::File::for_path(utils::get_doc_cache_path()).measure_disk_usage_async(
                gio::FileMeasureFlags::NONE,
                glib::source::Priority::DEFAULT,
                Option::<&gio::Cancellable>::None,
                None,
                clone!(
                    #[weak(rename_to = this)]
                    self,
                    move |res: Result<(u64, u64, u64), glib::error::Error>| {
                        if let Ok((bytes, _, _)) = res {
                            this.imp()
                                .info_db_size
                                .set_subtitle(&glib::format_size(bytes));
                        }
                        this.imp()
                            .n_async_in_progress
                            .set(this.imp().n_async_in_progress.get() - 1);
                    }
                ),
            );
        }
    }
}

fn attach_drag_source(row: &adw::ActionRow) {
    let drag = gtk::DragSource::new();
    drag.set_actions(gtk::gdk::DragAction::MOVE);
    drag.connect_prepare(clone!(
        #[weak]
        row,
        #[upgrade_or]
        None,
        move |_, _, _| {
            let title = row.title().to_string();
            Some(gtk::gdk::ContentProvider::for_value(&title.to_value()))
        }
    ));
    row.add_controller(drag);
}

fn attach_drop_target(row: &adw::ActionRow, list: &gtk::ListBox) {
    let drop = gtk::DropTarget::new(glib::types::Type::STRING, gtk::gdk::DragAction::MOVE);
    drop.connect_drop(clone!(
        #[weak]
        row,
        #[weak]
        list,
        #[upgrade_or]
        false,
        move |_, value, _, _| {
            let Ok(name) = value.get::<String>() else {
                return false;
            };
            // Find source row by title; remove it; insert before `row`.
            let mut src: Option<adw::ActionRow> = None;
            let mut child = list.first_child();
            while let Some(c) = child {
                let next = c.next_sibling();
                if let Some(r) = c.downcast_ref::<adw::ActionRow>() {
                    if r.title() == name {
                        src = Some(r.clone());
                        break;
                    }
                }
                child = next;
            }
            let Some(src) = src else { return false };
            if src == row {
                return false;
            }
            list.remove(&src);
            let target_idx = row.index();
            list.insert(&src, target_idx);
            // Persist new order.
            let new_priority = collect_mount_order(&list);
            let priority_strs: Vec<&str> = new_priority.iter().map(|s| s.as_str()).collect();
            let _ = utils::settings_manager()
                .child("library")
                .set_strv("mount-priority", priority_strs);
            true
        }
    ));
    row.add_controller(drop);
}

fn collect_mount_order(list: &gtk::ListBox) -> Vec<String> {
    let mut out = Vec::new();
    let mut child = list.first_child();
    while let Some(c) = child {
        let next = c.next_sibling();
        if let Some(r) = c.downcast_ref::<adw::ActionRow>() {
            out.push(r.title().to_string());
        }
        child = next;
    }
    out
}
